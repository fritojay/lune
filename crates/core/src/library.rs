use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::Connection;

use crate::art;
use crate::lists::{self, Playlist};
use crate::query::{self, AlbumRow, LibraryStats, ListTrack, MissingTrack};
use crate::queue::{self, Queue};
use crate::scan::{self, ScanMode, ScanProgress, ScanReport};
use crate::{Result, db};

pub struct Library {
    conn: Connection,
}

impl Library {
    /// Opens (creating and migrating if needed) the library database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            conn: db::open(path)?,
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Self {
            conn: db::open_in_memory()?,
        })
    }

    /// Scans `root` and updates the library to match it. Only files under
    /// `root` are added, updated, or marked missing. Music files are opened
    /// read-only. `progress` is called often (per directory listed and per
    /// file read), on this thread.
    pub fn scan(
        &mut self,
        root: &Path,
        mode: ScanMode,
        mut progress: impl FnMut(ScanProgress),
    ) -> Result<ScanReport> {
        scan::scan(&mut self.conn, root, mode, &mut progress)
    }

    /// Removes tracks under none of `folders` (see [`crate::Settings`]).
    /// Returns how many, or `None` when a folder is unavailable and nothing
    /// was removed.
    pub fn prune(&mut self, folders: &[PathBuf]) -> Result<Option<usize>> {
        scan::prune(&mut self.conn, folders)
    }

    /// `folder` is the path as configured, not canonicalized; one that isn't
    /// UTF-8 is not recorded. [`Library::prune`] forgets removed folders.
    pub fn record_folder_scan(&self, folder: &Path, at: SystemTime) -> Result<()> {
        scan::record_folder_scan(&self.conn, folder, at)
    }

    /// Unix nanoseconds; `None` if never scanned.
    pub fn folder_scans(&self, folders: &[PathBuf]) -> Result<Vec<Option<i64>>> {
        scan::folder_scans(&self.conn, folders)
    }

    /// Deletes the missing tracks among `ids`; the others are left alone.
    /// Returns how many were deleted. A file that comes back is added anew.
    pub fn forget_missing(&mut self, ids: &[i64]) -> Result<usize> {
        scan::forget_missing(&mut self.conn, Some(ids))
    }

    /// Deletes every missing track. Returns how many.
    pub fn forget_all_missing(&mut self) -> Result<usize> {
        scan::forget_missing(&mut self.conn, None)
    }

    /// Fills an empty library with `tracks` fake rows (no media files) for
    /// UI benchmarks. The paths are relative, so no scan root ever covers them.
    #[cfg(feature = "synthetic")]
    pub fn generate_synthetic(&mut self, tracks: usize) -> Result<()> {
        crate::synthetic::generate(&mut self.conn, tracks)
    }

    pub fn stats(&self) -> Result<LibraryStats> {
        query::stats(&self.conn)
    }

    pub fn albums(&self) -> Result<Vec<AlbumRow>> {
        query::albums(&self.conn)
    }

    pub fn album_tracks(&self, album_id: i64) -> Result<Vec<ListTrack>> {
        query::album_tracks(&self.conn, album_id)
    }

    pub fn track_list(&self) -> Result<Vec<ListTrack>> {
        query::track_list(&self.conn)
    }

    pub fn missing_tracks(&self) -> Result<Vec<MissingTrack>> {
        query::missing_tracks(&self.conn)
    }

    pub fn search(&self, text: &str, limit: u32) -> Result<Vec<ListTrack>> {
        query::search(&self.conn, text, limit)
    }

    /// The cached thumbnail of cover `art_id` (at most `size` pixels on its
    /// longer side) in `dir`, made now if needed. Slow: call off the UI thread.
    pub fn thumbnail(&self, art_id: i64, dir: &Path, size: u32) -> Result<Option<PathBuf>> {
        art::thumbnail(&self.conn, art_id, dir, size)
    }

    /// Deletes cached thumbnails in `dir` whose cover is no longer in the
    /// library. Returns how many.
    pub fn prune_thumbnails(&self, dir: &Path) -> Result<usize> {
        art::prune_thumbnails(&self.conn, dir)
    }

    pub fn load_queue(&self) -> Result<Queue> {
        queue::load(&self.conn)
    }

    pub fn playlists(&self) -> Result<Vec<Playlist>> {
        lists::load_playlists(&self.conn)
    }

    pub fn save_queue(&mut self, queue: &Queue) -> Result<()> {
        queue::save(&mut self.conn, queue)
    }

    pub fn save_queue_cursor(&self, queue: &Queue) -> Result<()> {
        queue::save_cursor(&self.conn, queue)
    }

    /// Records that playback of track `track_id` started at `at`, for Recently
    /// Played. An unknown id (a track removed since it was queued) is ignored.
    pub fn record_play(&self, track_id: i64, at: SystemTime) -> Result<()> {
        query::record_play(&self.conn, track_id, at)
    }

    pub fn search_ids(&self, text: &str) -> Result<HashSet<i64>> {
        query::search_ids(&self.conn, text)
    }
}
