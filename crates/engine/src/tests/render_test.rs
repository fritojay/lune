use rtrb::{Producer, RingBuffer};

use super::*;
use crate::alloc_counter;

/// A renderer (stereo) whose ring holds frames `0..frames`, each frame's
/// samples equal to its index.
fn setup(frames: usize) -> (Renderer, Producer<DiscardRange>, Arc<Shared>) {
    let (mut samples_tx, samples_rx) = RingBuffer::new(frames * 2);
    for i in 0..frames {
        samples_tx.push(i as f32).unwrap();
        samples_tx.push(i as f32).unwrap();
    }
    let (discards_tx, discards_rx) = RingBuffer::new(8);
    let shared = Arc::new(Shared::new());
    let renderer = Renderer::new(samples_rx, discards_rx, 2, Arc::clone(&shared));
    (renderer, discards_tx, shared)
}

#[test]
fn discarded_frames_are_skipped_but_counted_as_consumed() {
    let (mut r, mut discards, shared) = setup(100);
    discards.push(DiscardRange { from: 10, to: 20 }).unwrap();
    let mut out = vec![-1.0; 200];
    let played = r.render(&mut out);
    assert_eq!(played, 90);
    let left: Vec<f32> = out.iter().step_by(2).copied().collect();
    let expected: Vec<f32> = (0..10).chain(20..100).map(|i| i as f32).collect();
    assert_eq!(&left[..90], expected.as_slice());
    assert!(left[90..].iter().all(|s| *s == 0.0), "the rest is silence");
    assert_eq!(shared.consumed.load(Ordering::Acquire), 100);
}

#[test]
fn paused_output_is_silent_and_consumes_nothing() {
    let (mut r, _discards, shared) = setup(10);
    shared.paused.store(true, Ordering::Relaxed);
    let mut out = vec![-1.0; 8];
    assert_eq!(r.render(&mut out), 0);
    assert!(out.iter().all(|s| *s == 0.0));
    assert_eq!(r.available_frames(), 10);
}

#[test]
fn render_never_allocates() {
    let (mut r, mut discards, shared) = setup(4096);
    discards.push(DiscardRange { from: 100, to: 300 }).unwrap();
    discards
        .push(DiscardRange {
            from: 1000,
            to: 1100,
        })
        .unwrap();
    let mut out = vec![0.0; 512];
    shared.expecting_data.store(true, Ordering::Release);
    let before = alloc_counter::count();
    for i in 0..20 {
        shared.paused.store(i % 5 == 4, Ordering::Relaxed);
        let _ = r.render(&mut out); // includes discards and, at the end, underruns
    }
    assert_eq!(alloc_counter::count(), before, "Renderer::render allocated");
}

#[test]
fn realtime_code_has_no_obvious_allocation_locking_or_logging() {
    let source = include_str!("../render.rs");
    let code = source.split("#[cfg(test)]").next().unwrap();
    for banned in [
        "tracing::",
        "println!",
        "eprintln!",
        "format!",
        "Mutex",
        "RwLock",
        ".lock(",
        "Vec::",
        "vec!",
        "Box::",
        "String",
        "to_owned",
        "to_string",
        "clone()",
    ] {
        assert!(
            !code.contains(banned),
            "render.rs must not use `{banned}` (realtime)"
        );
    }
}

#[test]
fn a_fresh_output_is_idle() {
    assert!(Shared::new().idle());
}

#[test]
fn the_output_is_idle_once_the_last_audio_has_played() {
    let (mut r, _discards, shared) = setup(10);
    let mut out = vec![0.0; 8];
    assert_eq!(r.render(&mut out), 4);
    assert!(!shared.idle(), "audio is still playing");
    assert_eq!(r.render(&mut out), 4);
    assert_eq!(r.render(&mut out), 2, "the last frames, then silence");
    assert!(!shared.idle(), "a partly filled buffer still played audio");
    assert_eq!(r.render(&mut out), 0);
    assert!(shared.idle());
}

#[test]
fn an_underrun_or_a_loading_track_is_not_idle() {
    let (mut r, _discards, shared) = setup(0);
    let mut out = vec![0.0; 8];
    r.render(&mut out);
    shared.expecting_data.store(true, Ordering::Release);
    assert!(!shared.idle(), "more audio is coming");
    shared.expecting_data.store(false, Ordering::Release);
    shared.starting.store(true, Ordering::Release);
    assert!(!shared.idle(), "a track is loading");
}

#[test]
fn a_paused_output_is_idle_even_with_audio_left() {
    let (mut r, _discards, shared) = setup(10);
    let mut out = vec![0.0; 8];
    r.render(&mut out);
    shared.expecting_data.store(true, Ordering::Release);
    shared.paused.store(true, Ordering::Relaxed);
    assert!(shared.idle());
    shared.paused.store(false, Ordering::Relaxed);
    assert!(!shared.idle());
}
