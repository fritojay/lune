use std::fs::File;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Duration;

use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

use crate::error::Chain;
use crate::mp4_gapless::{self, Mp4Trim};
use crate::units::{SUPPORTED_RATES, duration_to_frames, frames_to_duration};
use crate::{EngineError, Result};

pub struct Chunk<'a> {
    pub rate: u32,
    pub channels: usize,
    pub samples: &'a [f32],
}

#[derive(Clone, Copy)]
struct Decoded {
    rate: u32,
    channels: usize,
    start: usize,
    stop: usize,
}

pub struct Source {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// Sample rate from the codec parameters (the decoded rate is checked per chunk).
    rate: u32,
    /// Our own trim (AAC in MP4 only; Symphonia trims the other formats).
    trim: Option<Mp4Trim>,
    /// Decoder-timeline frame index of the next frame we will output.
    decoded_pos: u64,
    /// Frames still to drop from upcoming decoder output (priming, seek alignment).
    skip: u64,
    total_frames: Option<u64>,
    buf: Vec<f32>,
}

impl Source {
    /// `open`, `next_chunk`, and `seek` catch panics from Symphonia (a
    /// malformed file must not kill the decoder thread) and report them as
    /// [`EngineError::DecoderPanic`]. After a panic, drop the source.
    pub fn open(path: &Path) -> Result<Self> {
        catch_unwind(|| Self::open_inner(path)).unwrap_or_else(|_| {
            Err(EngineError::DecoderPanic {
                path: path.to_path_buf(),
            })
        })
    }

    fn open_inner(path: &Path) -> Result<Self> {
        #[cfg(test)]
        if path.file_stem().is_some_and(|s| s == "panic-on-open") {
            panic!("test hook: simulated decoder panic");
        }
        let decode_err = |source| EngineError::Decode {
            path: path.to_path_buf(),
            source,
        };
        let file = File::open(path).map_err(|source| EngineError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(decode_err)?;
        let track =
            format
                .default_track(TrackType::Audio)
                .ok_or_else(|| EngineError::NoAudioTrack {
                    path: path.to_path_buf(),
                })?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| EngineError::NoAudioTrack {
                path: path.to_path_buf(),
            })?
            .clone();
        let track_id = track.id;
        let num_frames = track.num_frames;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default().gapless(true))
            .map_err(decode_err)?;
        let rate = params.sample_rate.unwrap_or(44_100);
        if !SUPPORTED_RATES.contains(&rate) {
            return Err(EngineError::InvalidFormat {
                path: path.to_path_buf(),
                reason: "unsupported sample rate",
            });
        }

        // Ask the demuxer, not the file extension: a misnamed file must not
        // get (or miss) the MP4 trim.
        let is_mp4 = format.format_info().short_name == "isomp4";
        let trim = if is_mp4 && params.codec == CODEC_ID_AAC {
            match mp4_gapless::read_trim(path, rate) {
                Ok(trim) => trim,
                Err(err) => {
                    tracing::warn!(err = %Chain(&err), path = %path.display(), "could not read MP4 gapless info");
                    None
                }
            }
        } else {
            None
        };
        if trim.is_none() && params.codec == CODEC_ID_AAC {
            tracing::debug!(path = %path.display(), "AAC without gapless info; not trimmed");
        }

