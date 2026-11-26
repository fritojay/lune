//! The decoder thread. It keeps the ring buffer full: when a track ends it
//! asks the [`PlayQueue`] for the next entry and keeps decoding straight into
//! the same ring, so the next track is pre-buffered behind the current one
//! and the output stream never stops (gapless).

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rtrb::Producer;

use crate::convert::Converter;
use crate::output::{OutputSpec, Waker};
use crate::queue::{EntryId, PlayQueue, QueueEntry};
use crate::render::{DiscardRange, Shared};
use crate::source::Source;
use crate::timeline::{Boundary, Timeline, lock};
use crate::units::frames_to_duration;

/// Commands still wake it at once; this only paces refilling, which the
/// ring's seconds of audio leave plenty of time for.
const RING_FULL_WAIT: Duration = Duration::from_millis(100);
const MAX_OPEN_ATTEMPTS: usize = 32;
const PREVIOUS_RESTART_AFTER: Duration = Duration::from_secs(3);

pub(crate) enum Command {
    Play(QueueEntry, Duration),
    Stop,
    Seek(Duration),
    Next,
    Previous,
    QueueChanged,
    Shutdown,
}

struct Current {
    entry: QueueEntry,
    source: Source,
    /// Whether its boundary has been recorded (it has produced audio).
    started: bool,
    /// Track position of its first frame (non-zero after a seek).
    offset: Duration,
}

pub(crate) struct Decoder {
    queue: Arc<dyn PlayQueue>,
    shared: Arc<Shared>,
    timeline: Arc<Mutex<Timeline>>,
    spec: OutputSpec,
    producer: Producer<f32>,
    discards: Producer<DiscardRange>,
    /// Discards that didn't fit in the ring yet (kept in order).
    pending_discards: VecDeque<DiscardRange>,
    commands: Receiver<Command>,
    /// Woken after a discard is pushed (a stream asleep while paused must
    /// still apply it, or the status keeps showing the discarded audio) and
    /// when audio first arrives after one.
    wake: Waker,
    /// Frames pushed into the ring so far (the timeline's write position).
    written: u64,
    converter: Converter,
    /// Timeline frame of the converter's output index 0.
    converter_base: u64,
    current: Option<Current>,
    out: Vec<f32>,
    out_pos: usize,
    /// True after a deliberate discard (play, seek, skip, stop, queue edit)
    /// until new audio reaches the ring. The silence meanwhile is expected,
    /// so it is not counted as an underrun.
    refilling: bool,
    resume_skip: Option<QueueEntry>,
}

