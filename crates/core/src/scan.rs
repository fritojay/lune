use std::collections::{HashMap, HashSet};
use std::io;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::mpsc::sync_channel;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, named_params};

use crate::normalize;
use crate::query::ALBUM_TRACK_ORDER;
use crate::tags::{READER_VERSION, TrackMeta, is_audio_path, read_track_meta};
use crate::{CoreError, Result};

const BATCH_SIZE: usize = 500;

/// A directory modified this recently can change again within the same
/// mtime tick (coarse on FAT and some network filesystems) without its mtime
/// moving, so the next scan doesn't skip it. "Recently" is judged against the
/// newest mtime the walk saw as well as our clock: a network share's clock
/// can lag ours, but it had reached that mtime.
const RACY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Skips directories whose mtime hasn't changed since the last scan: one
    /// stat per directory instead of per file. A directory's mtime changes
    /// when entries are added, removed, or renamed, but not when a file in it
    /// is rewritten in place, so such edits are missed.
    Quick,
    /// Stats every file.
    Thorough,
}

/// How far a scan has got, reported as it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanProgress {
    /// Listing directories: audio files found so far.
    Listing { files: usize },
    /// Reading the tags of new and changed files.
    Reading { done: usize, total: usize },
}

#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    pub root: PathBuf,
    pub files_found: usize,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    /// Tracks newly marked missing. They are kept, hidden from the library,
    /// and come back as they were if the file reappears.
    pub missing: usize,
    /// Missing tracks found again, unchanged.
    pub restored: usize,
    /// Unchanged tracks that got a date added (rows from before schema v5).
    pub dated: usize,
    /// What the scan saw but didn't add, in no particular order.
    pub left_out: Vec<LeftOut>,
    /// Directories a quick scan skipped as unchanged. None held a file whose
    /// tags failed or that couldn't be read when it was last listed, as such a
    /// directory is listed again, so only links and non-UTF-8 names can be
    /// left out unseen in one.
    pub dirs_skipped: HashSet<PathBuf>,
    /// The scan was asked to stop before it finished. A stop while listing
    /// changes nothing; one while reading tags keeps what was read, and the
    /// next scan lists the unfinished directories again.
    pub stopped: bool,
    /// Every directory reached was listed (it wasn't stopped while listing),
    /// so the links, names, and unreadable entries in `left_out`, and
    /// `dirs_skipped`, are all there. Tags failures are all there only if it
    /// wasn't stopped at all.
    pub listed: bool,
    pub walk_time: Duration,
    pub total_time: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftOut {
    pub path: PathBuf,
    pub reason: LeftOutReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeftOutReason {
    /// The file was opened but its tags couldn't be read, for this reason.
    /// It is retried by every scan.
    Tags(String),
    /// An audio file that is a link, which the walk doesn't follow.
    Link,
    /// An audio file whose name isn't valid UTF-8.
    NonUtf8Name,
    /// A file, directory, or entry that couldn't be opened or read, for this
    /// reason. Tracks in or under it are kept as they are rather than marked
    /// missing, and it is retried by every scan.
    Unreadable(String),
}

/// How many of a list of [`LeftOut`] there are for each reason.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LeftOutCounts {
    pub tags: usize,
    pub links: usize,
    pub non_utf8_names: usize,
    pub unreadable: usize,
}

impl LeftOutCounts {
    pub fn of(files: &[LeftOut]) -> Self {
        let mut counts = Self::default();
        for file in files {
            *match file.reason {
                LeftOutReason::Tags(_) => &mut counts.tags,
                LeftOutReason::Link => &mut counts.links,
                LeftOutReason::NonUtf8Name => &mut counts.non_utf8_names,
                LeftOutReason::Unreadable(_) => &mut counts.unreadable,
            } += 1;
        }
        counts
    }
}

impl ScanReport {
    fn left_out(&mut self, path: PathBuf, reason: LeftOutReason) {
        self.left_out.push(LeftOut { path, reason });
    }

    /// Logs the counts: the lists can be long, and each entry was logged as
    /// it was found.
    fn log(&self, message: &str) {
        tracing::info!(
            root = %self.root.display(),
            found = self.files_found,
            added = self.added,
            updated = self.updated,
            unchanged = self.unchanged,
            missing = self.missing,
            restored = self.restored,
            dated = self.dated,
            left_out = self.left_out.len(),
            dirs_skipped = self.dirs_skipped.len(),
            stopped = self.stopped,
            listed = self.listed,
            walk_time = ?self.walk_time,
            total_time = ?self.total_time,
            "{message}"
        );
    }
}

/// Rows a scan changed, for [`refresh_albums`] to bring up to date.
#[derive(Default)]
pub(crate) struct Touched {
    pub(crate) albums: HashSet<i64>,
    /// Covers that may have lost their last track.
    pub(crate) art: HashSet<i64>,
}

