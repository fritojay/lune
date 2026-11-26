//! Consecutive tracks with the same rate and channel count share one
//! continuous resampler stream, so the join between them is exactly as if
//! they were one file. When the format changes, the old stream is flushed so
//! its output length is exact (input frames x ratio, rounded) and a new
//! stream starts. Each stream strips the resampler's startup delay.

use std::panic::{AssertUnwindSafe, catch_unwind};

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Indexing, Resampler};

use crate::units::SUPPORTED_RATES;
use crate::{EngineError, Result};

const CHUNK_FRAMES: usize = 1024;
/// Track starts remembered per stream, for [`Converter::finish_at`]. Only
/// tracks still buffered ahead of the playing one can be cut, so a few suffice.
const MAX_STARTS: usize = 16;
/// A stream at this input rate panics on creation (see `Stream::new`).
#[cfg(test)]
pub(crate) const PANIC_TEST_RATE: u32 = 12_345;

pub struct Converter {
    out_rate: u32,
    out_channels: usize,
    stream: Option<Stream>,
    /// Output frames emitted by finished streams.
    finished_frames: u64,
}

struct Stream {
    in_rate: u32,
    in_channels: usize,
    out_rate: u32,
    /// `None` when the rates match (channel mapping only).
    resampler: Option<Fft<f32>>,
    /// Channel-mapped input waiting for a full resampler chunk.
    pending: Vec<f32>,
    out_scratch: Vec<f32>,
    input_frames: u64,
    /// Output frames emitted by this stream (after the startup delay).
    output_frames: u64,
    delay_left: usize,
    /// Where recent tracks start: (output index over the converter's
    /// lifetime, this stream's input frame count at that point).
    starts: Vec<(u64, u64)>,
}

impl Converter {
    pub fn new(out_rate: u32, out_channels: usize) -> Self {
        Self {
            out_rate,
            out_channels,
            stream: None,
            finished_frames: 0,
        }
    }

    /// Prepares for input with this format: if it differs from the current
    /// stream, flushes the current stream into `out` and starts a new one.
    /// Returns the output frame index (counted over this converter's lifetime)
    /// at which the next input frame will appear.
    pub fn prepare(&mut self, in_rate: u32, in_channels: usize, out: &mut Vec<f32>) -> Result<u64> {
        self.guarded(|c| c.prepare_inner(in_rate, in_channels, out))
    }

    fn prepare_inner(
        &mut self,
        in_rate: u32,
        in_channels: usize,
        out: &mut Vec<f32>,
    ) -> Result<u64> {
        if !SUPPORTED_RATES.contains(&in_rate) || in_channels == 0 {
            return Err(EngineError::UnsupportedFormat {
                rate: in_rate,
                channels: in_channels,
            });
        }
        let same = self
            .stream
            .as_ref()
            .is_some_and(|s| s.in_rate == in_rate && s.in_channels == in_channels);
        if !same {
            self.finish_inner(out)?;
            self.stream = Some(Stream::new(
                in_rate,
                in_channels,
                self.out_rate,
                self.out_channels,
            )?);
        }
        Ok(self.next_output_index())
    }

    /// Notes that a track starts at `index`, as [`Converter::prepare`] gave
    /// it, so [`Converter::finish_at`] can cut there.
    pub fn track_starts_at(&mut self, index: u64) {
        if let Some(stream) = self.stream.as_mut() {
            if stream.starts.len() >= MAX_STARTS {
                stream.starts.remove(0);
            }
            stream.starts.push((index, stream.input_frames));
        }
    }

    /// Used when a buffered next track is dropped: the end of the track before
    /// it can still be waiting in the resampler (unprocessed input, or the
    /// filter delay). The later track's unprocessed input is thrown away and the
    /// stream is flushed to its exact length. Output already computed with the
    /// later track's samples as lookahead is kept: it is what a seamless join
    /// would have played. The converter must be replaced afterwards.
    pub fn finish_at(&mut self, target: u64, out: &mut Vec<f32>) -> Result<()> {
        self.guarded(|c| c.finish_at_inner(target, out))
    }