impl Decoder {
    #[expect(
        clippy::too_many_arguments,
        reason = "independent handles, each set up by the player"
    )]
    pub(crate) fn new(
        queue: Arc<dyn PlayQueue>,
        shared: Arc<Shared>,
        timeline: Arc<Mutex<Timeline>>,
        spec: OutputSpec,
        producer: Producer<f32>,
        discards: Producer<DiscardRange>,
        commands: Receiver<Command>,
        wake: Waker,
    ) -> Self {
        Self {
            queue,
            shared,
            timeline,
            spec,
            producer,
            discards,
            pending_discards: VecDeque::new(),
            commands,
            wake,
            written: 0,
            converter: Converter::new(spec.rate, spec.channels),
            converter_base: 0,
            current: None,
            out: Vec::new(),
            out_pos: 0,
            refilling: false,
            resume_skip: None,
        }
    }

    pub(crate) fn run(mut self) {
        loop {
            let has_work = self.current.is_some()
                || self.out_pos < self.out.len()
                || self.resume_skip.is_some();
            self.shared
                .expecting_data
                .store(has_work && !self.refilling, Ordering::Release);

            // Commands first, so controls stay responsive.
            let command = if has_work {
                match self.commands.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return,
                }
            } else {
                match self.commands.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            };
            if let Some(command) = command {
                if !self.handle(command) {
                    return;
                }
                continue;
            }

            self.flush_discards();
            if self.out_pos < self.out.len() {
                self.push_out();
                if self.out_pos < self.out.len() {
                    // Ring full: wait for room, but wake for commands.
                    match self.commands.recv_timeout(RING_FULL_WAIT) {
                        Ok(command) => {
                            if !self.handle(command) {
                                return;
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    continue;
                }
            }
            self.work();
        }
    }

    fn work(&mut self) {
        match self.resume_skip.take() {
            Some(entry) => self.start_entry(Some(entry), Duration::ZERO),
            None => self.decode_step(),
        }
    }

    /// Returns false on shutdown.
    fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Shutdown => return false,
            Command::Play(entry, position) => {
                self.discard_all();
                self.shared.paused.store(false, Ordering::Relaxed);
                if position.is_zero() {
                    self.start_entry(Some(entry), Duration::ZERO);
                } else {
                    self.seek(entry, position);
                }
                self.shared.starting.store(false, Ordering::Release);
            }
            Command::Stop => {
                self.discard_all();
                self.shared.paused.store(false, Ordering::Relaxed);
                lock(&self.timeline).end = Some(self.written);
            }
            Command::Seek(position) => {
                if let Some((entry, _)) = self.playing() {
                    self.seek(entry, position);
                }
            }
            Command::Next => {
                if let Some((entry, _)) = self.playing() {
                    match self.queue.next_after(entry.id) {
                        Some(next) => {
                            self.discard_all();
                            self.start_entry(Some(next), Duration::ZERO);
                        }
                        None => {
                            self.discard_all();
                            lock(&self.timeline).end = Some(self.written);
                        }
                    }
                }
            }
            Command::Previous => {
                if let Some((entry, position)) = self.playing() {
                    let previous = (position <= PREVIOUS_RESTART_AFTER)
                        .then(|| self.open_previous(entry.id))
                        .flatten();
                    match previous {
                        Some((prev, source)) => {
                            self.discard_all();
                            self.begin(prev, source, Duration::ZERO);
                        }
                        None => self.seek(entry, Duration::ZERO),
                    }
                }
            }
            Command::QueueChanged => self.revalidate_next(),
        }
        true
    }

    /// The entry being heard and its position.
    fn playing(&self) -> Option<(QueueEntry, Duration)> {
        let consumed = self.shared.consumed.load(Ordering::Acquire);
        let timeline = lock(&self.timeline);
        if timeline.end.is_some_and(|end| consumed >= end) {
            return None;
        }
        match timeline
            .playing(consumed)
            .and_then(|i| timeline.boundaries.get(i))
        {
            Some(b) => Some((
                b.entry.clone(),
                b.offset + frames_to_duration(consumed.saturating_sub(b.start), self.spec.rate),
            )),
            // Started but no audio yet (e.g. just after Play).
            None => self.current.as_ref().map(|c| (c.entry.clone(), c.offset)),
        }
    }

    fn seek(&mut self, entry: QueueEntry, position: Duration) {
        let reuse = self
            .current
            .take()
            .filter(|c| c.entry.id == entry.id)
            .map(|c| c.source);
        self.discard_all();
        let source = match reuse {
            Some(source) => Ok(source),
            None => Source::open(&entry.path),
        };
        match source.and_then(|mut s| s.seek(position).map(|at| (s, at))) {
            Ok((source, offset)) => {
                self.current = Some(Current {
                    entry,
                    source,
                    started: false,
                    offset,
                });
            }
            Err(err) => {
                tracing::warn!(err = %err, "seek failed; skipping to the next track");
                let next = self.queue.next_after(entry.id);
                self.start_entry(next, Duration::ZERO);
            }
        }
    }

    fn discard_all(&mut self) {
        self.refilling = true;
        self.queue_discard(DiscardRange {
            from: 0,
            to: self.written,
        });
        self.out.clear();
        self.out_pos = 0;
        self.converter = Converter::new(self.spec.rate, self.spec.channels);
        self.converter_base = self.written;
        self.current = None;
        self.resume_skip = None;
        let mut timeline = lock(&self.timeline);
        timeline.boundaries.clear();
        timeline.end = None;
    }

    fn begin(&mut self, entry: QueueEntry, source: Source, offset: Duration) {
        self.current = Some(Current {
            entry,
            source,
            started: false,
            offset,
        });
        lock(&self.timeline).end = None;
    }

    /// The nearest playable entry before `id`, opened. Unplayable ones (e.g.
    /// missing files) are skipped, up to [`MAX_OPEN_ATTEMPTS`] of them.
    fn open_previous(&self, id: EntryId) -> Option<(QueueEntry, Source)> {
        let mut entry = self.queue.previous_before(id);
        for _ in 0..MAX_OPEN_ATTEMPTS {
            let e = entry?;
            match Source::open(&e.path) {
                Ok(source) => return Some((e, source)),
                Err(err) => {
                    tracing::warn!(err = %err, "skipping unplayable track");
                    entry = self.queue.previous_before(e.id);
                }
            }
        }
        None
    }

    /// Gives up after [`MAX_OPEN_ATTEMPTS`] unplayable entries and leaves the
    /// rest to the next loop iteration, so a queue of missing files (e.g. on
    /// an unmounted drive) can't stall command handling. With no entry left,
    /// finishes the stream.
    fn start_entry(&mut self, mut entry: Option<QueueEntry>, offset: Duration) {
        self.resume_skip = None;
        let mut attempts = 0;
        while let Some(e) = entry {
            if attempts == MAX_OPEN_ATTEMPTS {
                self.resume_skip = Some(e);
                return;
            }
            attempts += 1;
            match Source::open(&e.path) {
                Ok(source) => {
                    self.begin(e, source, offset);
                    return;
                }
                Err(err) => {
                    tracing::warn!(err = %err, "skipping unplayable track");
                    entry = self.queue.next_after(e.id);
                }
            }
        }
        // End of the queue: flush the resampler tail and mark where playback ends.
        if let Err(err) = self.converter.finish(&mut self.out) {
            tracing::warn!(err = %err, "resampler flush failed");
        }
        let pending = ((self.out.len() - self.out_pos) / self.spec.channels) as u64;
        lock(&self.timeline).end = Some(self.written + pending);
    }

    fn decode_step(&mut self) {
        let Some(current) = self.current.as_mut() else {
            return;
        };
        let duration = current.source.duration();
        match current.source.next_chunk() {
            Ok(Some(chunk)) => {
                if chunk.samples.is_empty() {
                    return;
                }
                let index = match self
                    .converter
                    .prepare(chunk.rate, chunk.channels, &mut self.out)
                {
                    Ok(index) => index,
                    Err(err) => {
                        tracing::error!(err = %err, "cannot convert audio; skipping track");
                        self.advance();
                        return;
                    }
                };
                if !current.started {
                    current.started = true;
                    self.converter.track_starts_at(index);
                    let consumed = self.shared.consumed.load(Ordering::Acquire);
                    let mut timeline = lock(&self.timeline);
                    if let Some(playing) = timeline.playing(consumed) {
                        timeline.boundaries.drain(..playing);
                    }
                    timeline.boundaries.push_back(Boundary {
                        start: self.converter_base + index,
                        entry: current.entry.clone(),
                        offset: current.offset,
                        duration,
                    });
                    timeline.end = None;
                }
                if let Err(err) =
                    self.converter
                        .process(chunk.samples, current.entry.gain, &mut self.out)
                {
                    tracing::error!(err = %err, "cannot convert audio; skipping track");
                    self.advance();
                }
            }
            Ok(None) => self.advance(),
            Err(err) => {
                tracing::warn!(err = %err, "decode failed; skipping to the next track");
                self.advance();
            }
        }
    }

    /// Doesn't discard anything: this is the gapless path.
    fn advance(&mut self) {
        let next = self
            .current
            .take()
            .and_then(|c| self.queue.next_after(c.entry.id));
        self.start_entry(next, Duration::ZERO);
    }

    /// After a queue edit: checks that every track buffered after the playing
    /// one (and the one being decoded) is still what the queue says comes
    /// next. At the first mismatch, discards from there and decodes the
    /// correct next track instead.
    fn revalidate_next(&mut self) {
        let consumed = self.shared.consumed.load(Ordering::Acquire);
        let (chain, playing_ix) = {
            let timeline = lock(&self.timeline);
            let Some(p) = timeline.playing(consumed) else {
                return;
            };
            let chain: Vec<(u64, EntryId)> = timeline
                .boundaries
                .iter()
                .map(|b| (b.start, b.entry.id))
                .collect();
            (chain, p)
        };

        let mut prev = chain[playing_ix].1;
        for &(start, id) in &chain[playing_ix + 1..] {
            let expected = self.queue.next_after(prev);
            if expected.as_ref().map(|e| e.id) != Some(id) {
                self.truncate_from(start, expected);
                return;
            }
            prev = id;
        }
        let expected = self.queue.next_after(prev);
        match &self.current {
            Some(c) if c.started => {} // it's the last boundary, already checked
            Some(c) => {
                if expected.as_ref().map(|e| e.id) != Some(c.entry.id) {
                    self.current = None;
                    self.start_entry(expected, Duration::ZERO);
                }
            }
            None => {
                // Decoding had reached the end of the queue; resume if the
                // queue now has more.
                if expected.is_some() {
                    self.start_entry(expected, Duration::ZERO);
                }
            }
        }
    }

    /// `from` can be past `written`: with the ring full, the end of the track
    /// before `from` may still be waiting in `out`, or even in the resampler.
    /// Those frames are kept (and flushed); only what lies at or after `from`
    /// is dropped.
    fn truncate_from(&mut self, from: u64, next: Option<QueueEntry>) {
        self.refilling = true;
        self.queue_discard(DiscardRange {
            from,
            to: self.written,
        });
        {
            let mut timeline = lock(&self.timeline);
            timeline.boundaries.retain(|b| b.start < from);
            timeline.end = None;
        }
        let channels = self.spec.channels;
        let pending = (self.out.len() - self.out_pos) / channels;
        let keep = usize::try_from(from.saturating_sub(self.written))
            .unwrap_or(usize::MAX)
            .min(pending);
        self.out.drain(..self.out_pos);
        self.out.truncate(keep * channels);
        self.out_pos = 0;
        // The resampler may still hold the end of that track; flush it out.
        if let Some(target) = from.checked_sub(self.converter_base)
            && let Err(err) = self.converter.finish_at(target, &mut self.out)
        {
            tracing::warn!(err = %err, "resampler flush failed");
        }
        self.converter = Converter::new(self.spec.rate, self.spec.channels);
        self.converter_base = self.written + (self.out.len() / channels) as u64;
        self.current = None;
        self.start_entry(next, Duration::ZERO);
    }

    fn queue_discard(&mut self, range: DiscardRange) {
        if range.to > range.from {
            self.pending_discards.push_back(range);
        }
        self.flush_discards();
    }

    fn flush_discards(&mut self) {
        let mut pushed = false;
        while let Some(&range) = self.pending_discards.front() {
            if self.discards.push(range).is_err() {
                break; // full; retried on the next loop
            }
            self.pending_discards.pop_front();
            pushed = true;
        }
        if pushed {
            self.wake.wake();
        }
    }

    fn push_out(&mut self) {
        let channels = self.spec.channels;
        let remaining = self.out.len() - self.out_pos;
        let n = remaining.min(self.producer.slots() / channels * channels);
        if n == 0 {
            return;
        }
        if let Ok(mut chunk) = self.producer.write_chunk(n) {
            let (a, b) = chunk.as_mut_slices();
            let src = &self.out[self.out_pos..self.out_pos + n];
            a.copy_from_slice(&src[..a.len()]);
            b.copy_from_slice(&src[a.len()..]);
            chunk.commit_all();
            self.out_pos += n;
            self.written += (n / channels) as u64;
            if self.refilling {
                self.refilling = false;
                // The stream may have slept while this loaded, and a check
                // can miss audio that is all pushed between two of them.
                self.wake.wake();
            }
        }
        if self.out_pos == self.out.len() {
            self.out.clear();
            self.out_pos = 0;
        }
    }
}

#[cfg(test)]
#[path = "tests/decoder_test.rs"]
mod tests;
