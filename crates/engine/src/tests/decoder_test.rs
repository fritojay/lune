//! Single-threaded simulations: the test drives the decoder and the
//! renderer step by step, so ring-full and mid-transition states happen
//! deterministically instead of by timing.

use std::sync::mpsc;

use rtrb::RingBuffer;

use super::*;
use crate::VecQueue;
use crate::output::Signal;
use crate::render::Renderer;
use crate::test_util::{TempDir, write_wav};

const OUT_RATE: u32 = 48_000;

struct Sim {
    decoder: Decoder,
    renderer: Renderer,
    captured: u64,
    _commands: mpsc::Sender<Command>,
    wakes: mpsc::Receiver<Signal>,
}

impl Sim {
    fn new(queue: Arc<dyn PlayQueue>, ring_frames: usize) -> Self {
        let shared = Arc::new(Shared::new());
        let (producer, samples) = RingBuffer::new(ring_frames * 2);
        let (discards_tx, discards_rx) = RingBuffer::new(64);
        let (commands, command_rx) = mpsc::channel();
        let spec = OutputSpec {
            rate: OUT_RATE,
            channels: 2,
        };
        let timeline = Arc::new(Mutex::new(Timeline::new()));
        let (wake, wakes) = Waker::detached();
        Self {
            decoder: Decoder::new(
                queue,
                Arc::clone(&shared),
                timeline,
                spec,
                producer,
                discards_tx,
                command_rx,
                wake,
            ),
            renderer: Renderer::new(samples, discards_rx, 2, shared),
            captured: 0,
            _commands: commands,
            wakes,
        }
    }

    fn pump(&mut self) {
        let d = &mut self.decoder;
        d.flush_discards();
        if d.out_pos < d.out.len() {
            d.push_out();
            if d.out_pos < d.out.len() {
                return;
            }
        }
        d.work();
    }

    fn render(&mut self, frames: usize) {
        let mut buf = vec![0.0; frames * 2];
        self.captured += self.renderer.render(&mut buf) as u64;
    }

    fn finished(&self) -> bool {
        let d = &self.decoder;
        d.current.is_none()
            && d.resume_skip.is_none()
            && d.out_pos >= d.out.len()
            && self.renderer.available_frames() == 0
    }

    fn run_to_end(&mut self) {
        for _ in 0..1_000_000 {
            if self.finished() {
                return;
            }
            self.pump();
            self.render(256);
        }
        panic!("simulation did not finish");
    }
}

fn out_frames(frames: u64) -> u64 {
    (frames * u64::from(OUT_RATE) + 22_050) / 44_100
}

/// A queue edit that drops the pre-buffered next track while the playing
/// track's last frames are still waiting (in `out` or in the resampler)
/// must not cut those frames. The lengths put the track's end at
/// different points of the resampler's chunk and filter delay.
#[test]
fn a_queue_edit_keeps_the_end_of_the_playing_track() {
    for a_len in [3_000, 4_500, 6_000, 7_777, 10_000, 15_000] {
        let dir = TempDir::new();
        let (a, b, c) = (dir.join("a.wav"), dir.join("b.wav"), dir.join("c.wav"));
        write_wav(&a, 44_100, 2, 0, a_len);
        write_wav(&b, 44_100, 2, 0, 9_000);
        write_wav(&c, 44_100, 2, 0, 7_000);
        let queue = Arc::new(VecQueue::from_paths([a, b, c.clone()]));
        let mut sim = Sim::new(queue.clone(), 4096);
        let first = queue.first().unwrap();
        assert!(
            sim.decoder
                .handle(Command::Play(first.clone(), Duration::ZERO))
        );

        // Advance until B has a boundary, rendering only a trickle so the
        // ring stays nearly full.
        let mut reached = false;
        for _ in 0..100_000 {
            let tl = lock(&sim.decoder.timeline);
            if tl.boundaries.len() == 2 {
                reached = sim.decoder.written < tl.boundaries[1].start;
                break;
            }
            drop(tl);
            sim.pump();
            sim.render(16);
        }
        assert!(reached, "setup ({a_len}): A's tail was already in the ring");

        queue.set(vec![first, QueueEntry::new(EntryId(2), c)]);
        assert!(sim.decoder.handle(Command::QueueChanged));
        sim.run_to_end();

        let want = out_frames(a_len) + out_frames(7_000);
        assert_eq!(sim.captured, want, "A = {a_len} frames");
    }
}

