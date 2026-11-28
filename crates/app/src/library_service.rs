//! The library thread: loads the library and saved queue, keeps the DB in
//! line with the configured folders (prune and scan at startup, then file
//! watching and a periodic rescan), and adds or removes folders. It owns its
//! own DB connection; the UI only receives [`Update`]s, and saves the folder
//! list each [`Update::Folders`] carries. No Qt here, so it is tested
//! directly.

use std::mem::{self, Discriminant};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

use lune_core::{
    Catalog, FolderWatcher, LeftOut, LeftOutCounts, LeftOutReason, Library, ListId, MissingTrack,
    Playlist, Queue, ScanMode, ScanProgress, ScanReport,
};

/// How long changes must settle before a watched folder is rescanned.
const WATCH_DELAY: Duration = Duration::from_secs(2);

/// How often every folder is rescanned (quickly), for changes the watcher
/// can't see, such as ones made on a network share by another machine.
pub const RESCAN_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// How often scan progress is sent, at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// The longest the thread sleeps before checking the clock again. The wait is
/// on a monotonic clock, which may not count time the computer was asleep, so
/// the due time is checked against the wall clock.
const MAX_WAIT: Duration = Duration::from_secs(60 * 60);

#[derive(Debug)]
pub enum Job {
    AddFolder(PathBuf),
    RemoveFolder(PathBuf),
    RescanAll,
    /// From the watcher.
    Rescan(Vec<PathBuf>),
    /// Deletes these tracks if they are missing.
    ForgetMissing(Vec<i64>),
    ForgetAllMissing,
}

pub struct SavedPlaylists {
    pub playlists: Vec<Playlist>,
    /// No stored list has it, so a new playlist can.
    pub unused_id: ListId,
}

pub enum Update {
    /// The library, saved queue, and playlists on startup; always the first
    /// update. The playlists are `None` when they couldn't be read, so they
    /// aren't saved over.
    Loaded(Catalog, Queue, Option<SavedPlaylists>),
    /// The library changed on disk or in the folder list.
    Library(Catalog),
    /// The tracks whose files are missing; sent with every `Loaded` and `Library`.
    Missing(Vec<MissingTrack>),
    Folders(Vec<Folder>),
    /// What the scans this run saw but didn't add, by resolved path and in
    /// path order; sent when it changes.
    LeftOut(Arc<Vec<LeftOut>>),
    /// What the thread is doing, or `None` when idle.
    Status(Option<String>),
    Error(String),
}

pub struct Folder {
    pub path: PathBuf,
    pub last_scanned_ns: Option<i64>,
}

pub struct LibraryService {
    jobs: Sender<Job>,
    /// Set with each job the user asked for, so a running scan stops for it
    /// and goes on afterwards.
    interrupt: Arc<AtomicBool>,
}

impl LibraryService {
    /// The library is pruned to `folders` at startup, or kept as is for
    /// `None` ([`Store::startup_folders`](crate::preferences::Store::startup_folders)).
    /// `thumbnail_dir`, if any, is cleared of stale covers after the startup
    /// scan. Every folder is
    /// rescanned `rescan_every` after the last full scan, by the wall clock
    /// `now` (`SystemTime::now` outside tests).
    pub fn start(
        db_path: PathBuf,
        folders: Option<Vec<PathBuf>>,
        thumbnail_dir: Option<PathBuf>,
        rescan_every: Duration,
        now: impl Fn() -> SystemTime + Send + 'static,
        notify: impl Fn(Update) + Send + 'static,
    ) -> Self {
        let (jobs, rx) = mpsc::channel();
        let watcher_jobs = jobs.clone();
        let interrupt = Arc::new(AtomicBool::new(false));
        let worker_interrupt = Arc::clone(&interrupt);
        let spawned = std::thread::Builder::new()
            .name("lune-library".into())
            .spawn(move || {
                let worker = Worker {
                    library: None,
                    db_path,
                    folders: Vec::new(),
                    thumbnail_dir,
                    rescan_every,
                    now: Box::new(now),
                    watcher: None,
                    jobs: watcher_jobs,
                    notify: Box::new(notify),
                    left_out: Arc::default(),
                    interrupt: worker_interrupt,
                    unfinished: None,
                };
                worker.run(folders, rx);
            });
        if let Err(err) = spawned {
            tracing::error!(%err, "could not start the library thread");
        }
        Self { jobs, interrupt }
    }