pub(crate) struct FoundFile {
    pub(crate) path: String,
    pub(crate) mtime_ns: i64,
    pub(crate) size: i64,
    pub(crate) added_ns: Option<i64>,
    /// Whether the DB already has a row for this path.
    pub(crate) known: bool,
}

/// Scans `root` and brings the DB in line with it. Only files under `root` are
/// added, updated, or marked missing.
///
/// Work is committed as it goes, so a scan that stops early keeps it: tags
/// already read aren't read again, and directories whose files were all
/// saved are skipped by the next quick scan. A directory's stored mtime is
/// only written together with its files and missing marks, as a later quick
/// scan trusts all three.
pub(crate) fn scan(
    conn: &mut Connection,
    root: &Path,
    mode: ScanMode,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<ScanReport> {
    scan_using(conn, root, mode, read_track_meta, progress)
}

/// [`scan`] with the tag reader as a parameter, so tests can inject tags or failures.
pub(crate) fn scan_using(
    conn: &mut Connection,
    root: &Path,
    mode: ScanMode,
    reader: impl Fn(&Path) -> Result<TrackMeta> + Sync,
    progress: &mut dyn FnMut(ScanProgress),
) -> Result<ScanReport> {
    scan_until(conn, root, mode, reader, progress, &|| false)
}

/// [`scan_using`] that stops soon after `stop` returns true (see
/// [`ScanReport::stopped`]).
pub(crate) fn scan_until(
    conn: &mut Connection,
    root: &Path,
    mode: ScanMode,
    reader: impl Fn(&Path) -> Result<TrackMeta> + Sync,
    progress: &mut dyn FnMut(ScanProgress),
    stop: &dyn Fn() -> bool,
) -> Result<ScanReport> {
    let started = Instant::now();
    let now = SystemTime::now();
    let root = std::fs::canonicalize(root).map_err(|source| CoreError::Io {
        path: root.to_path_buf(),
        source,
    })?;
    if !root.is_dir() {
        return Err(CoreError::NotADirectory(root));
    }
    let mut report = ScanReport {
        root: root.clone(),
        ..ScanReport::default()
    };

    let mut known = existing_tracks(conn, &root)?;
    let mut stored_dirs = stored_dirs(conn, &root)?;
    // A quick scan lists the directories of stale tracks, so they are re-read.
    for (path, stored) in &known {
        if stored.stale
            && stored.missing_ns.is_none()
            && let Some(mtime) = parent_str(path).and_then(|dir| stored_dirs.get_mut(dir))
        {
            *mtime = 0;
        }
    }
    let walked = walk(&root, &stored_dirs, mode, now, &mut report, progress, stop);
    report.walk_time = started.elapsed();
    // A walk cut short didn't see every file, so it can't tell which are missing.
    if walked.stopped {
        report.stopped = true;
        report.total_time = started.elapsed();
        report.log("scan stopped while listing");
        return Ok(report);
    }
    report.listed = true;

    let mut to_read = Vec::new();
    let mut undated = Vec::new();
    let mut restored = Vec::new();
    let mut found = walked.files.len();
    for mut file in walked.files {
        match known.remove(&file.path) {
            Some(stored)
                if !stored.stale
                    && stored.mtime_ns == file.mtime_ns
                    && stored.size == file.size =>
            {
                if stored.missing_ns.is_some() {
                    restored.push(file.path.clone());
                } else {
                    report.unchanged += 1;
                }
                if !stored.dated {
                    undated.extend(file.added_ns.map(|added| (file.path, added)));
                }
            }
            Some(_) => {
                file.known = true;
                to_read.push(file);
            }
            None => to_read.push(file),
        }
    }
    // Whatever is left in `known` was not seen on disk: it is in a skipped
    // directory, in one that could not be read, or gone.
    let mut missing = Vec::new();
    for (path, stored) in known {
        let p = Path::new(&path);
        if stored.missing_ns.is_some() || walked.unreadable.covers(p) {
            continue;
        }
        if p.parent().is_some_and(|dir| walked.skipped.contains(dir)) {
            report.unchanged += 1;
            found += 1;
        } else {
            missing.push(path);
        }
    }
    report.files_found = found;

    // Directories wait for their files to be read before they are saved.
    let mut checkpoints = Checkpoints::default();
    for file in &to_read {
        if let Some(dir) = parent_str(&file.path) {
            *checkpoints.pending.entry(dir.to_owned()).or_default() += 1;
        }
    }

    let tx = conn.transaction()?;
    let mut touched = Touched::default();
    for (path, added_ns) in &undated {
        report.dated += tx
            .prepare_cached(
                "UPDATE tracks SET added_ns = :added_ns WHERE path = :path AND added_ns IS NULL",
            )?
            .execute(named_params! { ":path": path, ":added_ns": added_ns })?;
    }
    for path in &restored {
        let album: Option<i64> = tx
            .prepare_cached(
                "UPDATE tracks SET missing_ns = NULL WHERE path = :path RETURNING album_id",
            )?
            .query_row(named_params! { ":path": path }, |row| row.get("album_id"))
            .optional()?;
        if let Some(album_id) = album {
            touched.albums.insert(album_id);
            report.restored += 1;
        }
    }
    for path in &missing {
        let album: Option<i64> = tx
            .prepare_cached(
                "UPDATE tracks SET missing_ns = :missing_ns WHERE path = :path RETURNING album_id",
            )?
            .query_row(
                named_params! { ":path": path, ":missing_ns": unix_ns(now) },
                |row| row.get("album_id"),
            )
            .optional()?;
        if let Some(album_id) = album {
            touched.albums.insert(album_id);
            report.missing += 1;
        }
    }
    // Forgetting a directory only costs listing it again, so stale rows go
    // now; the listed ones come back as they are finished.
    let mut forget = tx.prepare_cached("DELETE FROM dirs WHERE path = :path")?;
    for dir in stored_dirs.keys() {
        if !walked.skipped.contains(Path::new(dir)) {
            forget.execute(named_params! { ":path": dir })?;
        }
    }
    drop(forget);
    report.dirs_skipped = walked.skipped;
    // Every directory reached gets a row now, as a quick scan finds the
    // subdirectories of a skipped one by their rows. Unfinished ones are
    // stored as 0 (list again) until their files are saved.
    for (dir, mtime) in walked.listed {
        if checkpoints.pending.contains_key(&dir) {
            save_dir(&tx, &dir, 0)?;
            checkpoints.listed.insert(dir, mtime);
        } else {
            save_dir(&tx, &dir, mtime)?;
        }
    }
    refresh_albums(&tx, &touched)?;
    tx.commit()?;

    read_and_write(
        conn,
        to_read,
        &reader,
        &mut checkpoints,
        &mut report,
        progress,
        stop,
    )?;

    report.total_time = started.elapsed();
    report.log("scan finished");
    Ok(report)
}

/// Removes the tracks that are under none of `folders`. Returns how many,
/// or `None` if any folder can't be resolved (e.g. an unmounted drive): its
/// tracks can't be told apart then, so nothing is removed.
pub(crate) fn prune(conn: &mut Connection, folders: &[PathBuf]) -> Result<Option<usize>> {
    // Keyed by the path as configured, so unlike tracks these can be told
    // apart even while a folder is unavailable.
    forget_folder_scans(conn, folders)?;
    // Tracks are stored under the canonical root the scan walked.
    let mut roots = Vec::with_capacity(folders.len() * 2);
    for folder in folders {
        match std::fs::canonicalize(folder) {
            Ok(canonical) => roots.push(canonical),
            Err(err) => {
                tracing::warn!(%err, folder = %folder.display(), "folder unavailable; nothing pruned");
                return Ok(None);
            }
        }
        roots.push(folder.clone());
    }
    let inside = |path: &str| roots.iter().any(|root| Path::new(path).starts_with(root));
    let tx = conn.transaction()?;
    let outside: Vec<(i64, i64, Option<i64>)> = {
        let mut stmt = tx.prepare("SELECT id, album_id, art_id, path FROM tracks")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>("id")?,
                row.get::<_, i64>("album_id")?,
                row.get::<_, Option<i64>>("art_id")?,
                row.get::<_, String>("path")?,
            ))
        })?;
        let mut outside = Vec::new();
        for row in rows {
            let (id, album_id, art_id, path) = row?;
            if !inside(&path) {
                outside.push((id, album_id, art_id));
            }
        }
        outside
    };
    let mut touched = Touched::default();
    {
        let mut delete = tx.prepare_cached("DELETE FROM tracks WHERE id = :id")?;
        for (id, album_id, art_id) in &outside {
            delete.execute(named_params! { ":id": id })?;
            touched.albums.insert(*album_id);
            touched.art.extend(*art_id);
        }
    }
    {
        let mut stmt = tx.prepare("SELECT path FROM dirs")?;
        let dirs: Vec<String> = stmt
            .query_map([], |row| row.get("path"))?
            .collect::<rusqlite::Result<_>>()?;
        let mut delete = tx.prepare_cached("DELETE FROM dirs WHERE path = :path")?;
        for dir in dirs.iter().filter(|dir| !inside(dir)) {
            delete.execute(named_params! { ":path": dir })?;
        }
    }
    refresh_albums(&tx, &touched)?;
    tx.commit()?;
    if !outside.is_empty() {
        tracing::info!(
            removed = outside.len(),
            "pruned tracks outside the library folders"
        );
    }
    Ok(Some(outside.len()))
}

