//! Watches the library folders and reports which directories to rescan.
//! Rescanning a directory ([`crate::Library::scan`]) is incremental and only
//! touches files under it, so the watcher only has to narrow changes down to
//! directories, not interpret every event.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use notify::event::ModifyKind;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::tags::is_audio_path;
use crate::{CoreError, Result};

/// Stops watching when dropped.
pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
}

impl FolderWatcher {
    /// Watches `folders` recursively. After changes settle for `delay`,
    /// `on_change` gets the directories to rescan (none inside another).
    /// Folders that can't be watched are returned with the error; the rest
    /// are still watched.
    pub fn start(
        folders: &[PathBuf],
        delay: Duration,
        on_change: impl FnMut(Vec<PathBuf>) + Send + 'static,
    ) -> Result<(Self, Vec<(PathBuf, CoreError)>)> {
        // Event paths are under the path watched; scanned tracks are stored
        // under the canonical root, so watch that.
        let mut roots = Vec::new();
        let mut failed = Vec::new();
        for folder in folders {
            match std::fs::canonicalize(folder) {
                Ok(root) => roots.push(root),
                Err(source) => failed.push((
                    folder.clone(),
                    CoreError::Io {
                        path: folder.clone(),
                        source,
                    },
                )),
            }
        }
        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |event| {
                let _ = tx.send(event);
            },
            // Like the scanner, which skips links inside the folders.
            Config::default().with_follow_symlinks(false),
        )
        .map_err(|err| CoreError::Watch(err.to_string()))?;
        for root in &roots {
            if let Err(err) = watcher.watch(root, RecursiveMode::Recursive) {
                // On Linux this is usually the inotify watch limit.
                failed.push((root.clone(), CoreError::Watch(err.to_string())));
            }
        }
        // Ends when the watcher is dropped, which drops the sender.
        std::thread::Builder::new()
            .name("lune-watch".into())
            .spawn(move || debounce(&rx, &roots, delay, on_change))
            .map_err(|err| CoreError::Watch(err.to_string()))?;
        Ok((Self { _watcher: watcher }, failed))
    }
}

fn debounce(
    events: &Receiver<notify::Result<Event>>,
    roots: &[PathBuf],
    delay: Duration,
    mut on_change: impl FnMut(Vec<PathBuf>),
) {
    let mut pending = Pending::default();
    loop {
        let received = if pending.is_empty() {
            events.recv().map_err(|_| RecvTimeoutError::Disconnected)
        } else {
            events.recv_timeout(delay)
        };
        match received {
            Ok(event) => pending.add(event),
            Err(RecvTimeoutError::Timeout) => {
                let dirs = pending.take(roots);
                if !dirs.is_empty() {
                    on_change(dirs);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Changes seen since the last report. Every change is kept: a debouncer
/// that cancels a create followed by a remove loses deletions on macOS,
/// where FSEvents repeats a recent create in the remove event.
#[derive(Default)]
pub(crate) struct Pending {
    paths: HashSet<PathBuf>,
    rescan_all: bool,
}

impl Pending {
    pub(crate) fn add(&mut self, event: notify::Result<Event>) {
        match event {
            // The OS dropped events: only a full rescan is safe.
            Ok(event) if event.need_rescan() => self.rescan_all = true,
            Ok(event) if changes_files(&event.kind) => {
                let folder_touch = touches_in_place(&event.kind);
                self.paths.extend(
                    event
                        .paths
                        .into_iter()
                        .filter(|p| !(folder_touch && p.is_dir())),
                );
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(%err, "file watching error; rescanning everything");
                self.rescan_all = true;
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        !self.rescan_all && self.paths.is_empty()
    }

    pub(crate) fn take(&mut self, roots: &[PathBuf]) -> Vec<PathBuf> {
        let pending = std::mem::take(self);
        if pending.rescan_all {
            roots.to_vec()
        } else {
            dirs_to_rescan(roots, &pending.paths)
        }
    }
}

/// Whether an event can change what's in the library. Reads are ignored:
/// the scanner opens every file it reads, so reacting to them would rescan
/// forever. So are metadata-only changes (permissions, access times).
pub(crate) fn changes_files(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(_) => false,
        EventKind::Modify(ModifyKind::Metadata(_)) => false,
        EventKind::Any
        | EventKind::Create(_)
        | EventKind::Modify(_)
        | EventKind::Remove(_)
        | EventKind::Other => true,
    }
}

/// A change to an entry in place, not to its name. On a folder that is only
/// its own times: Windows reports one when the folder is listed or a file in
/// it is read, so a scan would set off another forever. Changes to what the
/// folder holds are reported on the entries themselves.
fn touches_in_place(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Other)
    )
}

/// The directories to rescan for changes at `paths`: each change's parent
/// directory (which covers files and folders created, changed, removed, or
/// renamed), kept within its root, with nested directories collapsed.
/// Changes to other files that still exist (covers, playlists, ...) are ignored.
pub(crate) fn dirs_to_rescan<'a>(
    roots: &[PathBuf],
    paths: impl IntoIterator<Item = &'a PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for path in paths {
        if path.is_file() && !is_audio_path(path) {
            continue;
        }
        let Some(root) = roots.iter().find(|root| path.starts_with(root)) else {
            continue;
        };
        let dir = match path.parent() {
            Some(parent) if parent.starts_with(root) => parent,
            _ => root.as_path(),
        };
        dirs.push(dir.to_path_buf());
    }
    collapse_nested(dirs)
}

/// `dirs` sorted, without the ones inside another (or repeated), so scanning
/// what's left covers them all once.
pub fn collapse_nested(mut dirs: Vec<PathBuf>) -> Vec<PathBuf> {
    // Sorted, a directory comes before everything inside it.
    dirs.sort();
    let mut collapsed: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !collapsed.last().is_some_and(|kept| dir.starts_with(kept)) {
            collapsed.push(dir);
        }
    }
    collapsed
}

#[cfg(test)]
#[path = "tests/watch_test.rs"]
mod tests;
