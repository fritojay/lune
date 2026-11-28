use std::time::Instant;

use super::*;
use crate::VecQueue;
use crate::test_util::{TempDir, capture, play_to_end, play_to_end_with, wait_stopped, write_wav};

#[test]
fn a_decoder_panic_skips_to_the_next_track() {
    let dir = TempDir::new();
    let bad = dir.join("panic-on-open.wav");
    let good = dir.join("good.wav");
    write_wav(&bad, 48_000, 2, 0, 1000);
    write_wav(&good, 48_000, 2, 0, 12_345);
    let queue = Arc::new(VecQueue::from_paths([bad, good]));
    let first = queue.first().unwrap();
    let frames = play_to_end(&dir, 48_000, queue, first);
    assert_eq!(frames, 12_345);
}

#[test]
fn a_track_with_an_absurd_sample_rate_is_skipped() {
    let dir = TempDir::new();
    let bad = dir.join("bad.wav");
    let good = dir.join("good.wav");
    write_wav(&bad, 4_294_967_291, 2, 0, 1000);
    write_wav(&good, 48_000, 2, 0, 12_345);
    let queue = Arc::new(VecQueue::from_paths([bad, good]));
    let first = queue.first().unwrap();
    assert_eq!(play_to_end(&dir, 48_000, queue, first), 12_345);
}

#[test]
fn a_resampler_panic_skips_to_the_next_track() {
    let dir = TempDir::new();
    let bad = dir.join("bad.wav");
    let good = dir.join("good.wav");
    write_wav(&bad, crate::convert::PANIC_TEST_RATE, 2, 0, 1000);
    write_wav(&good, 48_000, 2, 0, 12_345);
    let queue = Arc::new(VecQueue::from_paths([bad, good]));
    let first = queue.first().unwrap();
    assert_eq!(play_to_end(&dir, 48_000, queue, first), 12_345);
}

#[test]
fn same_format_tracks_play_back_to_back_at_exact_length() {
    let dir = TempDir::new();
    let a = dir.join("a.wav");
    let b = dir.join("b.wav");
    write_wav(&a, 48_000, 2, 0, 30_000);
    write_wav(&b, 48_000, 2, 30_000, 20_001);
    let queue = Arc::new(VecQueue::from_paths([a, b]));
    let first = queue.first().unwrap();
    assert_eq!(play_to_end(&dir, 48_000, queue, first), 50_001);
}

#[test]
fn without_gapless_every_frame_of_each_track_plays() {
    let dir = TempDir::new();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    write_wav(&a, 44_100, 2, 0, 30_000);
    write_wav(&b, 48_000, 2, 0, 20_001);
    let queue = Arc::new(VecQueue::from_paths([a, b]));
    let first = queue.first().unwrap();
    let config = |out: &std::path::Path| PlayerConfig {
        gapless: false,
        ..capture(out, 48_000)
    };
    let frames = play_to_end_with(&dir, config, queue, first);
    assert_eq!(frames, 32_653 + 20_001);
}

/// The decoder waits for a resume while a track plays out paused, so the
/// resume must wake it, or the next track never opens. Realtime, so A is
/// still playing when it pauses.
#[test]
fn without_gapless_a_resume_while_a_track_plays_out_goes_on_to_the_next() {
    let dir = TempDir::new();
    let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
    write_wav(&a, 48_000, 2, 0, 24_000);
    write_wav(&b, 48_000, 2, 0, 4_800);
    let queue = Arc::new(VecQueue::from_paths([a, b]));
    let first = queue.first().unwrap();
    let player = Player::new(
        PlayerConfig {
            output: OutputKind::Capture {
                path: None,
                rate: 48_000,
                channels: 2,
                realtime: true,
            },
            gapless: false,
            ..PlayerConfig::default()
        },
        queue,
    )
    .unwrap();
    player.play(first.clone()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while player.status().entry != Some(first.id) {
        assert!(Instant::now() < deadline, "A never played");
        std::thread::sleep(Duration::from_millis(1));
    }
    player.pause();
    // Time for the decoder to read the rest of A and wait for the resume.
    std::thread::sleep(Duration::from_millis(100));
    player.resume();
    wait_stopped(&player);
    assert_eq!(player.status().underruns, 0);
}