pub(crate) fn record_folder_scan(conn: &Connection, folder: &Path, at: SystemTime) -> Result<()> {
    let Some(folder) = folder.to_str() else {
        return Ok(());
    };
    conn.prepare_cached(
        "INSERT OR REPLACE INTO folder_scans (path, scanned_ns) VALUES (:path, :scanned_ns)",
    )?
    .execute(named_params! { ":path": folder, ":scanned_ns": unix_ns(at) })?;
    Ok(())
}

/// Deletes the scan times of folders not in `folders`.
fn forget_folder_scans(conn: &Connection, folders: &[PathBuf]) -> Result<()> {
    let keep: HashSet<&str> = folders.iter().filter_map(|f| f.to_str()).collect();
    let mut stmt = conn.prepare("SELECT path FROM folder_scans")?;
    let recorded: Vec<String> = stmt
        .query_map([], |row| row.get("path"))?
        .collect::<rusqlite::Result<_>>()?;
    let mut delete = conn.prepare_cached("DELETE FROM folder_scans WHERE path = :path")?;
    for folder in recorded.iter().filter(|f| !keep.contains(f.as_str())) {
        delete.execute(named_params! { ":path": folder })?;
    }
    Ok(())
}

pub(crate) fn folder_scans(conn: &Connection, folders: &[PathBuf]) -> Result<Vec<Option<i64>>> {
    let mut stmt = conn.prepare_cached("SELECT scanned_ns FROM folder_scans WHERE path = :path")?;
    let mut scans = Vec::with_capacity(folders.len());
    for folder in folders {
        scans.push(match folder.to_str() {
            Some(path) => stmt
                .query_row(named_params! { ":path": path }, |row| row.get("scanned_ns"))
                .optional()?,
            None => None,
        });
    }
    Ok(scans)
}