    fn finish_at_inner(&mut self, target: u64, out: &mut Vec<f32>) -> Result<()> {
        let out_channels = self.out_channels;
        let finished = self.finished_frames;
        let Some(stream) = self.stream.as_mut() else {
            return Ok(());
        };
        if stream.resampler.is_none() || finished + stream.output_frames >= target {
            return Ok(()); // nothing held back for the earlier track
        }
        let Some(&(_, input_end)) = stream.starts.iter().find(|(index, _)| *index == target) else {
            tracing::debug!(target, "no track start at the cut; its tail may be lost");
            return Ok(());
        };
        let held = (stream.pending.len() / out_channels) as u64;
        let processed = stream.input_frames - held;
        let keep = usize::try_from(input_end.saturating_sub(processed)).unwrap_or(usize::MAX);
        stream.pending.truncate(keep * out_channels);
        stream.input_frames = input_end;
        self.finish_inner(out)
    }

    fn next_output_index(&self) -> u64 {
        match &self.stream {
            Some(s) => self.finished_frames + s.expected_output(self.out_rate),
            None => self.finished_frames,
        }
    }

    pub fn process(&mut self, samples: &[f32], gain: f32, out: &mut Vec<f32>) -> Result<()> {
        self.guarded(|c| c.process_inner(samples, gain, out))
    }

    fn process_inner(&mut self, samples: &[f32], gain: f32, out: &mut Vec<f32>) -> Result<()> {
        let Some(stream) = self.stream.as_mut() else {
            return Ok(());
        };
        let out_channels = self.out_channels;
        if stream.resampler.is_none() {
            map_channels(samples, stream.in_channels, out_channels, gain, out);
            let frames = (samples.len() / stream.in_channels) as u64;
            stream.input_frames += frames;
            stream.output_frames += frames;
            return Ok(());
        }
        map_channels(
            samples,
            stream.in_channels,
            out_channels,
            gain,
            &mut stream.pending,
        );
        stream.input_frames += (samples.len() / stream.in_channels) as u64;
        stream.drain(out_channels, false, out)
    }

    /// Flushes the current stream so its total output matches its input
    /// exactly, and ends it.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> Result<()> {
        self.guarded(|c| c.finish_inner(out))
    }

    fn finish_inner(&mut self, out: &mut Vec<f32>) -> Result<()> {
        if let Some(mut stream) = self.stream.take() {
            if stream.resampler.is_some() {
                stream.drain(self.out_channels, true, out)?;
                let wanted = stream.expected_output(self.out_rate);
                let extra = stream.output_frames.saturating_sub(wanted) as usize;
                out.truncate(out.len().saturating_sub(extra * self.out_channels));
                stream.output_frames = stream.output_frames.min(wanted);
            }
            self.finished_frames += stream.output_frames;
        }
        Ok(())
    }

    /// Turns a panic inside rubato into an error, so it can't kill the
    /// decoder thread. The stream's state is unknown afterwards, so it is
    /// dropped; the next input starts a new one.
    fn guarded<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        match catch_unwind(AssertUnwindSafe(|| f(self))) {
            Ok(result) => result,
            Err(_) => {
                self.stream = None;
                Err(EngineError::ResamplerPanic)
            }
        }
    }
}

impl Stream {
    fn new(in_rate: u32, in_channels: usize, out_rate: u32, out_channels: usize) -> Result<Self> {
        #[cfg(test)]
        if in_rate == PANIC_TEST_RATE {
            panic!("test hook: simulated resampler panic");
        }
        let resampler = if in_rate == out_rate {
            None
        } else {
            Some(
                Fft::<f32>::new(
                    in_rate as usize,
                    out_rate as usize,
                    CHUNK_FRAMES,
                    out_channels,
                    FixedSync::Input,
                )
                .map_err(EngineError::resample)?,
            )
        };
        let (delay_left, out_max) = resampler
            .as_ref()
            .map_or((0, 0), |r| (r.output_delay(), r.output_frames_max()));
        Ok(Self {
            in_rate,
            in_channels: in_channels.max(1),
            out_rate,
            resampler,
            pending: Vec::new(),
            out_scratch: vec![0.0; out_max * out_channels],
            input_frames: 0,
            output_frames: 0,
            delay_left,
            starts: Vec::new(),
        })
    }

    fn expected_output(&self, out_rate: u32) -> u64 {
        let num = u128::from(self.input_frames) * u128::from(out_rate);
        let den = u128::from(self.in_rate);
        ((num + den / 2) / den) as u64
    }