    pub fn send(&self, job: Job) {
        // The watcher's rescans wait their turn, so changes on disk can't
        // keep a scan from finishing.
        let interrupts = !matches!(job, Job::Rescan(_));
        let _ = self.jobs.send(job);
        // Set after sending: set before, the worker could clear it while
        // this job isn't in the channel yet, and the scan wouldn't stop.
        if interrupts {
            self.interrupt.store(true, Ordering::Relaxed);
        }
    }
}

struct Worker {
    library: Option<Library>,
    db_path: PathBuf,
    folders: Vec<PathBuf>,
    thumbnail_dir: Option<PathBuf>,
    rescan_every: Duration,
    now: Box<dyn Fn() -> SystemTime + Send>,
    watcher: Option<FolderWatcher>,
    jobs: Sender<Job>,
    notify: Box<dyn Fn(Update) + Send>,
    /// What the scans saw but didn't add, as [`Update::LeftOut`] sends it.
    left_out: Arc<Vec<LeftOut>>,
    interrupt: Arc<AtomicBool>,
    /// What a scan stopped for a job left to do, resumed once jobs are done.
    unfinished: Option<(Vec<PathBuf>, ScanMode)>,
}

impl Worker {
    fn run(mut self, folders: Option<Vec<PathBuf>>, jobs: Receiver<Job>) {
        self.startup(folders);
        let mut rescan_due = (self.now)() + self.rescan_every;
        loop {
            let Some(wait) = rescan_wait(rescan_due, (self.now)()) else {
                rescan_due = (self.now)() + self.rescan_every;
                let folders = self.folders.clone();
                if self.scan(folders, ScanMode::Quick) {
                    self.publish_library();
                }
                self.status_idle();
                continue;
            };
            let wait = if self.unfinished.is_some() {
                Duration::ZERO
            } else {
                wait
            };
            let mut batch = match jobs.recv_timeout(wait) {
                Ok(job) => vec![job],
                Err(RecvTimeoutError::Timeout) if self.unfinished.is_some() => Vec::new(),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            // Cleared before the jobs run, so a job sent meanwhile stops
            // their scans in turn.
            self.interrupt.store(false, Ordering::Relaxed);
            batch.extend(jobs.try_iter());
            let mut changed = false;
            let mut forgot = false;
            let mut rescan: Vec<PathBuf> = Vec::new();
            for job in batch {
                match job {
                    Job::AddFolder(folder) => changed |= self.add_folder(folder),
                    Job::RemoveFolder(folder) => changed |= self.remove_folder(&folder),
                    Job::RescanAll => {
                        rescan.extend(self.folders.clone());
                        rescan_due = (self.now)() + self.rescan_every;
                    }
                    Job::Rescan(dirs) => rescan.extend(dirs),
                    Job::ForgetMissing(ids) => forgot |= self.forget_missing(Some(&ids)),
                    Job::ForgetAllMissing => forgot |= self.forget_missing(None),
                }
            }
            // Both are thorough: a watched change may be a file rewritten in
            // place, which leaves its directory's mtime alone.
            if let Some((mut dirs, mode)) = self.unfinished.take() {
                dirs.retain(|dir| !rescan.contains(dir));
                changed |= self.scan(rescan, ScanMode::Thorough);
                changed |= self.scan(dirs, mode);
            } else {
                changed |= self.scan(rescan, ScanMode::Thorough);
            }
            if changed {
                self.publish_library();
            } else if forgot {
                self.publish_missing();
            }
            self.status_idle();
        }
    }

    /// Says the thread is idle, unless a stopped scan is still to go on.
    fn status_idle(&self) {
        if self.unfinished.is_none() {
            (self.notify)(Update::Status(None));
        }
    }

    fn startup(&mut self, folders: Option<Vec<PathBuf>>) {
        let opened = Library::open(&self.db_path).and_then(|library| {
            let catalog = Catalog::new(library.track_list()?);
            let queue = library.load_queue()?;
            let playlists = SavedPlaylists {
                playlists: library.playlists()?,
                unused_id: library.unused_list_id()?,
            };
            Ok((library, catalog, queue, playlists))
        });
        match opened {
            Ok((library, catalog, queue, playlists)) => {
                tracing::info!(
                    tracks = catalog.len(),
                    queued = queue.len(),
                    playlists = playlists.playlists.len(),
                    "library loaded"
                );
                self.library = Some(library);
                (self.notify)(Update::Loaded(catalog, queue, Some(playlists)));
                self.publish_missing();
            }
            Err(err) => {
                self.error(format!(
                    "Could not open the library at {}: {err}. To start a new library, quit Lune and move that file away.",
                    self.db_path.display()
                ));
                (self.notify)(Update::Loaded(
                    Catalog::new(Vec::new()),
                    Queue::default(),
                    None,
                ));
            }
        }

        let mut changed = false;
        if let Some(folders) = folders {
            self.folders = folders;
            changed |= self.prune();
        }
        self.publish_folders();
        if self.library.is_none() {
            (self.notify)(Update::Status(None));
            return;
        }
        self.watch();
        // Thorough: files retagged in place while the app was closed leave
        // their directories' mtimes alone.
        changed |= self.scan(self.folders.clone(), ScanMode::Thorough);
        if changed {
            self.publish_library();
        }
        self.prune_thumbnails();
        self.status_idle();
    }

    fn prune_thumbnails(&self) {
        let (Some(library), Some(dir)) = (&self.library, &self.thumbnail_dir) else {
            return;
        };
        match library.prune_thumbnails(dir) {
            Ok(0) => {}
            Ok(removed) => tracing::info!(
                removed,
                "deleted thumbnails of covers no longer in the library"
            ),
            Err(err) => tracing::warn!(%err, "could not prune the thumbnail cache"),
        }
    }

    fn add_folder(&mut self, folder: PathBuf) -> bool {
        if !lune_core::add_folder(&mut self.folders, folder.clone()) {
            return false;
        }
        self.publish_folders();
        self.watch();
        // A folder that replaced folders inside it rescans them too; that's
        // incremental, so it's cheap.
        self.scan(vec![folder], ScanMode::Quick)
    }

    fn remove_folder(&mut self, folder: &Path) -> bool {
        if !lune_core::remove_folder(&mut self.folders, folder) {
            return false;
        }
        // By resolved path, as a folder that is a link can lead into another
        // folder, whose files stay listed.
        let roots: Vec<PathBuf> = self.folders.iter().map(|f| resolved(f)).collect();
        let kept: Vec<LeftOut> = self
            .left_out
            .iter()
            .filter(|file| roots.iter().any(|root| file.path.starts_with(root)))
            .cloned()
            .collect();
        if kept.len() != self.left_out.len() {
            self.left_out = Arc::new(kept);
            self.publish_left_out();
        }
        self.publish_folders();
        self.watch();
        self.prune()
    }

    /// Deletes the missing tracks among `ids`, or all for `None`. Returns
    /// whether any were.
    fn forget_missing(&mut self, ids: Option<&[i64]>) -> bool {
        let Some(library) = &mut self.library else {
            return false;
        };
        let forgot = match ids {
            Some(ids) => library.forget_missing(ids),
            None => library.forget_all_missing(),
        };
        match forgot {
            Ok(n) => n > 0,
            Err(err) => {
                self.error(format!("Could not remove the unavailable tracks: {err}"));
                false
            }
        }
    }

    fn publish_folders(&self) {
        let folders = &self.folders;
        let scans = match &self.library {
            Some(library) => library.folder_scans(folders).unwrap_or_else(|err| {
                tracing::warn!(%err, "could not read when the folders were last scanned");
                vec![None; folders.len()]
            }),
            None => vec![None; folders.len()],
        };
        let folders = folders
            .iter()
            .zip(scans)
            .map(|(path, last_scanned_ns)| Folder {
                path: path.clone(),
                last_scanned_ns,
            })
            .collect();
        (self.notify)(Update::Folders(folders));
    }

    fn publish_left_out(&self) {
        (self.notify)(Update::LeftOut(Arc::clone(&self.left_out)));
    }

    fn prune(&mut self) -> bool {
        let Some(library) = &mut self.library else {
            return false;
        };
        match library.prune(&self.folders) {
            Ok(removed) => removed.is_some_and(|n| n > 0),
            Err(err) => {
                self.error(format!("Could not update the library: {err}"));
                false
            }
        }
    }

    fn watch(&mut self) {
        // Replace, don't stack: drop the old watcher first.
        self.watcher = None;
        if self.folders.is_empty() {
            return;
        }
        // Watching walks every directory, which takes a while on a network
        // share, so say so before the scan's own status.
        (self.notify)(Update::Status(Some(
            "Setting up folder watching".to_owned(),
        )));
        let jobs = self.jobs.clone();
        match FolderWatcher::start(&self.folders, WATCH_DELAY, move |dirs| {
            let _ = jobs.send(Job::Rescan(dirs));
        }) {
            Ok((watcher, failed)) => {
                for (folder, err) in failed {
                    tracing::warn!(%err, folder = %folder.display(), "not watching folder");
                }
                self.watcher = Some(watcher);
            }
            Err(err) => tracing::warn!(%err, "not watching the library folders"),
        }
    }

    /// Scans `dirs` (each limited to a configured folder). Returns whether
    /// the library changed. Stopped for a job, it leaves the rest to
    /// `unfinished`.
    fn scan(&mut self, dirs: Vec<PathBuf>, mode: ScanMode) -> bool {
        let dirs = lune_core::collapse_nested(
            dirs.into_iter()
                .filter_map(|dir| existing_ancestor(&dir, &self.folders))
                .collect(),
        );
        let mut changed = false;
        let mut recorded = false;
        let mut left_out_changed = false;
        let mut dirs = dirs.into_iter();
        while let Some(dir) = dirs.next() {
            let Some(library) = &mut self.library else {
                return false;
            };
            let name = dir.display().to_string();
            let notify = &self.notify;
            let interrupt = &self.interrupt;
            notify(Update::Status(Some(format!(
                "Scanning {name}: listing files"
            ))));
            let mut last_sent: Option<(Instant, Discriminant<ScanProgress>)> = None;
            let stop = || interrupt.load(Ordering::Relaxed);
            let scanned = library.scan_until(
                &dir,
                mode,
                |progress| {
                    let now = Instant::now();
                    let phase = mem::discriminant(&progress);
                    // A new phase is sent at once, so the total to read shows up.
                    if last_sent.is_some_and(|(sent, last_phase)| {
                        last_phase == phase && now.duration_since(sent) < PROGRESS_EVERY
                    }) {
                        return;
                    }
                    last_sent = Some((now, phase));
                    notify(Update::Status(Some(progress_status(&name, progress))));
                },
                stop,
            );
            match scanned {
                Ok(report) if report.stopped => {
                    changed |= library_changed(&report);
                    // Directories it finished are skipped by a quick scan
                    // resuming it, so their links and names count from here.
                    if report.listed {
                        left_out_changed |= merge_left_out(&mut self.left_out, report);
                    }
                    let rest = std::iter::once(dir).chain(dirs.by_ref());
                    let unfinished = self.unfinished.get_or_insert((Vec::new(), mode));
                    unfinished.0.extend(rest);
                    if mode == ScanMode::Thorough {
                        unfinished.1 = ScanMode::Thorough;
                    }
                    break;
                }
                Ok(report) => {
                    changed |= library_changed(&report);
                    left_out_changed |= merge_left_out(&mut self.left_out, report);
                    if self.folders.contains(&dir) {
                        match library.record_folder_scan(&dir, (self.now)()) {
                            Ok(()) => recorded = true,
                            Err(err) => {
                                tracing::warn!(%err, dir = %dir.display(), "could not record the scan")
                            }
                        }
                    }
                }
                // A folder that is gone (e.g. an unmounted drive) keeps its
                // tracks until it is removed from the list.
                Err(err) => tracing::warn!(%err, dir = %dir.display(), "could not scan"),
            }
        }
        if recorded {
            self.publish_folders();
        }
        if left_out_changed {
            self.publish_left_out();
        }
        changed
    }

    fn publish_library(&mut self) {
        let Some(library) = &self.library else {
            return;
        };
        match library.track_list() {
            Ok(tracks) => (self.notify)(Update::Library(Catalog::new(tracks))),
            Err(err) => self.error(format!("Could not read the library: {err}")),
        }
        self.publish_missing();
    }

    fn publish_missing(&self) {
        let Some(library) = &self.library else {
            return;
        };
        match library.missing_tracks() {
            Ok(tracks) => (self.notify)(Update::Missing(tracks)),
            Err(err) => self.error(format!("Could not read the unavailable tracks: {err}")),
        }
    }

    fn error(&self, message: String) {
        tracing::error!("{message}");
        (self.notify)(Update::Error(message));
    }
}

/// The nearest existing directory at or above `dir` that is still inside one
/// of `folders`, so a deleted folder's tracks are removed by scanning its
/// parent. `None` when `dir` is in none of them.
pub fn existing_ancestor(dir: &Path, folders: &[PathBuf]) -> Option<PathBuf> {
    let canonical: Vec<PathBuf> = folders
        .iter()
        .map(|f| std::fs::canonicalize(f).unwrap_or_else(|_| f.clone()))
        .collect();
    let root = folders
        .iter()
        .chain(&canonical)
        .find(|root| dir.starts_with(root))?;
    dir.ancestors()
        .take_while(|a| a.starts_with(root))
        .find(|a| a.is_dir())
        .map(Path::to_path_buf)
}

/// `folder` with links resolved, as scans report paths, or as it is if it
/// can't be (e.g. an unmounted drive).
fn resolved(folder: &Path) -> PathBuf {
    std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf())
}