/// Deletes the missing tracks in `ids`, or every missing track for `None`.
/// Tracks that are present are never deleted. Returns how many were.
pub(crate) fn forget_missing(conn: &mut Connection, ids: Option<&[i64]>) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut deleted: Vec<(i64, Option<i64>)> = Vec::new();
    {
        let columns = |row: &rusqlite::Row<'_>| Ok((row.get("album_id")?, row.get("art_id")?));
        match ids {
            None => {
                let mut stmt = tx.prepare(
                    "DELETE FROM tracks WHERE missing_ns IS NOT NULL RETURNING album_id, art_id",
                )?;
                for row in stmt.query_map([], columns)? {
                    deleted.push(row?);
                }
            }
            Some(ids) => {
                let mut stmt = tx.prepare(
                    "DELETE FROM tracks WHERE id = :id AND missing_ns IS NOT NULL
                     RETURNING album_id, art_id",
                )?;
                for id in ids {
                    deleted.extend(
                        stmt.query_row(named_params! { ":id": id }, columns)
                            .optional()?,
                    );
                }
            }
        }
    }
    let mut touched = Touched::default();
    for (album_id, art_id) in &deleted {
        touched.albums.insert(*album_id);
        touched.art.extend(*art_id);
    }
    refresh_albums(&tx, &touched)?;
    tx.commit()?;
    Ok(deleted.len())
}

#[derive(Default)]
struct Walked {
    files: Vec<FoundFile>,
    unreadable: Unreadable,
    /// Directories a quick scan skipped: the DB's tracks directly in them stand.
    skipped: HashSet<PathBuf>,
    /// The directories reached and not skipped, with the mtime to store for
    /// each (0: don't skip it next time).
    listed: Vec<(String, i64)>,
    /// The newest mtime of anything stat'ed.
    newest: Option<SystemTime>,
    /// Asked to stop before every directory was reached.
    stopped: bool,
}

/// Paths the walk failed to read, so their DB rows must be left alone.
#[derive(Default)]
struct Unreadable {
    paths: Vec<PathBuf>,
}

impl Unreadable {
    fn covers(&self, path: &Path) -> bool {
        self.paths.iter().any(|p| path.starts_with(p))
    }
}