        // The trim comes from tags; never let it claim more audio than the
        // container holds.
        let trim = match (trim, num_frames) {
            (Some(t), Some(n)) if t.delay > n => None,
            (Some(t), Some(n)) => Some(Mp4Trim {
                delay: t.delay,
                valid_frames: t.valid_frames.map(|v| v.min(n - t.delay)),
            }),
            (trim, _) => trim,
        };
        let delay = trim.map_or(0, |t| t.delay);
        let total_frames = match trim {
            Some(t) => t
                .valid_frames
                .or(num_frames.map(|n| n.saturating_sub(delay))),
            None => num_frames,
        };
        Ok(Self {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            rate,
            trim,
            decoded_pos: delay,
            skip: delay,
            total_frames,
            buf: Vec::new(),
        })
    }

    pub fn duration(&self) -> Option<Duration> {
        self.total_frames.map(|n| frames_to_duration(n, self.rate))
    }

    fn delay(&self) -> u64 {
        self.trim.map_or(0, |t| t.delay)
    }

    /// Decoder-timeline frame where our trim ends the track, if any.
    fn end(&self) -> Option<u64> {
        self.trim
            .and_then(|t| t.valid_frames)
            .map(|n| self.delay().saturating_add(n))
    }

    /// Decodes the next packet. Returns `Ok(None)` at the end of the track.
    /// The chunk may be empty (e.g. while skipping priming frames).
    pub fn next_chunk(&mut self) -> Result<Option<Chunk<'_>>> {
        let decoded = match catch_unwind(AssertUnwindSafe(|| self.decode_next())) {
            Ok(result) => result?,
            Err(_) => {
                return Err(EngineError::DecoderPanic {
                    path: self.path.clone(),
                });
            }
        };
        Ok(decoded.map(|d| Chunk {
            rate: d.rate,
            channels: d.channels,
            samples: &self.buf[d.start..d.stop],
        }))
    }

    fn decode_next(&mut self) -> Result<Option<Decoded>> {
        loop {
            if self.end().is_some_and(|end| self.decoded_pos >= end) {
                return Ok(None);
            }
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => return Ok(None),
                // A chained stream (new track inside the file) or a truncated
                // file: treat as the end of this track.
                Err(SymError::ResetRequired) => return Ok(None),
                Err(SymError::IoError(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(None);
                }
                Err(source) => {
                    return Err(EngineError::Decode {
                        path: self.path.clone(),
                        source,
                    });
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let audio = match self.decoder.decode(&packet) {
                Ok(audio) => audio,
                Err(SymError::DecodeError(err)) => {
                    tracing::warn!(err, path = %self.path.display(), "skipping corrupt packet");
                    continue;
                }
                Err(source) => {
                    return Err(EngineError::Decode {
                        path: self.path.clone(),
                        source,
                    });
                }
            };
            let rate = audio.spec().rate();
            if !SUPPORTED_RATES.contains(&rate) {
                return Err(EngineError::InvalidFormat {
                    path: self.path.clone(),
                    reason: "decoded audio has an unsupported sample rate",
                });
            }
            let channels = audio.spec().channels().count().max(1);
            let frames = audio.frames() as u64;
            self.buf.resize(audio.samples_interleaved(), 0.0);
            audio.copy_to_slice_interleaved(&mut self.buf);

            let skipped = self.skip.min(frames);
            self.skip -= skipped;
            let mut kept = frames - skipped;
            if let Some(end) = self.end() {
                kept = kept.min(end.saturating_sub(self.decoded_pos));
            }
            self.decoded_pos += kept;
            let start = skipped as usize * channels;
            let stop = start + kept as usize * channels;
            return Ok(Some(Decoded {
                rate,
                channels,
                start,
                stop,
            }));
        }
    }

    /// Returns the position actually reached.
    pub fn seek(&mut self, position: Duration) -> Result<Duration> {
        catch_unwind(AssertUnwindSafe(|| self.seek_inner(position))).unwrap_or_else(|_| {
            Err(EngineError::DecoderPanic {
                path: self.path.clone(),
            })
        })
    }

    fn seek_inner(&mut self, position: Duration) -> Result<Duration> {
        let rate = u64::from(self.rate);
        let target = duration_to_frames(position, self.rate);
        let target = self.total_frames.map_or(target, |n| target.min(n));
        // Position on the decoder's timeline (which includes our priming frames).
        let decoder_target = target.saturating_add(self.delay());
        let time =
            Time::try_from_secs_f64(decoder_target as f64 / rate as f64).unwrap_or(Time::ZERO);
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|source| EngineError::Decode {
                path: self.path.clone(),
                source,
            })?;
        self.decoder.reset();
        // Frames between where the demuxer landed and where we want to be,
        // converted from the track's time base to frames.
        let ahead_ts = seeked
            .required_ts
            .get()
            .saturating_sub(seeked.actual_ts.get())
            .max(0) as u64;
        let ahead = match self.format.tracks().iter().find(|t| t.id == self.track_id) {
            Some(track) => match track.time_base {
                Some(tb) => {
                    let secs =
                        tb.calc_time(symphonia::core::units::Timestamp::new(ahead_ts as i64));
                    secs.map_or(ahead_ts, |t| (t.as_secs_f64() * rate as f64).round() as u64)
                }
                None => ahead_ts,
            },
            None => ahead_ts,
        };
        self.skip = ahead;
        self.decoded_pos = decoder_target;
        Ok(frames_to_duration(target, self.rate))
    }
}

#[cfg(test)]
#[path = "tests/source_test.rs"]
mod tests;