/// A 44.1k -> 96k -> 48k sequence plays at the exact total
/// length (each rate change flushes the previous stream exactly), and
/// consecutive same-rate tracks share one stream.
#[test]
fn mixed_rates_play_at_the_exact_total_length() {
    let dir = TempDir::new();
    let tracks = [
        (44_100, 30_001),
        (44_100, 12_345),
        (96_000, 50_000),
        (48_000, 20_000),
    ];
    let paths: Vec<_> = tracks
        .iter()
        .enumerate()
        .map(|(i, &(rate, frames))| {
            let p = dir.join(&format!("{i}.wav"));
            write_wav(&p, rate, 2, 0, frames);
            p
        })
        .collect();
    let queue = Arc::new(VecQueue::from_paths(paths));
    let mut sim = Sim::new(queue.clone(), 8192);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
    );
    sim.run_to_end();
    // Same-rate neighbours share a stream, so round per stream.
    let stream = |rate: u64, frames: u64| (frames * u64::from(OUT_RATE) + rate / 2) / rate;
    let want = stream(44_100, 30_001 + 12_345) + stream(96_000, 50_000) + 20_000;
    assert_eq!(sim.captured, want);
}

#[test]
fn skipping_many_unplayable_entries_yields_between_batches() {
    let dir = TempDir::new();
    let good = dir.join("good.wav");
    write_wav(&good, OUT_RATE, 2, 0, 1000);
    let mut paths: Vec<_> = (0..100)
        .map(|i| dir.join(&format!("missing-{i}.wav")))
        .collect();
    paths.push(good);
    let queue = Arc::new(VecQueue::from_paths(paths));
    let mut sim = Sim::new(queue.clone(), 8192);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
    );
    assert_eq!(
        sim.decoder.resume_skip.as_ref().map(|e| e.id),
        Some(EntryId(32))
    );
    sim.run_to_end();
    assert_eq!(sim.captured, 1000);
}

fn current_id(sim: &Sim) -> Option<EntryId> {
    sim.decoder.current.as_ref().map(|c| c.entry.id)
}

#[test]
fn previous_skips_unplayable_entries_back_to_a_playable_one() {
    let dir = TempDir::new();
    let first = dir.join("first.wav");
    let last = dir.join("last.wav");
    write_wav(&first, OUT_RATE, 2, 0, 1000);
    write_wav(&last, OUT_RATE, 2, 0, 1000);
    let queue = Arc::new(VecQueue::from_paths([
        first,
        dir.join("missing-1.wav"),
        dir.join("missing-2.wav"),
        last,
    ]));
    let mut sim = Sim::new(queue.clone(), 8192);
    let last_entry = queue.next_after(EntryId(2)).unwrap();
    assert!(
        sim.decoder
            .handle(Command::Play(last_entry, Duration::ZERO))
    );
    assert!(sim.decoder.handle(Command::Previous));
    assert_eq!(current_id(&sim), Some(EntryId(0)));
}

#[test]
fn previous_restarts_the_track_when_only_unplayable_entries_precede_it() {
    let dir = TempDir::new();
    let last = dir.join("last.wav");
    write_wav(&last, OUT_RATE, 2, 0, 1000);
    let queue = Arc::new(VecQueue::from_paths([dir.join("missing.wav"), last]));
    let mut sim = Sim::new(queue.clone(), 8192);
    let last_entry = queue.next_after(EntryId(0)).unwrap();
    assert!(
        sim.decoder
            .handle(Command::Play(last_entry, Duration::ZERO))
    );
    assert!(sim.decoder.handle(Command::Previous));
    assert_eq!(current_id(&sim), Some(EntryId(1)));
    sim.run_to_end();
    assert_eq!(sim.captured, 1000);
}

