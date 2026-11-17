use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::queue::QueueEntry;

#[derive(Debug, Clone)]
pub(crate) struct Boundary {
    pub(crate) start: u64,
    pub(crate) entry: QueueEntry,
    /// Track position at `start` (non-zero after a seek).
    pub(crate) offset: Duration,
    pub(crate) duration: Option<Duration>,
}

#[derive(Debug)]
pub(crate) struct Timeline {
    pub(crate) boundaries: VecDeque<Boundary>,
    /// Timeline frame where playback ends (all audio decoded), if it does.
    /// `None` with no boundaries means a track was requested but has not
    /// produced audio yet.
    pub(crate) end: Option<u64>,
}

impl Timeline {
    /// Nothing requested yet: stopped.
    pub(crate) fn new() -> Self {
        Self {
            boundaries: VecDeque::new(),
            end: Some(0),
        }
    }

    pub(crate) fn playing(&self, consumed: u64) -> Option<usize> {
        if self.boundaries.is_empty() {
            return None;
        }
        Some(
            self.boundaries
                .iter()
                .rposition(|b| b.start <= consumed)
                .unwrap_or(0),
        )
    }
}

pub(crate) fn lock(timeline: &Mutex<Timeline>) -> MutexGuard<'_, Timeline> {
    // Timeline updates are single assignments; a panicking holder can't leave
    // it half-written in a way that matters, so recover from poisoning.
    timeline.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "tests/timeline_test.rs"]
mod tests;