/// Walks `root` without following symlinks inside it (a link can point
/// anywhere on the system). `root` itself has already been resolved. A quick
/// scan doesn't list a directory whose mtime matches `stored`; it goes on to
/// the subdirectories stored for it, which can't have changed either.
fn walk(
    root: &Path,
    stored: &HashMap<String, i64>,
    mode: ScanMode,
    now: SystemTime,
    report: &mut ScanReport,
    progress: &mut dyn FnMut(ScanProgress),
    stop: &dyn Fn() -> bool,
) -> Walked {
    let mut children: HashMap<&Path, Vec<&Path>> = HashMap::new();
    for dir in stored.keys() {
        let dir = Path::new(dir);
        if let Some(parent) = dir.parent() {
            children.entry(parent).or_default().push(dir);
        }
    }
    let mut walked = Walked::default();
    // Each directory's mtime, if it was listed completely. Whether it is
    // stored depends on the newest mtime, known once the walk is done.
    let mut reached: Vec<(String, Option<SystemTime>)> = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        if stop() {
            walked.stopped = true;
            break;
        }
        let meta = match std::fs::symlink_metadata(&dir) {
            Ok(meta) => meta,
            // A stored subdirectory that is gone: its tracks are missing.
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                walk_error(report, &mut walked.unreadable, &dir, &err);
                // Keep a row so a quick scan of its parent still visits it.
                reached.extend(dir.to_str().map(|k| (k.to_owned(), None)));
                continue;
            }
        };
        if !meta.is_dir() {
            continue;
        }
        let modified = meta.modified().ok();
        walked.newest = walked.newest.max(modified);
        let mtime = modified.map_or(0, unix_ns);
        let key = dir.to_str().map(str::to_owned);
        let unchanged = mtime != 0 && key.as_ref().and_then(|k| stored.get(k)) == Some(&mtime);
        if mode == ScanMode::Quick && unchanged {
            let subdirs = children.get(dir.as_path()).into_iter().flatten();
            pending.extend(subdirs.map(|d| d.to_path_buf()));
            walked.skipped.insert(dir);
            continue;
        }
        let complete = list_dir(&dir, &mut walked, &mut pending, report);
        if let Some(key) = key {
            reached.push((key, modified.filter(|_| complete)));
        }
        progress(ScanProgress::Listing {
            files: walked.files.len(),
        });
    }
    let reference = walked.newest.map_or(now, |newest| newest.min(now));
    let settled = |m: &SystemTime| reference.duration_since(*m).is_ok_and(|age| age >= RACY);
    walked.listed = reached
        .into_iter()
        .map(|(key, modified)| (key, modified.filter(settled).map_or(0, unix_ns)))
        .collect();
    walked
}

/// Lists `dir`: audio files go to `walked.files`, subdirectories onto
/// `pending`. Returns whether everything in it could be read.
fn list_dir(
    dir: &Path,
    walked: &mut Walked,
    pending: &mut Vec<PathBuf>,
    report: &mut ScanReport,
) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            walk_error(report, &mut walked.unreadable, dir, &err);
            return false;
        }
    };
    let mut complete = true;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            // Stops at the first error: on Unix the listing ends there
            // anyway, and on Windows the next call can fail the same way
            // again and again. The directory is left out once.
            Err(err) => {
                walk_error(report, &mut walked.unreadable, dir, &err);
                complete = false;
                break;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) => {
                walk_error(report, &mut walked.unreadable, &path, &err);
                complete = false;
                continue;
            }
        };
        if file_type.is_dir() {
            pending.push(path);
            continue;
        }
        if !file_type.is_file() || !is_audio_path(&path) {
            if file_type.is_symlink() && is_audio_path(&path) {
                tracing::info!(path = %path.display(), "skipping linked file");
                report.left_out(path, LeftOutReason::Link);
            }
            continue;
        }
        // Doesn't follow links, but `path` is not one.
        let (mtime, added, size) = match entry.metadata() {
            Ok(meta) => (meta.modified(), added_time(&meta), meta.len()),
            Err(err) => {
                walk_error(report, &mut walked.unreadable, &path, &err);
                complete = false;
                continue;
            }
        };
        walked.newest = walked.newest.max(mtime.as_ref().ok().copied());
        let path = match path.into_os_string().into_string() {
            Ok(path) => path,
            Err(path) => {
                let path = PathBuf::from(path);
                tracing::warn!(path = %path.display(), "skipping non-UTF-8 path");
                report.left_out(path, LeftOutReason::NonUtf8Name);
                continue;
            }
        };
        walked.files.push(FoundFile {
            path,
            mtime_ns: mtime_ns(mtime),
            size: i64::try_from(size).unwrap_or(i64::MAX),
            added_ns: added.map(unix_ns),
            known: false,
        });
    }
    complete
}

fn walk_error(report: &mut ScanReport, unreadable: &mut Unreadable, path: &Path, err: &io::Error) {
    tracing::warn!(%err, path = %path.display(), "could not read");
    report.left_out(
        path.to_path_buf(),
        LeftOutReason::Unreadable(err.to_string()),
    );
    unreadable.paths.push(path.to_path_buf());
}

/// The stored directory mtimes for `root` and everything under it.
fn stored_dirs(conn: &Connection, root: &Path) -> Result<HashMap<String, i64>> {
    let Some(root) = root.to_str() else {
        return Ok(HashMap::new());
    };
    let (from, to) = path_prefix_range(root);
    let mut stmt = conn.prepare(
        "SELECT path, mtime_ns FROM dirs
         WHERE path = :root OR (path >= :from AND path < :to)",
    )?;
    let rows = stmt.query_map(
        named_params! { ":root": root, ":from": from, ":to": to },
        |row| Ok((row.get("path")?, row.get("mtime_ns")?)),
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn mtime_ns(modified: io::Result<SystemTime>) -> i64 {
    modified.map_or(0, unix_ns)
}

/// When the file arrived: the earlier of its created and modified times, as
/// a copy that keeps the modified time gets a new created time, and some
/// filesystems have no created time at all.
fn added_time(meta: &std::fs::Metadata) -> Option<SystemTime> {
    [meta.created(), meta.modified()]
        .into_iter()
        .filter_map(io::Result::ok)
        .min()
}

pub(crate) fn unix_ns(t: SystemTime) -> i64 {
    let to_i64 = |d: Duration| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX);
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => to_i64(d),
        Err(before_epoch) => -to_i64(before_epoch.duration()),
    }
}

