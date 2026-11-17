use super::*;

#[test]
fn conversions_round_trip_and_tolerate_zero_rate() {
    assert_eq!(frames_to_duration(48_000, 48_000), Duration::from_secs(1));
    assert_eq!(frames_to_duration(123, 0), Duration::ZERO);
    assert_eq!(
        frames_to_duration(u64::MAX, 1),
        Duration::from_nanos(u64::MAX)
    );
    assert_eq!(
        duration_to_frames(Duration::from_millis(500), 44_100),
        22_050
    );
    assert_eq!(duration_to_frames(Duration::MAX, u32::MAX), u64::MAX);
    for frames in [0, 1, 999, 44_100, 1 << 40] {
        let d = frames_to_duration(frames, 44_100);
        assert_eq!(duration_to_frames(d, 44_100), frames);
    }
}
