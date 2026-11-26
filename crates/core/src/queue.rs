//! The play queue: a snapshot of tracks, separate from every view, so filter
//! changes and rescans never alter it.

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, named_params};

use crate::Result;
use crate::lists::{self, ListId};

/// One slot in the queue. The same track can be queued more than once, each
/// time in its own slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlotId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRef {
    pub id: i64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    pub slot: SlotId,
    pub track_id: i64,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Queue {
    /// In play order (shuffled while shuffle is on).
    items: Vec<QueueItem>,
    /// While shuffle is on: every slot in the order it had before shuffling,
    /// with later additions placed as if shuffle were off.
    original: Option<Vec<SlotId>>,
    current: Option<SlotId>,
    /// Index of `current` in `items`. When the current item is removed, it
    /// stays where the item was, so playback continues with whatever took
    /// its place.
    cursor: usize,
    /// How far into this slot playback had got, to continue from there.
    resume: Option<(SlotId, Duration)>,
    next_slot: u64,
    rng: u64,
}

impl Default for Queue {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            original: None,
            current: None,
            cursor: 0,
            resume: None,
            next_slot: 0,
            // Odd and nonzero, as xorshift needs.
            rng: RandomState::new().hash_one(0u8) | 1,
        }
    }
}

impl Queue {
    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn shuffle(&self) -> bool {
        self.original.is_some()
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.current_index().map(|i| &self.items[i])
    }

    /// The current item's index, if it is still in the queue.
    pub fn current_index(&self) -> Option<usize> {
        let current = self.current?;
        match self.items.get(self.cursor) {
            Some(item) if item.slot == current => Some(self.cursor),
            _ => None,
        }
    }

    /// Marks `slot` as the one playing (the engine reports it), from its start.
    pub fn set_current(&mut self, slot: SlotId) {
        if let Some(i) = self.index_of(slot) {
            self.current = Some(slot);
            self.cursor = i;
            self.resume = None;
        }
    }

    /// Where to start the current item when playback starts again.
    pub fn resume_position(&self) -> Duration {
        match (self.current(), self.resume) {
            (Some(item), Some((slot, position))) if item.slot == slot => position,
            _ => Duration::ZERO,
        }
    }

    pub fn set_resume_position(&mut self, slot: SlotId, position: Duration) {
        self.resume = Some((slot, position));
    }

    /// Replaces the whole queue with `tracks` and makes `tracks[start]`
    /// current. With shuffle on, the new queue is shuffled with that track
    /// first. Returns the item to play.
    pub fn replace(
        &mut self,
        tracks: impl IntoIterator<Item = TrackRef>,
        start: usize,
    ) -> Option<&QueueItem> {
        self.items = self.new_items(tracks);
        let start = self.items.get(start).map(|item| item.slot);
        self.current = start;
        self.cursor = 0;
        if let Some(slot) = start {
            self.cursor = self.index_of(slot).unwrap_or(0);
        }
        if self.shuffle() {
            self.original = Some(self.slots());
            self.shuffle_around_current();
        }
        self.current()
    }

    /// Inserts `tracks` right after the current item (at the front when
    /// nothing is current).
    pub fn play_next(&mut self, tracks: impl IntoIterator<Item = TrackRef>) {
        let new = self.new_items(tracks);
        let at = self.insertion_point();
        if let Some(original) = &mut self.original {
            let at = self
                .current
                .and_then(|c| original.iter().position(|&s| s == c))
                .map_or(0, |i| i + 1);
            original.splice(at..at, new.iter().map(|item| item.slot));
        }
        self.items.splice(at..at, new);
    }

    pub fn add(&mut self, tracks: impl IntoIterator<Item = TrackRef>) {
        let new = self.new_items(tracks);
        if let Some(original) = &mut self.original {
            original.extend(new.iter().map(|item| item.slot));
        }
        self.items.extend(new);
    }

    pub fn remove(&mut self, slots: &[SlotId]) {
        let remove: HashSet<SlotId> = slots.iter().copied().collect();
        let before_cursor = self.items[..self.cursor.min(self.items.len())]
            .iter()
            .filter(|item| remove.contains(&item.slot))
            .count();
        self.cursor -= before_cursor;
        self.items.retain(|item| !remove.contains(&item.slot));
        if let Some(original) = &mut self.original {
            original.retain(|slot| !remove.contains(slot));
        }
    }

