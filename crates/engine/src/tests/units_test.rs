use proptest::prelude::*;

use super::*;

#[test]
fn conversions_saturate_and_tolerate_zero_rate() {
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
}

/// A rate, and frames whose duration at it fits in `u64::MAX` nanoseconds
/// (past that, conversions saturate instead).
fn arb_rate_and_frames() -> impl Strategy<Value = (u32, u64)> {
    prop_oneof![Just(44_100u32), SUPPORTED_RATES].prop_flat_map(|rate| {
        let max = u64::try_from(u128::from(u64::MAX) * u128::from(rate) / 1_000_000_000).unwrap();
        (Just(rate), prop_oneof![0..100_000u64, 0..=max, Just(max)])
    })
}

proptest! {
    #[test]
    fn frames_round_trip_through_a_duration_at_any_rate(
        (rate, frames) in arb_rate_and_frames(),
    ) {
        prop_assert_eq!(duration_to_frames(frames_to_duration(frames, rate), rate), frames);
    }

    #[test]
    fn conversions_never_panic_and_keep_order(a: u64, b: u64, rate: u32, nanos: u64) {
        let (lo, hi) = (a.min(b), a.max(b));
        prop_assert!(frames_to_duration(lo, rate) <= frames_to_duration(hi, rate));
        let d = Duration::from_nanos(nanos);
        prop_assert!(duration_to_frames(d, rate) <= duration_to_frames(d * 2, rate));
    }
}
