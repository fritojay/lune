//! Frame/time conversions in integer math. Rates come from untrusted files,
//! so these never divide by zero or panic (unlike `Duration::from_secs_f64`).

use std::ops::RangeInclusive;
use std::time::Duration;

/// Input sample rates the engine plays. Rates come from untrusted files, and
/// the resampler sizes its buffers by the rate (up to the rate itself when it
/// shares no factor with the output rate), so an absurd rate could exhaust
/// memory, which aborts the process instead of failing the track.
pub const SUPPORTED_RATES: RangeInclusive<u32> = 1_000..=768_000;

/// `frames` at `rate` Hz as a duration. A zero rate gives zero.
pub fn frames_to_duration(frames: u64, rate: u32) -> Duration {
    if rate == 0 {
        return Duration::ZERO;
    }
    let nanos = u128::from(frames) * 1_000_000_000 / u128::from(rate);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

pub fn duration_to_frames(duration: Duration, rate: u32) -> u64 {
    let frames = (duration.as_nanos() * u128::from(rate) + 500_000_000) / 1_000_000_000;
    u64::try_from(frames).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "tests/units_test.rs"]
mod tests;