#[test]
fn a_discard_wakes_the_output_so_a_sleeping_stream_still_applies_it() {
    let dir = TempDir::new();
    let a = dir.join("a.wav");
    write_wav(&a, 48_000, 2, 0, 48_000);
    let queue = Arc::new(VecQueue::from_paths([a]));
    let mut sim = Sim::new(queue.clone(), 8192);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
    );
    for _ in 0..100 {
        sim.pump();
    }
    sim.render(256);
    sim.decoder.shared.paused.store(true, Ordering::Relaxed);
    while sim.wakes.try_recv().is_ok() {}

    assert!(
        sim.decoder
            .handle(Command::Seek(Duration::from_millis(500)))
    );
    assert!(
        matches!(sim.wakes.try_recv(), Ok(Signal::Wake)),
        "the seek's discard did not wake the output"
    );
}

#[test]
fn audio_arriving_after_a_discard_wakes_the_output() {
    let dir = TempDir::new();
    let a = dir.join("a.wav");
    write_wav(&a, 48_000, 2, 0, 48_000);
    let queue = Arc::new(VecQueue::from_paths([a]));
    let mut sim = Sim::new(queue.clone(), 8192);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
    );
    assert!(
        sim.decoder
            .handle(Command::Seek(Duration::from_millis(500)))
    );
    while sim.wakes.try_recv().is_ok() {}

    while sim.renderer.available_frames() == 0 {
        sim.pump();
    }
    assert!(
        matches!(sim.wakes.try_recv(), Ok(Signal::Wake)),
        "the refill did not wake the output"
    );
}

#[test]
fn play_from_a_position_plays_only_the_rest_of_the_track() {
    let dir = TempDir::new();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    write_wav(&a, OUT_RATE, 2, 0, 48_000);
    write_wav(&b, OUT_RATE, 2, 0, 1000);
    let queue = Arc::new(VecQueue::from_paths([a, b]));
    let mut sim = Sim::new(queue.clone(), 8192);
    assert!(sim.decoder.handle(Command::Play(
        queue.first().unwrap(),
        Duration::from_millis(500)
    )));
    assert_eq!(
        sim.decoder.playing().map(|p| (p.entry.id, p.position)),
        Some((EntryId(0), Duration::from_millis(500)))
    );
    sim.run_to_end();
    assert_eq!(sim.captured, 24_000 + 1000);
}

/// Entries in order. `repeats` times, an entry is followed by itself
/// (repeat one); after that, by the next one, wrapping past the last when
/// `wrap` (repeat all). Skips never repeat.
struct Repeating {
    entries: Vec<QueueEntry>,
    repeats: Mutex<usize>,
    wrap: bool,
}

impl Repeating {
    fn new(paths: Vec<std::path::PathBuf>, repeats: usize, wrap: bool) -> Arc<Self> {
        let entries = (0u64..)
            .zip(paths)
            .map(|(i, p)| QueueEntry::new(EntryId(i), p))
            .collect();
        Arc::new(Self {
            entries,
            repeats: Mutex::new(repeats),
            wrap,
        })
    }

    fn neighbour(&self, id: EntryId, step: isize) -> Option<QueueEntry> {
        let i = self.entries.iter().position(|e| e.id == id)?;
        let len = self.entries.len() as isize;
        let j = i as isize + step;
        let j = if self.wrap { j.rem_euclid(len) } else { j };
        usize::try_from(j)
            .ok()
            .and_then(|j| self.entries.get(j))
            .cloned()
    }
}

impl PlayQueue for Repeating {
    fn next_after(&self, id: EntryId) -> Option<QueueEntry> {
        let mut repeats = self.repeats.lock().unwrap();
        if *repeats > 0 {
            *repeats -= 1;
            return self.entries.iter().find(|e| e.id == id).cloned();
        }
        drop(repeats);
        self.neighbour(id, 1)
    }