/// Replaces what `left_out` has under the report's root with what the scan
/// found, keeping it in path order. Returns whether that changed it; if not,
/// it is left as it was, still shared.
///
/// Links, non-UTF-8 names, and anything under a non-UTF-8 directory stand if
/// a quick scan didn't reach them. It reaches the subdirectories of one it
/// skips by their rows, and only a directory whose path is valid UTF-8 has
/// one, so they stand if the nearest such directory above them was skipped.
/// Other files are always reached (see [`ScanReport::dirs_skipped`]).
fn merge_left_out(left_out: &mut Arc<Vec<LeftOut>>, report: ScanReport) -> bool {
    let stands = |file: &LeftOut| {
        if !file.path.starts_with(&report.root) {
            return true;
        }
        let skippable = matches!(
            file.reason,
            LeftOutReason::Link | LeftOutReason::NonUtf8Name
        ) || file.path.to_str().is_none();
        skippable
            && file
                .path
                .ancestors()
                .skip(1)
                .find(|dir| dir.to_str().is_some())
                .is_some_and(|dir| report.dirs_skipped.contains(dir))
    };
    let mut merged: Vec<LeftOut> = left_out.iter().filter(|f| stands(f)).cloned().collect();
    merged.extend(report.left_out);
    merged.sort_by(|a, b| a.path.cmp(&b.path));
    let changed = merged != **left_out;
    if changed {
        *left_out = Arc::new(merged);
    }
    changed
}