    /// Moves `slots` (kept in their queue order) so they sit before the item
    /// that was at index `to`, or at the end when `to` is past the end.
    pub fn move_to(&mut self, slots: &[SlotId], to: usize) {
        let moving: HashSet<SlotId> = slots.iter().copied().collect();
        let to = to.min(self.items.len());
        let before = self.items[..to]
            .iter()
            .filter(|item| moving.contains(&item.slot))
            .count();
        let (moved, mut rest): (Vec<QueueItem>, Vec<QueueItem>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|item| moving.contains(&item.slot));
        let at = to - before;
        rest.splice(at..at, moved);
        self.items = rest;
        self.relocate_cursor();
    }

    pub fn clear(&mut self) {
        self.items.clear();
        if let Some(original) = &mut self.original {
            original.clear();
        }
        self.current = None;
        self.cursor = 0;
    }

    /// Turning shuffle on puts the current item first and shuffles the rest.
    /// Turning it off restores the order from before, including later edits.
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle() {
            return;
        }
        if on {
            self.original = Some(self.slots());
            self.shuffle_around_current();
        } else if let Some(original) = self.original.take() {
            let mut by_slot: std::collections::HashMap<SlotId, QueueItem> =
                self.items.drain(..).map(|item| (item.slot, item)).collect();
            self.items = original
                .iter()
                .filter_map(|slot| by_slot.remove(slot))
                .collect();
            self.relocate_cursor();
        }
    }

    /// The item after `slot`. If `slot` was the current item and has been
    /// removed, this is the item that took its place.
    pub fn next_after(&self, slot: SlotId) -> Option<&QueueItem> {
        match self.index_of(slot) {
            Some(i) => self.items.get(i + 1),
            None if self.current == Some(slot) => self.items.get(self.cursor),
            None => None,
        }
    }

    pub fn previous_before(&self, slot: SlotId) -> Option<&QueueItem> {
        let i = match self.index_of(slot) {
            Some(i) => i,
            None if self.current == Some(slot) => self.cursor,
            None => return None,
        };
        i.checked_sub(1).and_then(|i| self.items.get(i))
    }

    pub fn index_of(&self, slot: SlotId) -> Option<usize> {
        // The cursor is the common case (the engine asks about the playing item).
        if self.items.get(self.cursor).is_some_and(|i| i.slot == slot) {
            return Some(self.cursor);
        }
        self.items.iter().position(|item| item.slot == slot)
    }

    /// Rebuilds a queue loaded from the database, from its unshuffled order.
    /// A shuffled queue is shuffled anew, with the current item first.
    pub(crate) fn restore(
        tracks: Vec<TrackRef>,
        shuffle: bool,
        cursor: Option<usize>,
        resume: Duration,
    ) -> Self {
        let mut queue = Self::default();
        queue.items = queue.new_items(tracks);
        if let Some(item) = cursor.and_then(|c| queue.items.get(c)) {
            queue.current = Some(item.slot);
            queue.cursor = cursor.unwrap_or(0);
            queue.resume = Some((item.slot, resume));
        }
        queue.set_shuffle(shuffle);
        queue
    }

    /// The track ids in the order shuffle off would put them.
    pub(crate) fn unshuffled_track_ids(&self) -> Vec<i64> {
        match &self.original {
            None => self.items.iter().map(|item| item.track_id).collect(),
            Some(original) => {
                let track_id: HashMap<SlotId, i64> = self
                    .items
                    .iter()
                    .map(|item| (item.slot, item.track_id))
                    .collect();
                original
                    .iter()
                    .filter_map(|slot| track_id.get(slot).copied())
                    .collect()
            }
        }
    }

    /// The current item's index in [`Queue::unshuffled_track_ids`].
    pub(crate) fn unshuffled_current_index(&self) -> Option<usize> {
        let i = self.current_index()?;
        match &self.original {
            None => Some(i),
            Some(original) => original.iter().position(|&slot| slot == self.items[i].slot),
        }
    }

    fn new_items(&mut self, tracks: impl IntoIterator<Item = TrackRef>) -> Vec<QueueItem> {
        tracks
            .into_iter()
            .map(|TrackRef { id, path }| {
                let slot = SlotId(self.next_slot);
                self.next_slot += 1;
                QueueItem {
                    slot,
                    track_id: id,
                    path,
                }
            })
            .collect()
    }

    fn slots(&self) -> Vec<SlotId> {
        self.items.iter().map(|item| item.slot).collect()
    }

    fn insertion_point(&self) -> usize {
        match self.current_index() {
            Some(i) => i + 1,
            // The current item was removed: insert where it was.
            None if self.current.is_some() => self.cursor.min(self.items.len()),
            None => 0,
        }
    }

    fn relocate_cursor(&mut self) {
        if let Some(i) = self.current.and_then(|c| self.index_of(c)) {
            self.cursor = i;
        }
    }

    fn shuffle_around_current(&mut self) {
        let first = match self.current_index() {
            Some(i) => {
                self.items.swap(0, i);
                1
            }
            None => 0,
        };
        // Fisher-Yates over everything after the current item.
        for i in (first + 1..self.items.len()).rev() {
            let j = first + (self.next_random() % (i - first + 1) as u64) as usize;
            self.items.swap(i, j);
        }
        self.relocate_cursor();
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64*: plenty for shuffling, and needs no extra crate.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        self.rng.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// Replaces the saved queue with `queue`, in one transaction. The queue is
/// saved unshuffled: a shuffled order lasts only until the app quits.
pub(crate) fn save(conn: &mut Connection, queue: &Queue) -> Result<()> {
    let tx = conn.transaction()?;
    lists::save_items(&tx, ListId::QUEUE, &queue.unshuffled_track_ids())?;
    save_state(&tx, queue)?;
    tx.commit()?;
    Ok(())
}

/// Saves only which item is current, where to resume it, and the shuffle
/// flag: cheap enough to run on every track change.
pub(crate) fn save_cursor(conn: &Connection, queue: &Queue) -> Result<()> {
    save_state(conn, queue)
}

fn save_state(conn: &Connection, queue: &Queue) -> Result<()> {
    conn.execute(
        "INSERT INTO queue_state (id, cursor, shuffle, resume_ms) VALUES (1, :cursor, :shuffle, :resume_ms)
         ON CONFLICT (id) DO UPDATE SET cursor = excluded.cursor, shuffle = excluded.shuffle,
             resume_ms = excluded.resume_ms",
        named_params! {
            ":cursor": queue.unshuffled_current_index().map(|i| i as i64),
            ":shuffle": queue.shuffle(),
            ":resume_ms": i64::try_from(queue.resume_position().as_millis()).unwrap_or(i64::MAX),
        },
    )?;
    Ok(())
}

/// Loads the saved queue. Tracks deleted from the library since are dropped;
/// missing ones stay, as their files may come back (playback skips them).
pub(crate) fn load(conn: &Connection) -> Result<Queue> {
    let (cursor, shuffle, resume_ms): (Option<i64>, bool, i64) = conn
        .query_row(
            "SELECT cursor, shuffle, resume_ms FROM queue_state WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get("cursor")?,
                    row.get("shuffle")?,
                    row.get("resume_ms")?,
                ))
            },
        )
        .optional()?
        .unwrap_or((None, false, 0));
    let mut tracks = Vec::new();
    let mut new_cursor = None;
    for (position, (track_id, path)) in lists::load_items(conn, ListId::QUEUE)?
        .into_iter()
        .enumerate()
    {
        let Some(path) = path else { continue };
        if Some(position as i64) == cursor {
            new_cursor = Some(tracks.len());
        }
        tracks.push(TrackRef { id: track_id, path });
    }
    let resume = Duration::from_millis(u64::try_from(resume_ms).unwrap_or(0));
    Ok(Queue::restore(tracks, shuffle, new_cursor, resume))
}

#[cfg(test)]
#[path = "tests/queue_test.rs"]
mod tests;