    fn previous_before(&self, id: EntryId) -> Option<QueueEntry> {
        self.neighbour(id, -1)
    }

    fn skip_after(&self, id: EntryId) -> Option<QueueEntry> {
        self.neighbour(id, 1)
    }
}

/// The (entry, playthrough) heard, each time it changes.
fn heard(sim: &mut Sim) -> Vec<(EntryId, u64)> {
    let mut heard = Vec::new();
    for _ in 0..1_000_000 {
        if sim.finished() {
            return heard;
        }
        sim.pump();
        sim.render(256);
        let consumed = sim.renderer.shared().consumed.load(Ordering::Acquire);
        let timeline = lock(&sim.decoder.timeline);
        let now = timeline
            .playing(consumed)
            .and_then(|i| timeline.boundaries.get(i))
            .map(|b| (b.entry.id, b.playthrough));
        if let Some(now) = now
            && heard.last() != Some(&now)
        {
            heard.push(now);
        }
    }
    panic!("simulation did not finish");
}

#[test]
fn a_repeated_track_plays_again_whole_as_a_new_playthrough() {
    let dir = TempDir::new();
    let a = dir.join("a.wav");
    write_wav(&a, OUT_RATE, 2, 0, 10_000);
    let queue = Repeating::new(vec![a], 1, false);
    let mut sim = Sim::new(queue.clone(), 4096);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.entries[0].clone(), Duration::ZERO))
    );
    assert_eq!(heard(&mut sim), [(EntryId(0), 1), (EntryId(0), 2)]);
    assert_eq!(
        sim.captured, 20_000,
        "both playthroughs, with nothing cut or added"
    );
}

#[test]
fn next_moves_on_from_a_repeated_track() {
    let dir = TempDir::new();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    write_wav(&a, OUT_RATE, 2, 0, 10_000);
    write_wav(&b, OUT_RATE, 2, 0, 10_000);
    let queue = Repeating::new(vec![a, b], usize::MAX, false);
    let mut sim = Sim::new(queue.clone(), 4096);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.entries[0].clone(), Duration::ZERO))
    );
    assert!(sim.decoder.handle(Command::Next));
    let current = sim.decoder.current.as_ref().map(|c| c.entry.id);
    assert_eq!(current, Some(EntryId(1)));
}

#[test]
fn a_repeating_queue_of_unplayable_tracks_stops_after_one_lap() {
    let dir = TempDir::new();
    let missing = (0..3)
        .map(|i| dir.join(&format!("missing-{i}.wav")))
        .collect();
    let queue = Repeating::new(missing, 0, true);
    let mut sim = Sim::new(queue.clone(), 4096);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.entries[0].clone(), Duration::ZERO))
    );
    for _ in 0..4 {
        sim.pump();
    }
    assert!(sim.finished());
}

#[test]
fn a_seek_stays_in_its_playthrough_and_a_play_from_a_position_starts_one() {
    let dir = TempDir::new();
    let a = dir.join("a.wav");
    write_wav(&a, OUT_RATE, 2, 0, 48_000);
    let queue = Repeating::new(vec![a], 0, false);
    let mut sim = Sim::new(queue.clone(), 4096);
    let playthroughs = |sim: &mut Sim| {
        for _ in 0..100 {
            sim.pump();
            sim.render(256);
        }
        let timeline = lock(&sim.decoder.timeline);
        timeline
            .boundaries
            .iter()
            .map(|b| b.playthrough)
            .collect::<Vec<_>>()
    };
    let entry = queue.entries[0].clone();
    assert!(
        sim.decoder
            .handle(Command::Play(entry.clone(), Duration::ZERO))
    );
    assert_eq!(playthroughs(&mut sim), [1]);
    assert!(
        sim.decoder
            .handle(Command::Seek(Duration::from_millis(500)))
    );
    assert_eq!(playthroughs(&mut sim), [1]);
    assert!(
        sim.decoder
            .handle(Command::Play(entry, Duration::from_millis(200)))
    );
    assert_eq!(playthroughs(&mut sim), [2]);
}