/// The stored tracks under `root`. A rescan of one directory (the watcher's
/// common case) reads only its rows, through the index on `path`.
struct Stored {
    mtime_ns: i64,
    size: i64,
    dated: bool,
    missing_ns: Option<i64>,
    /// Read by an older [`READER_VERSION`].
    stale: bool,
}

fn existing_tracks(conn: &Connection, root: &Path) -> Result<HashMap<String, Stored>> {
    let (from, to) = match root.to_str().map(path_prefix_range) {
        Some(range) => range,
        // No stored path is under a non-UTF-8 root; read them all and let the
        // filter below decide.
        None => (String::new(), String::from(char::MAX)),
    };
    let mut stmt = conn.prepare(
        "SELECT path, mtime_ns, size, added_ns IS NOT NULL AS dated, missing_ns,
                reader_version < :reader_version AS stale
         FROM tracks WHERE path >= :from AND path < :to",
    )?;
    let params = named_params! { ":from": from, ":to": to, ":reader_version": READER_VERSION };
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>("path")?,
            Stored {
                mtime_ns: row.get("mtime_ns")?,
                size: row.get("size")?,
                dated: row.get("dated")?,
                missing_ns: row.get("missing_ns")?,
                stale: row.get("stale")?,
            },
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, stamp) = row?;
        if Path::new(&path).starts_with(root) {
            out.insert(path, stamp);
        }
    }
    Ok(out)
}

/// The range of strings that start with `root` followed by a separator.
/// SQLite compares text bytewise, and the separator is ASCII, so the bound
/// is the same prefix with the separator's next byte.
fn path_prefix_range(root: &str) -> (String, String) {
    let sep = std::path::MAIN_SEPARATOR;
    let base = root.strip_suffix(sep).unwrap_or(root);
    // MAIN_SEPARATOR is '/' or '\\', so the next char is ASCII too.
    let next = char::from(sep as u8 + 1);
    (format!("{base}{sep}"), format!("{base}{next}"))
}

fn save_dir(tx: &Transaction<'_>, dir: &str, mtime_ns: i64) -> Result<()> {
    tx.prepare_cached("INSERT OR REPLACE INTO dirs (path, mtime_ns) VALUES (:path, :mtime_ns)")?
        .execute(named_params! { ":path": dir, ":mtime_ns": mtime_ns })?;
    Ok(())
}

fn parent_str(path: &str) -> Option<&str> {
    Path::new(path).parent().and_then(Path::to_str)
}

/// Listed directories still waiting for some of their files to be read.
#[derive(Default)]
struct Checkpoints {
    /// Files left to read in each directory.
    pending: HashMap<String, usize>,
    /// The mtime to store for each, once its files are saved.
    listed: HashMap<String, i64>,
    /// Directories with a file whose tags failed: not skipped next time, so
    /// the file is retried.
    failed: HashSet<String>,
}

impl Checkpoints {
    /// Counts `path` as read. Returns its directory and the mtime to store
    /// for it if that was the directory's last file.
    fn read(&mut self, path: &str) -> Option<(String, i64)> {
        let dir = parent_str(path)?;
        let left = self.pending.get_mut(dir)?;
        *left -= 1;
        if *left > 0 {
            return None;
        }
        self.pending.remove(dir);
        let mtime = self.listed.remove(dir)?;
        let mtime = if self.failed.remove(dir) { 0 } else { mtime };
        Some((dir.to_owned(), mtime))
    }
}

fn read_and_write(
    conn: &mut Connection,
    files: Vec<FoundFile>,
    reader: &(impl Fn(&Path) -> Result<TrackMeta> + Sync),
    checkpoints: &mut Checkpoints,
    report: &mut ScanReport,
    progress: &mut dyn FnMut(ScanProgress),
    stop: &dyn Fn() -> bool,
) -> Result<()> {
    let total = files.len();
    if total == 0 {
        return Ok(());
    }
    progress(ScanProgress::Reading { done: 0, total });
    std::thread::scope(|scope| {
        let (sender, receiver) = sync_channel(BATCH_SIZE * 2);
        // One file at a time, so a network share or spinning disk sees one
        // request in flight. Reading runs on its own thread so DB writes
        // overlap it.
        scope.spawn(move || {
            for file in files {
                let meta = read_guarded(reader, Path::new(&file.path));
                // A send error means the writer stopped (DB error); stop reading too.
                if sender.send((file, meta)).is_err() {
                    break;
                }
            }
        });

        let mut batch = Vec::with_capacity(BATCH_SIZE);
        for (done, item) in receiver.into_iter().enumerate() {
            batch.push(item);
            progress(ScanProgress::Reading {
                done: done + 1,
                total,
            });
            if batch.len() >= BATCH_SIZE {
                write_batch(conn, &mut batch, checkpoints, report)?;
            }
            // Dropping the receiver stops the reader. What it read is saved.
            if stop() {
                report.stopped = true;
                break;
            }
        }
        write_batch(conn, &mut batch, checkpoints, report)
    })
}

