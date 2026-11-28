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
use crate::units::duration_to_frames;

const OUT_RATE: u32 = 48_000;

struct Sim {
    decoder: Decoder,
    renderer: Renderer,
    captured: u64,
    /// Frames rendered silent between the first audio and the end.
    silence: u64,
    _commands: mpsc::Sender<Command>,
    wakes: mpsc::Receiver<Signal>,
}

impl Sim {
    fn new(queue: Arc<dyn PlayQueue>, ring_frames: usize) -> Self {
        Self::with_gapless(queue, ring_frames, true)
    }

    fn with_gapless(queue: Arc<dyn PlayQueue>, ring_frames: usize, gapless: bool) -> Self {
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
                gapless,
            ),
            renderer: Renderer::new(samples, discards_rx, 2, shared),
            captured: 0,
            silence: 0,
            _commands: commands,
            wakes,
        }
    }

    /// One step of `Decoder::run`, without a command. Its waits pass as the
    /// output would meanwhile: a timed one plays that much and, as there,
    /// is checked again; one for a command, or a timed one that played
    /// nothing (paused), ends the step.
    fn pump(&mut self) {
        loop {
            self.decoder.update_expecting_data();
            match self.decoder.wait() {
                Wait::Poll => break,
                Wait::For(wait) => {
                    let frames = duration_to_frames(wait, OUT_RATE).max(1);
                    if self.render(usize::try_from(frames).unwrap()) == 0 {
                        return;
                    }
                }
                Wait::UntilCommand => return,
            }
        }
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

    /// Returns the frames that came from the ring.
    fn render(&mut self, frames: usize) -> usize {
        let mut buf = vec![0.0; frames * 2];
        let got = self.renderer.render(&mut buf);
        let started = self.captured > 0;
        self.captured += got as u64;
        if started && !self.finished() {
            self.silence += (frames - got) as u64;
        }
        got
    }

    fn finished(&self) -> bool {
        let d = &self.decoder;
        d.current.is_none()
            && d.resume_skip.is_none()
            && d.ended.is_none()
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
    entries: Mutex<Vec<QueueEntry>>,
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
            entries: Mutex::new(entries),
            repeats: Mutex::new(repeats),
            wrap,
        })
    }

    fn entry(&self, index: usize) -> QueueEntry {
        self.entries.lock().unwrap()[index].clone()
    }

    fn remove(&self, id: EntryId) {
        self.entries.lock().unwrap().retain(|e| e.id != id);
    }

    fn neighbour(&self, id: EntryId, step: isize) -> Option<QueueEntry> {
        let entries = self.entries.lock().unwrap();
        let i = entries.iter().position(|e| e.id == id)?;
        let len = entries.len() as isize;
        let j = i as isize + step;
        let j = if self.wrap { j.rem_euclid(len) } else { j };
        usize::try_from(j)
            .ok()
            .and_then(|j| entries.get(j))
            .cloned()
    }
}

