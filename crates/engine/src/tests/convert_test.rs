use proptest::prelude::*;

use super::*;

#[test]
fn a_zero_rate_or_channel_count_is_an_error_not_a_panic() {
    let mut c = Converter::new(48_000, 2);
    let mut out = Vec::new();
    assert!(c.prepare(0, 2, &mut out).is_err());
    assert!(c.prepare(44_100, 0, &mut out).is_err());
    assert!(c.prepare(44_100, 2, &mut out).is_ok());
}

#[test]
fn a_rate_outside_the_supported_range_is_an_error_not_an_allocation() {
    let mut c = Converter::new(48_000, 2);
    let mut out = Vec::new();
    // A large prime would size the resampler's buffers in the billions.
    for rate in [999, 768_001, 4_294_967_291] {
        assert!(matches!(
            c.prepare(rate, 2, &mut out),
            Err(EngineError::UnsupportedFormat { .. })
        ));
    }
    assert!(c.prepare(768_000, 2, &mut out).is_ok());
    assert!(c.prepare(1_000, 2, &mut out).is_ok());
}

#[test]
fn a_resampler_panic_is_an_error_and_the_converter_keeps_working() {
    let mut c = Converter::new(48_000, 2);
    let mut out = Vec::new();
    assert!(matches!(
        c.prepare(PANIC_TEST_RATE, 2, &mut out),
        Err(EngineError::ResamplerPanic)
    ));
    c.prepare(44_100, 2, &mut out).unwrap();
    c.process(&vec![0.5; 2 * 44_100], 1.0, &mut out).unwrap();
    c.finish(&mut out).unwrap();
    assert_eq!(out.len(), 2 * 48_000);
}

fn map(samples: &[f32], in_ch: usize, out_ch: usize) -> Vec<f32> {
    let mut out = Vec::new();
    map_channels(samples, in_ch, out_ch, 1.0, &mut out);
    out
}

#[test]
fn stereo_to_mono_averages_and_mono_to_stereo_copies() {
    assert_eq!(map(&[1.0, 0.0, 0.5, 0.5], 2, 1), [0.5, 0.5]);
    assert_eq!(map(&[0.25, -1.0], 1, 2), [0.25, 0.25, -1.0, -1.0]);
}

#[test]
fn surround_downmix_keeps_center_and_surrounds_and_drops_lfe() {
    let only = |ch: usize, n: usize| {
        let mut f = vec![0.0; n];
        f[ch] = 1.0;
        map(&f, n, 2)
    };
    for n in [6, 8] {
        let [cl, cr] = only(2, n)[..] else {
            unreachable!()
        };
        assert!(
            cl > 0.0 && (cl - cr).abs() < 1e-6,
            "center goes to both sides equally"
        );
        assert_eq!(only(3, n), [0.0, 0.0], "LFE is dropped");
        let [sl, sr] = only(4, n)[..] else {
            unreachable!()
        };
        assert!(sl > 0.0 && sr == 0.0, "left surround stays left");
        let all = map(&vec![1.0; n], n, 2);
        assert!(
            all.iter().all(|s| *s <= 1.0 + 1e-6),
            "full scale can't clip: {all:?}"
        );
        assert_eq!(map(&vec![1.0; n], n, 1).len(), 1);
    }
}

#[test]
fn other_layouts_keep_the_first_channels() {
    assert_eq!(map(&[1.0, 2.0, 3.0, 4.0], 4, 2), [1.0, 2.0]);
    assert_eq!(map(&[1.0, 2.0], 2, 3), [1.0, 2.0, 0.0]);
}

proptest! {
    #[test]
    fn mapped_audio_has_whole_frames_scales_by_the_gain_and_never_clips(
        in_ch in 1..=8usize,
        out_ch in 1..=8usize,
        frames in prop::collection::vec(prop::collection::vec(-1.0f32..=1.0, 8), 0..4),
        gain in 0.0f32..=1.0,
    ) {
        let samples: Vec<f32> = frames.iter().flat_map(|f| f[..in_ch].to_vec()).collect();
        let unscaled = map(&samples, in_ch, out_ch);
        prop_assert_eq!(unscaled.len(), frames.len() * out_ch);
        prop_assert!(unscaled.iter().all(|s| s.abs() <= 1.0 + 1e-6), "{unscaled:?}");
        let mut scaled = Vec::new();
        map_channels(&samples, in_ch, out_ch, gain, &mut scaled);
        let expected: Vec<f32> = unscaled.iter().map(|s| s * gain).collect();
        prop_assert_eq!(scaled, expected);
        if in_ch == out_ch {
            prop_assert_eq!(unscaled, samples);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Dropping a buffered next track keeps the whole of the track before
    /// it, however many pieces of the dropped one were already prepared.
    /// Output already made past the cut stays (see `finish_at`).
    #[test]
    fn cutting_at_a_track_start_keeps_exactly_the_track_before_it(
        a_frames in 1usize..6_000,
        b_pieces in 0usize..40,
        piece_frames in 1usize..20,
    ) {
        let mut c = Converter::new(48_000, 2);
        let mut out = Vec::new();
        let a_start = c.prepare(44_100, 2, &mut out).unwrap();
        c.track_starts_at(a_start);
        c.process(&vec![0.25; 2 * a_frames], 1.0, &mut out).unwrap();
        let b_start = c.prepare(44_100, 2, &mut out).unwrap();
        c.track_starts_at(b_start);
        c.process(&vec![0.25; 2 * piece_frames], 1.0, &mut out).unwrap();
        for _ in 0..b_pieces {
            c.prepare(44_100, 2, &mut out).unwrap();
            c.process(&vec![0.25; 2 * piece_frames], 1.0, &mut out).unwrap();
        }
        let made = (out.len() / 2) as u64;
        c.finish_at(b_start, &mut out).unwrap();
        prop_assert_eq!((out.len() / 2) as u64, made.max(b_start));
    }
}