/// Runs `reader`, turning a panic inside the tag parser (lofty) into an error
/// for this one file, so a malformed file can't abort the whole scan.
fn read_guarded(reader: &impl Fn(&Path) -> Result<TrackMeta>, path: &Path) -> Result<TrackMeta> {
    std::panic::catch_unwind(AssertUnwindSafe(|| reader(path)))
        .unwrap_or_else(|_| Err(CoreError::TagPanic(path.to_path_buf())))
}

/// Saves `batch` with the albums it changed and the directories it finished,
/// so stopping after any batch leaves the DB consistent.
fn write_batch(
    conn: &mut Connection,
    batch: &mut Vec<(FoundFile, Result<TrackMeta>)>,
    checkpoints: &mut Checkpoints,
    report: &mut ScanReport,
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    let mut touched = Touched::default();
    for (file, meta) in batch.drain(..) {
        match meta {
            Ok(meta) => {
                upsert_track(&tx, &file, &meta, &mut touched)?;
                if file.known {
                    report.updated += 1;
                } else {
                    report.added += 1;
                }
            }
            Err(err) => {
                tracing::warn!(%err, "skipping unreadable file");
                checkpoints
                    .failed
                    .extend(parent_str(&file.path).map(str::to_owned));
                let cause = err.file_cause();
                let reason = match err {
                    CoreError::Io { .. } => LeftOutReason::Unreadable(cause),
                    _ => LeftOutReason::Tags(cause),
                };
                report.left_out(PathBuf::from(&file.path), reason);
            }
        }
        if let Some((dir, mtime)) = checkpoints.read(&file.path) {
            save_dir(&tx, &dir, mtime)?;
        }
    }
    refresh_albums(&tx, &touched)?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn upsert_track(
    tx: &Transaction<'_>,
    file: &FoundFile,
    meta: &TrackMeta,
    touched: &mut Touched,
) -> Result<()> {
    let album_artist = meta
        .album_artist
        .as_deref()
        .or(meta.artist.as_deref())
        .unwrap_or("");
    let album_title = meta.album.as_deref().unwrap_or("");
    // The no-op DO UPDATE makes RETURNING yield the existing row's id.
    // Display strings are refreshed afterwards in `refresh_albums`.
    let album_id: i64 = tx
        .prepare_cached(
            "INSERT INTO albums (album_artist, album_artist_key, title, title_key)
             VALUES (:album_artist, :album_artist_key, :title, :title_key)
             ON CONFLICT (album_artist_key, title_key) DO UPDATE SET id = id
             RETURNING id",
        )?
        .query_row(
            named_params! {
                ":album_artist": album_artist,
                ":album_artist_key": normalize::key(album_artist),
                ":title": album_title,
                ":title_key": normalize::key(album_title),
            },
            |row| row.get("id"),
        )?;

    let art_id: Option<i64> = match &meta.art {
        Some(art) => Some(
            tx.prepare_cached(
                "INSERT INTO art (hash, mime) VALUES (:hash, :mime)
                 ON CONFLICT (hash) DO UPDATE SET id = id
                 RETURNING id",
            )?
            .query_row(
                named_params! { ":hash": art.hash, ":mime": art.mime },
                |row| row.get("id"),
            )?,
        ),
        None => None,
    };

    if file.known {
        let old: Option<(i64, Option<i64>)> = tx
            .prepare_cached("SELECT album_id, art_id FROM tracks WHERE path = :path")?
            .query_row(named_params! { ":path": file.path }, |row| {
                Ok((row.get("album_id")?, row.get("art_id")?))
            })
            .optional()?;
        if let Some((old_album, old_art)) = old {
            touched.albums.insert(old_album);
            touched.art.extend(old_art);
        }
    }
    touched.albums.insert(album_id);

    // Upsert keeps the track id stable across rescans (the queue refers to it).
    tx.prepare_cached(
        "INSERT INTO tracks (path, mtime_ns, size, title, artist, album_artist, album_id,
                             disc_no, track_no, year, duration_ms, codec, sample_rate, art_id,
                             album, added_ns, bitrate_kbps, reader_version, genre)
         VALUES (:path, :mtime_ns, :size, :title, :artist, :album_artist, :album_id,
                 :disc_no, :track_no, :year, :duration_ms, :codec, :sample_rate, :art_id,
                 :album, :added_ns, :bitrate_kbps, :reader_version, :genre)
         ON CONFLICT (path) DO UPDATE SET
             mtime_ns = excluded.mtime_ns, size = excluded.size, title = excluded.title,
             artist = excluded.artist, album_artist = excluded.album_artist,
             album_id = excluded.album_id, disc_no = excluded.disc_no,
             track_no = excluded.track_no, year = excluded.year,
             duration_ms = excluded.duration_ms, codec = excluded.codec,
             sample_rate = excluded.sample_rate, art_id = excluded.art_id,
             album = excluded.album, bitrate_kbps = excluded.bitrate_kbps,
             reader_version = excluded.reader_version, genre = excluded.genre,
             added_ns = COALESCE(tracks.added_ns, excluded.added_ns),
             missing_ns = NULL",
    )?
    .execute(named_params! {
        ":path": file.path,
        ":mtime_ns": file.mtime_ns,
        ":size": file.size,
        ":title": meta.title,
        ":artist": meta.artist,
        ":album_artist": meta.album_artist,
        ":album_id": album_id,
        ":disc_no": meta.disc_no,
        ":track_no": meta.track_no,
        ":year": meta.year,
        ":duration_ms": i64::try_from(meta.duration_ms).unwrap_or(i64::MAX),
        ":codec": meta.codec,
        ":sample_rate": meta.sample_rate,
        ":art_id": art_id,
        ":album": meta.album,
        ":added_ns": file.added_ns,
        ":bitrate_kbps": meta.bitrate_kbps,
        ":reader_version": READER_VERSION,
        ":genre": meta.genre,
    })?;
    Ok(())
}