/// What `files` are, counted by why they weren't added, if any.
pub fn left_out_summary(files: &[LeftOut]) -> Option<String> {
    let counts = LeftOutCounts::of(files);
    let parts: Vec<String> = [
        (
            counts.tags,
            "file whose tags couldn't be read",
            "files whose tags couldn't be read",
        ),
        (
            counts.links,
            "linked file (links aren't followed)",
            "linked files (links aren't followed)",
        ),
        (
            counts.non_utf8_names,
            "file whose name isn't valid UTF-8",
            "files whose names aren't valid UTF-8",
        ),
        (
            counts.unreadable,
            "file or folder that couldn't be opened",
            "files or folders that couldn't be opened",
        ),
    ]
    .into_iter()
    .filter(|&(count, ..)| count > 0)
    .map(|(count, one, many)| format!("{count} {}", if count == 1 { one } else { many }))
    .collect();
    let list = match parts.as_slice() {
        [] => return None,
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    };
    Some(format!("Not added: {list}."))
}

fn library_changed(report: &ScanReport) -> bool {
    report.added + report.updated + report.missing + report.restored + report.dated > 0
}

fn progress_status(folder: &str, progress: ScanProgress) -> String {
    match progress {
        ScanProgress::Listing { files } => {
            format!("Scanning {folder}: listing files, {files} found")
        }
        ScanProgress::Reading { done, total } => {
            format!("Scanning {folder}: reading tags, {done} of {total}")
        }
    }
}

/// How long to wait for a rescan due at `due`, or `None` if it is due now.
fn rescan_wait(due: SystemTime, now: SystemTime) -> Option<Duration> {
    match due.duration_since(now) {
        Ok(left) if !left.is_zero() => Some(left.min(MAX_WAIT)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/library_service_test.rs"]
mod tests;
