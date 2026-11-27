//! The real queue lives in `core`/`app`; the engine only needs
//! [`PlayQueue`]. Whenever the queue is edited, call
//! [`Player::queue_changed`](crate::Player::queue_changed) so a pre-buffered
//! next track that is no longer next gets dropped.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

/// Identifies one slot in the queue. The same track can appear in the queue
/// more than once, each time with its own `EntryId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct QueueEntry {
    pub id: EntryId,
    pub path: PathBuf,
    /// Linear per-track gain. 1.0 (unity) until ReplayGain is added.
    pub gain: f32,
}

impl QueueEntry {
    pub fn new(id: EntryId, path: PathBuf) -> Self {
        Self {
            id,
            path,
            gain: 1.0,
        }
    }
}

pub trait PlayQueue: Send + Sync {
    /// The entry that plays when `id` ends (`id` itself to repeat it), or
    /// `None` at the end of the queue (or if `id` is no longer in the queue).
    fn next_after(&self, id: EntryId) -> Option<QueueEntry>;
    fn previous_before(&self, id: EntryId) -> Option<QueueEntry>;

    /// The entry skipping forward from `id` goes to, which is never `id`
    /// repeated.
    fn skip_after(&self, id: EntryId) -> Option<QueueEntry> {
        self.next_after(id)
    }
}

#[derive(Debug, Default)]
pub struct VecQueue {
    entries: Mutex<Vec<QueueEntry>>,
}

impl VecQueue {
    pub fn new(entries: Vec<QueueEntry>) -> Self {
        Self {
            entries: Mutex::new(entries),
        }
    }

    pub fn from_paths(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self::new(
            paths
                .into_iter()
                .zip(0u64..)
                .map(|(path, i)| QueueEntry::new(EntryId(i), path))
                .collect(),
        )
    }

    pub fn first(&self) -> Option<QueueEntry> {
        self.lock().first().cloned()
    }

    /// Call `Player::queue_changed` afterwards.
    #[cfg(test)]
    pub fn set(&self, entries: Vec<QueueEntry>) {
        *self.lock() = entries;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<QueueEntry>> {
        // The data is a plain Vec; a panic elsewhere can't leave it inconsistent.
        self.entries.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn position(entries: &[QueueEntry], id: EntryId) -> Option<usize> {
        entries.iter().position(|e| e.id == id)
    }
}

impl PlayQueue for VecQueue {
    fn next_after(&self, id: EntryId) -> Option<QueueEntry> {
        let entries = self.lock();
        let i = Self::position(&entries, id)?;
        entries.get(i + 1).cloned()
    }

    fn previous_before(&self, id: EntryId) -> Option<QueueEntry> {
        let entries = self.lock();
        let i = Self::position(&entries, id)?;
        i.checked_sub(1).and_then(|i| entries.get(i)).cloned()
    }
}