impl PlayQueue for Repeating {
    fn next_after(&self, id: EntryId) -> Option<QueueEntry> {
        let mut repeats = self.repeats.lock().unwrap();
        if *repeats > 0 {
            *repeats -= 1;
            return self
                .entries
                .lock()
                .unwrap()
                .iter()
                .find(|e| e.id == id)
                .cloned();
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
    for gapless in [true, false] {
        let dir = TempDir::new();
        let a = dir.join("a.wav");
        write_wav(&a, OUT_RATE, 2, 0, 10_000);
        let queue = Repeating::new(vec![a], 1, false);
        let mut sim = Sim::with_gapless(queue.clone(), 4096, gapless);
        assert!(
            sim.decoder
                .handle(Command::Play(queue.entry(0), Duration::ZERO))
        );
        assert_eq!(
            heard(&mut sim),
            [(EntryId(0), 1), (EntryId(0), 2)],
            "gapless {gapless}"
        );
        assert_eq!(
            sim.captured, 20_000,
            "gapless {gapless}: both playthroughs, with nothing cut or added"
        );
    }
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
            .handle(Command::Play(queue.entry(0), Duration::ZERO))
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
            .handle(Command::Play(queue.entry(0), Duration::ZERO))
    );
    for _ in 0..4 {
        sim.pump();
    }
    assert!(sim.finished());
}

#[test]
fn a_repeating_queue_of_unplayable_tracks_stops_after_one_lap_when_its_first_is_removed_midway() {
    let dir = TempDir::new();
    let missing = (0..MAX_OPEN_ATTEMPTS * 2)
        .map(|i| dir.join(&format!("missing-{i}.wav")))
        .collect();
    let queue = Repeating::new(missing, 0, true);
    let mut sim = Sim::new(queue.clone(), 4096);
    let first = queue.entry(0);
    assert!(
        sim.decoder
            .handle(Command::Play(first.clone(), Duration::ZERO))
    );
    assert!(
        !sim.finished(),
        "the lap is longer than one round of attempts"
    );
    queue.remove(first.id);
    assert!(sim.decoder.handle(Command::QueueChanged));
    for _ in 0..10 {
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
    let entry = queue.entry(0);
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

/// Plays until the decoder is done with the playing track and only its
/// last audio is left in the ring.
fn play_until_it_plays_out(sim: &mut Sim) {
    for _ in 0..100_000 {
        sim.pump();
        if current_id(sim).is_none() && sim.renderer.available_frames() > 0 {
            return;
        }
        sim.render(16);
    }
    panic!("the track never played out");
}

/// A's lengths put its end at different points of the resampler's chunk
/// and filter delay, whose tail must still play before the gap; B at the
/// output rate starts a stream of another format.
#[test]
fn without_gapless_the_next_track_opens_once_the_last_one_has_played_out() {
    for (a_len, b_rate) in [
        (3_000, 44_100),
        (7_777, 44_100),
        (15_000, 44_100),
        (7_777, OUT_RATE),
    ] {
        let dir = TempDir::new();
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        write_wav(&a, 44_100, 2, 0, a_len);
        write_wav(&b, b_rate, 2, 0, 9_000);
        let b_frames = if b_rate == OUT_RATE {
            9_000
        } else {
            out_frames(9_000)
        };
        let queue = Arc::new(VecQueue::from_paths([a, b]));
        let mut sim = Sim::with_gapless(queue.clone(), 4096, false);
        assert!(
            sim.decoder
                .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
        );

        let mut opened_after = None;
        for _ in 0..100_000 {
            sim.pump();
            if current_id(&sim) == Some(EntryId(1)) {
                opened_after = Some((sim.captured, sim.renderer.available_frames()));
                break;
            }
            sim.render(256);
        }
        let case = format!("A = {a_len}, B at {b_rate} Hz");
        assert_eq!(
            opened_after,
            Some((out_frames(a_len), 0)),
            "{case}: B opens after all of A played, and not before"
        );

        sim.run_to_end();
        assert_eq!(sim.captured, out_frames(a_len) + b_frames, "{case}");
        assert_eq!(
            sim.renderer.shared().underruns.load(Ordering::Relaxed),
            0,
            "{case}: the gap is not an underrun"
        );
    }
}

/// Each step renders 256 frames (5 ms), and the decoder's waits play
/// what they last. With gapless off the gap is the steps that open B,
/// decode its first audio, and push it: a wait added past the end of A, or
/// a track's tail played late, would lengthen it.
#[test]
fn without_gapless_the_gap_is_only_the_time_to_open_the_next_track() {
    for (gapless, most) in [(true, 0), (false, 2 * 256)] {
        let dir = TempDir::new();
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        write_wav(&a, 44_100, 2, 0, 7_777);
        write_wav(&b, 44_100, 2, 7_777, 9_000);
        let queue = Arc::new(VecQueue::from_paths([a, b]));
        let mut sim = Sim::with_gapless(queue.clone(), 4096, gapless);
        assert!(
            sim.decoder
                .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
        );
        sim.run_to_end();
        assert_eq!(sim.captured, out_frames(7_777) + out_frames(9_000));
        assert!(
            sim.silence <= most,
            "gapless {gapless}: {} silent frames",
            sim.silence
        );
    }
}

/// What a command, given while A plays out, leads to: what is heard next,
/// and the frames played in all, from those played before it.
type PlayOutCase = (
    &'static str,
    fn(&mut Sim, &VecQueue),
    &'static [(u64, u64)],
    fn(u64) -> u64,
);

#[test]
fn without_gapless_commands_while_a_track_plays_out_take_effect() {
    const LEN: u64 = 6_000;
    let cases: [PlayOutCase; 6] = [
        (
            "next",
            |sim, _| assert!(sim.decoder.handle(Command::Next)),
            &[(1, 2), (2, 3)],
            |before| before + 2 * LEN,
        ),
        (
            "stop",
            |sim, _| assert!(sim.decoder.handle(Command::Stop)),
            &[],
            |before| before,
        ),
        (
            "seek",
            |sim, _| {
                assert!(sim.decoder.handle(Command::Seek(Duration::from_millis(50))));
            },
            &[(0, 1), (1, 2), (2, 3)],
            |before| before + (LEN - 2_400) + 2 * LEN,
        ),
        (
            "previous, at the first track",
            |sim, _| assert!(sim.decoder.handle(Command::Previous)),
            &[(0, 1), (1, 2), (2, 3)],
            |before| before + 3 * LEN,
        ),
        (
            "gapless turned back on",
            |sim, _| assert!(sim.decoder.handle(Command::SetGapless(true))),
            &[(0, 1), (1, 2), (2, 3)],
            |_| 3 * LEN,
        ),
        (
            "queue edit",
            |sim, queue| {
                let a = queue.first().unwrap();
                let c = queue.next_after(EntryId(1)).unwrap();
                queue.set(vec![a, c]);
                assert!(sim.decoder.handle(Command::QueueChanged));
                assert_eq!(current_id(sim), None, "C waits for A to play out");
            },
            &[(0, 1), (2, 2)],
            |_| 2 * LEN,
        ),
    ];
    for (name, command, then, total) in cases {
        let dir = TempDir::new();
        let paths = ["a", "b", "c"].map(|name| {
            let path = dir.join(&format!("{name}.wav"));
            write_wav(&path, OUT_RATE, 2, 0, LEN);
            path
        });
        let queue = Arc::new(VecQueue::from_paths(paths));
        let mut sim = Sim::with_gapless(queue.clone(), 4096, false);
        assert!(
            sim.decoder
                .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
        );
        play_until_it_plays_out(&mut sim);
        let before = sim.captured;

        command(&mut sim, &queue);
        let then: Vec<_> = then.iter().map(|&(id, p)| (EntryId(id), p)).collect();
        assert_eq!(heard(&mut sim), then, "{name}");
        assert_eq!(sim.captured, total(before), "{name}");
    }
}

#[test]
fn without_gapless_a_pause_while_a_track_plays_out_holds_the_next_one_back() {
    let dir = TempDir::new();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    write_wav(&a, OUT_RATE, 2, 0, 30_000);
    write_wav(&b, OUT_RATE, 2, 0, 30_000);
    let queue = Arc::new(VecQueue::from_paths([a, b]));
    // Holds more than `RING_FULL_WAIT` of A's end.
    let mut sim = Sim::with_gapless(queue.clone(), 24_000, false);
    assert!(
        sim.decoder
            .handle(Command::Play(queue.first().unwrap(), Duration::ZERO))
    );
    play_until_it_plays_out(&mut sim);
    let before = sim.captured;

    let shared = sim.renderer.shared();
    shared.paused.store(true, Ordering::Relaxed);
    for _ in 0..1_000 {
        sim.pump();
        sim.render(256);
    }
    assert_eq!(sim.captured, before, "nothing plays while paused");
    assert_eq!(current_id(&sim), None, "B waits for A to play out");
    assert_eq!(
        sim.decoder.wait(),
        Wait::UntilCommand,
        "no timed wakeups while paused"
    );

    shared.paused.store(false, Ordering::Relaxed);
    assert!(sim.decoder.handle(Command::Resumed));
    assert!(
        matches!(sim.decoder.wait(), Wait::For(wait) if wait <= RING_FULL_WAIT),
        "a capped wait for A to play out"
    );
    assert_eq!(heard(&mut sim), [(EntryId(0), 1), (EntryId(1), 2)]);
    assert_eq!(sim.captured, 60_000);
}