    /// Runs full chunks through the resampler. With `flush`, also runs the
    /// partial remainder and then silence until the stream has produced its
    /// exact length (the caller trims any overshoot).
    fn drain(&mut self, channels: usize, flush: bool, out: &mut Vec<f32>) -> Result<()> {
        let wanted = self.expected_output(self.out_rate);
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(());
        };
        let mut consumed = 0usize;
        // Bound the silence fed while flushing: the startup delay plus a chunk.
        let mut flush_rounds = 0usize;
        loop {
            let available = self.pending.len() / channels - consumed;
            let needed = resampler.input_frames_next();
            let partial = if available >= needed {
                None
            } else if flush && self.output_frames < wanted && flush_rounds < 64 {
                flush_rounds += 1;
                Some(available)
            } else {
                break;
            };
            let out_capacity = self.out_scratch.len() / channels;
            let input = &self.pending[consumed * channels..];
            let in_adapter = InterleavedSlice::new(input, channels, input.len() / channels)
                .map_err(EngineError::resample)?;
            let mut out_adapter =
                InterleavedSlice::new_mut(&mut self.out_scratch, channels, out_capacity)
                    .map_err(EngineError::resample)?;
            let indexing = Indexing {
                input_offset: 0,
                output_offset: 0,
                partial_len: partial,
                active_channels_mask: None,
            };
            let (used, written) = resampler
                .process_into_buffer(&in_adapter, &mut out_adapter, Some(&indexing))
                .map_err(EngineError::resample)?;
            consumed += used.min(available);

            let drop = self.delay_left.min(written);
            self.delay_left -= drop;
            out.extend_from_slice(&self.out_scratch[drop * channels..written * channels]);
            self.output_frames += (written - drop) as u64;
        }
        self.pending.drain(..consumed * channels);
        Ok(())
    }
}

/// - Stereo to mono: the average of both channels.
/// - 5.1 and 7.1 to stereo or mono: ITU-R BS.775 downmix (see [`downmix`]).
/// - Otherwise: mono is copied to every output channel; extra input channels
///   beyond the output count are dropped; missing ones are silent.
fn map_channels(samples: &[f32], in_ch: usize, out_ch: usize, gain: f32, out: &mut Vec<f32>) {
    let in_ch = in_ch.max(1);
    out.reserve(samples.len() / in_ch * out_ch);
    for frame in samples.chunks_exact(in_ch) {
        match (in_ch, out_ch) {
            (2, 1) => out.push((frame[0] + frame[1]) * 0.5 * gain),
            (6 | 8, 1) => {
                let (l, r) = downmix(frame);
                out.push((l + r) * 0.5 * gain);
            }
            (6 | 8, 2) => {
                let (l, r) = downmix(frame);
                out.extend_from_slice(&[l * gain, r * gain]);
            }
            _ => {
                for c in 0..out_ch {
                    let s = if in_ch == 1 {
                        frame[0]
                    } else {
                        frame.get(c).copied().unwrap_or(0.0)
                    };
                    out.push(s * gain);
                }
            }
        }
    }
}

/// Downmixes one 5.1 or 7.1 frame to stereo (ITU-R BS.775): center and each
/// surround pair are added at -3 dB, LFE is dropped. The result is scaled so
/// that full-scale input on every channel can't clip.
///
/// Channel order is Symphonia's (WAVE order): FL, FR, FC, LFE, then the
/// surround pairs (5.1: back or side; 7.1: back, then side).
fn downmix(frame: &[f32]) -> (f32, f32) {
    const MINUS_3DB: f32 = std::f32::consts::FRAC_1_SQRT_2;
    let mut l = frame[0] + MINUS_3DB * frame[2];
    let mut r = frame[1] + MINUS_3DB * frame[2];
    let surrounds = frame.get(4..).unwrap_or_default();
    for [sl, sr] in surrounds.as_chunks::<2>().0 {
        l += MINUS_3DB * sl;
        r += MINUS_3DB * sr;
    }
    let pairs = (surrounds.len() / 2) as f32;
    let norm = 1.0 / (1.0 + MINUS_3DB * (1.0 + pairs));
    (l * norm, r * norm)
}

#[cfg(test)]
#[path = "tests/convert_test.rs"]
mod tests;
