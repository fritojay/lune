//! Gapless spike: how well does Symphonia trim encoder delay and padding?
//!
//! Decodes `<dir>/seg<i>.<suffix>`: consecutive segments of one continuous
//! 44.1 kHz chirp (200 Hz to 2 kHz, amplitude 0.5, over the total length),
//! each encoded with the encoder under test. For each segment, reports:
//! - `lead`: extra frames at the start (untrimmed encoder delay; 0 = perfect)
//! - `tail`: extra frames at the end (untrimmed padding; 0 = perfect)
//! - `err`: max abs error vs. the reference after alignment
//!
//! usage: gapless_spike <dir> <suffix> <len0> <len1> ...
//!   e.g. gapless_spike spike lame.mp3 54321 61234 49999 70001

use std::error::Error;
use std::f64::consts::PI;
use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

const SR: f64 = 44100.0;
const F0: f64 = 200.0;
const F1: f64 = 2000.0;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, suffix, lens @ ..] = args.as_slice() else {
        return Err("usage: gapless_spike <dir> <suffix> <len>...".into());
    };
    let lens: Vec<usize> = lens.iter().map(|l| l.parse()).collect::<Result<_, _>>()?;
    let total: usize = lens.iter().sum();
    let reference = |n: i64| {
        let t = n as f64 / SR;
        let dur = total as f64 / SR;
        0.5 * (2.0 * PI * (F0 * t + (F1 - F0) * t * t / (2.0 * dur))).sin()
    };

    let mut start = 0usize;
    for (i, &len) in lens.iter().enumerate() {
        let path = Path::new(dir).join(format!("seg{i}.{suffix}"));
        let decoded = decode_channel0(&path)?;
        let lead = best_lag(&decoded, start, &reference);
        let tail = decoded.len() as i64 - len as i64 - lead;
        // Error over the overlap once aligned, skipping nothing: this is
        // what the listener would hear at the joins if lead/tail were 0.
        let mut max_err = 0.0f64;
        for (n, &s) in decoded.iter().enumerate() {
            let k = n as i64 - lead;
            if k >= 0 && (k as usize) < len {
                max_err = max_err.max((f64::from(s) - reference(start as i64 + k)).abs());
            }
        }
        println!(
            "{suffix:>10} seg{i}: decoded {:>6} expected {len:>6}  lead {lead:>5}  tail {tail:>5}  err {max_err:.4}",
            decoded.len()
        );
        start += len;
    }
    Ok(())
}

/// Lag (decoded index - expected index) that best aligns the segment with the
/// reference, searched within +-4096 frames over a window in the middle.
fn best_lag(decoded: &[f32], start: usize, reference: &impl Fn(i64) -> f64) -> i64 {
    let window = 8192.min(decoded.len() / 2);
    let mid = (decoded.len() / 2 - window / 2) as i64;
    (-4096i64..=4096)
        .map(|lag| {
            let err: f64 = (0..window as i64)
                .map(|n| {
                    let d = f64::from(decoded[(mid + n) as usize]);
                    let r = reference(start as i64 + mid + n - lag);
                    (d - r) * (d - r)
                })
                .sum();
            (lag, err)
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(0, |(lag, _)| lag)
}

fn decode_channel0(path: &Path) -> Result<Vec<f32>, Box<dyn Error>> {
    let mss = MediaSourceStream::new(Box::new(File::open(path)?), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe().probe(
        &hint,
        mss,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or("no audio track")?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("no audio codec params")?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default().gapless(true))?;
    let track_id = track.id;

    let mut out = Vec::new();
    let mut buf: Vec<f32> = Vec::new();
    while let Some(packet) = format.next_packet()? {
        if packet.track_id != track_id {
            continue;
        }
        let audio = decoder.decode(&packet)?;
        let channels = audio.spec().channels().count();
        buf.resize(audio.samples_interleaved(), 0.0);
        audio.copy_to_slice_interleaved(&mut buf);
        out.extend(buf.iter().step_by(channels.max(1)));
    }
    Ok(out)
}
