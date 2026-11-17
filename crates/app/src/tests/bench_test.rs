use super::FrameStats;

#[test]
fn no_frames_gives_zeros() {
    let stats = FrameStats::new(&[], 60.0);
    assert_eq!(stats.frames, 0);
    assert_eq!(stats.dropped, 0);
    assert_eq!(stats.max, 0.0);
}

#[test]
fn frames_over_one_and_a_half_intervals_count_as_dropped() {
    let mut frames = vec![1.0 / 60.0; 98];
    frames.push(0.024); // under 25 ms: not dropped at 60 Hz
    frames.push(0.050);
    let stats = FrameStats::new(&frames, 60.0);
    assert_eq!(stats.frames, 100);
    assert_eq!(stats.dropped, 1);
    assert!((stats.p50 - 16.67).abs() < 0.01);
    assert_eq!(stats.max, 50.0);

    let at_120 = FrameStats::new(&frames, 120.0);
    assert_eq!(at_120.dropped, 100, "every 60 Hz frame is late at 120 Hz");
}
