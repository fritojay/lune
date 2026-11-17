//! REVIEW RULE: everything reachable from [`Renderer::render`] runs on the
//! realtime audio thread. It must not allocate, lock, block, do I/O, or log.
//! It only reads from lock-free SPSC rings (rtrb) and touches atomics.
//!
//! Frames are counted on one timeline shared with the decoder thread: frame
//! `n` is the `n`-th frame ever pushed into the ring. The decoder records
//! where each track starts on that timeline; the renderer counts how many
//! frames it has consumed. Together these give the playing track and its
//! position from frames actually handed to the device, not frames decoded.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use rtrb::Consumer;

#[derive(Debug)]
pub struct Shared {
    /// Frames consumed from the ring (played or discarded). Written only by
    /// the callback.
    pub consumed: AtomicU64,
    /// When set, the callback outputs silence and consumes nothing.
    pub paused: AtomicBool,
    /// Output volume, as `f32` bits.
    pub volume: AtomicU32,
    /// Set by the decoder while more audio is coming, so an empty ring
    /// counts as an underrun rather than the end of playback.
    pub expecting_data: AtomicBool,
    pub underruns: AtomicU64,
    /// Set by `Player::play` until the decoder has handled the request, so
    /// status reads "playing" (loading) instead of "stopped" meanwhile.
    /// Not used by the callback.
    pub starting: AtomicBool,
    /// Set by the output's error callback (e.g. the device was unplugged).
    /// Playback is silent until the output is reopened. Not used by the
    /// data callback.
    pub output_failed: AtomicBool,
    /// Set by the callback when its last buffer held no audio from the ring.
    pub silent: AtomicBool,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            consumed: AtomicU64::new(0),
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            expecting_data: AtomicBool::new(false),
            underruns: AtomicU64::new(0),
            starting: AtomicBool::new(false),
            output_failed: AtomicBool::new(false),
            silent: AtomicBool::new(true),
        }
    }

    /// Paused, or out of audio with none coming, so the device stream may
    /// sleep. Not used by the callback.
    pub fn idle(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
            || (self.silent.load(Ordering::Acquire)
                && !self.expecting_data.load(Ordering::Acquire)
                && !self.starting.load(Ordering::Acquire))
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }
}

/// Frames `[from, to)` on the shared timeline that must be skipped rather
/// than played (stale audio after a seek, skip, stop, or queue edit).
/// Ranges are pushed in increasing order of `to`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiscardRange {
    pub from: u64,
    pub to: u64,
}

pub struct Renderer {
    samples: Consumer<f32>,
    discards: Consumer<DiscardRange>,
    channels: usize,
    shared: Arc<Shared>,
    /// Local copy of `shared.consumed` (only this thread writes it).
    consumed: u64,
}

impl Renderer {
    pub fn new(
        samples: Consumer<f32>,
        discards: Consumer<DiscardRange>,
        channels: usize,
        shared: Arc<Shared>,
    ) -> Self {
        Self {
            samples,
            discards,
            channels: channels.max(1),
            shared,
            consumed: 0,
        }
    }

    /// Call before moving the renderer into the audio callback.
    pub fn shared(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }

    /// Frames ready to play (ignoring pending discards).
    pub fn available_frames(&self) -> usize {
        self.samples.slots() / self.channels
    }

    /// Fills `out` (interleaved, device channel count) from the ring and
    /// returns how many frames came from the ring; the rest is silence.
    /// Realtime-safe: no allocation, locks, I/O, or logging.
    pub fn render(&mut self, out: &mut [f32]) -> usize {
        self.apply_discards();

        if self.shared.paused.load(Ordering::Relaxed) {
            out.fill(0.0);
            self.shared.silent.store(true, Ordering::Release);
            return 0;
        }

        let volume = self.shared.volume();
        let frames_wanted = out.len() / self.channels;
        let mut written = 0usize;
        while written < frames_wanted {
            self.apply_discards();
            let limit = self.frames_until_next_discard();
            let n = (frames_wanted - written)
                .min(self.available_frames())
                .min(limit);
            if n == 0 {
                break;
            }
            let Ok(chunk) = self.samples.read_chunk(n * self.channels) else {
                break;
            };
            let (a, b) = chunk.as_slices();
            let dst = &mut out[written * self.channels..(written + n) * self.channels];
            let (dst_a, dst_b) = dst.split_at_mut(a.len());
            for (d, s) in dst_a.iter_mut().zip(a) {
                *d = s * volume;
            }
            for (d, s) in dst_b.iter_mut().zip(b) {
                *d = s * volume;
            }
            chunk.commit_all();
            written += n;
            self.consumed += n as u64;
        }
        self.shared.consumed.store(self.consumed, Ordering::Release);
        self.shared.silent.store(written == 0, Ordering::Release);

        if written < frames_wanted {
            out[written * self.channels..].fill(0.0);
            if self.shared.expecting_data.load(Ordering::Acquire) {
                self.shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
        }
        written
    }

    fn apply_discards(&mut self) {
        loop {
            while let Ok(front) = self.discards.peek() {
                if front.to <= self.consumed {
                    let _ = self.discards.pop();
                } else {
                    break;
                }
            }
            let Some(to) = self.covering_range_end() else {
                break;
            };
            let want = (to - self.consumed) as usize;
            let n = want.min(self.available_frames());
            if n == 0 {
                break;
            }
            if let Ok(chunk) = self.samples.read_chunk(n * self.channels) {
                chunk.commit_all();
                self.consumed += n as u64;
            } else {
                break;
            }
            self.shared.consumed.store(self.consumed, Ordering::Release);
            if n < want {
                break; // the rest of the range hasn't been written yet
            }
        }
    }

    fn covering_range_end(&mut self) -> Option<u64> {
        let pos = self.consumed;
        let slots = self.discards.slots();
        let chunk = self.discards.read_chunk(slots).ok()?;
        let (a, b) = chunk.as_slices();
        // Not committing: this only peeks; ranges are popped once expired.
        a.iter()
            .chain(b)
            .filter(|r| r.from <= pos && pos < r.to)
            .map(|r| r.to)
            .max()
    }

    fn frames_until_next_discard(&mut self) -> usize {
        let pos = self.consumed;
        let slots = self.discards.slots();
        let Ok(chunk) = self.discards.read_chunk(slots) else {
            return usize::MAX;
        };
        let (a, b) = chunk.as_slices();
        a.iter()
            .chain(b)
            .filter(|r| r.from > pos)
            .map(|r| usize::try_from(r.from - pos).unwrap_or(usize::MAX))
            .min()
            .unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
#[path = "tests/render_test.rs"]
mod tests;