/// Display strings come from the album's first track in album order (the
/// same fallback as identity: album artist, else track artist), so a tag fix
/// such as a capitalization change shows up on rescan.
pub(crate) fn refresh_albums(tx: &Transaction<'_>, touched: &Touched) -> Result<()> {
    let mut delete_art = tx.prepare_cached(
        "DELETE FROM art WHERE id = :id AND NOT EXISTS (SELECT 1 FROM tracks WHERE art_id = :id)",
    )?;
    for id in &touched.art {
        delete_art.execute(named_params! { ":id": id })?;
    }
    if touched.albums.is_empty() {
        return Ok(());
    }
    let first_track = |column: &str| {
        format!(
            "(SELECT {column} FROM tracks t WHERE t.album_id = albums.id AND t.missing_ns IS NULL
              ORDER BY {ALBUM_TRACK_ORDER} LIMIT 1)"
        )
    };
    let mut update = tx.prepare_cached(&format!(
        "UPDATE albums SET
             year_max = (SELECT MAX(year) FROM tracks
                         WHERE album_id = albums.id AND missing_ns IS NULL),
             art_id = (SELECT t.art_id FROM tracks t
                       WHERE t.album_id = albums.id AND t.art_id IS NOT NULL
                         AND t.missing_ns IS NULL
                       ORDER BY {ALBUM_TRACK_ORDER}
                       LIMIT 1),
             genre = (SELECT t.genre FROM tracks t
                      WHERE t.album_id = albums.id AND t.genre IS NOT NULL
                        AND t.missing_ns IS NULL
                      ORDER BY {ALBUM_TRACK_ORDER}
                      LIMIT 1)
         WHERE id = :id"
    ))?;
    // Only rewrites when the strings differ, so `execute` returns 1 exactly
    // when the display changed. Rows from schema v1 have no album tag (NULL)
    // and keep the current title.
    let new_artist = first_track("COALESCE(t.album_artist, t.artist, '')");
    let new_title = first_track("t.album");
    let mut update_display = tx.prepare_cached(&format!(
        "UPDATE albums SET
             album_artist = COALESCE({new_artist}, album_artist),
             title = COALESCE({new_title}, title)
         WHERE id = :id
           AND (album_artist IS NOT COALESCE({new_artist}, album_artist)
                OR title IS NOT COALESCE({new_title}, title))"
    ))?;
    // The FTS rows copy the album strings; the tracks' update trigger rebuilds them.
    let mut reindex =
        tx.prepare_cached("UPDATE tracks SET album_id = album_id WHERE album_id = :id")?;
    let mut delete_empty = tx.prepare_cached(
        "DELETE FROM albums
         WHERE id = :id AND NOT EXISTS (SELECT 1 FROM tracks WHERE album_id = :id)",
    )?;
    for id in &touched.albums {
        let id = named_params! { ":id": id };
        update.execute(id)?;
        if update_display.execute(id)? > 0 {
            reindex.execute(id)?;
        }
        delete_empty.execute(id)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/scan_test.rs"]
mod tests;
