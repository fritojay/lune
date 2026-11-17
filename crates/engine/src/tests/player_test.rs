use super::*;
use crate::VecQueue;
use crate::test_util::{TempDir, play_to_end, write_wav};

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
