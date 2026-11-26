//! Each output runs on its own thread, which owns the cpal stream (cpal
//! streams are not `Send` on every platform) and drives one [`Renderer`].
//!
//! A device stream is paused while the engine is idle, so the OS doesn't
//! show the app as playing sound (e.g. KDE's taskbar speaker icon) and the
//! device can sleep.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample};

use crate::render::{Renderer, Shared};
use crate::units::frames_to_duration;
use crate::{EngineError, Result};

/// Largest callback buffer converted in one pass for non-f32 devices.
const SCRATCH_SAMPLES: usize = 1 << 16;
const CAPTURE_BLOCK_FRAMES: usize = 1024;
const CAPTURE_RATES: std::ops::RangeInclusive<u32> = 8_000..=384_000;
const CAPTURE_CHANNELS: std::ops::RangeInclusive<usize> = 1..=32;
const IDLE_CHECK: Duration = Duration::from_millis(250);
/// Long enough that a quick pause and resume doesn't put the stream to sleep.
const SLEEP_AFTER: Duration = Duration::from_secs(2);
/// A restart can fail for a moment, e.g. while the system resumes from
/// suspend, so only this many failures in a row fail the output.
const RESTART_ATTEMPTS: u32 = 3;
/// How often an output opened in place of its preferred device checks
/// whether that one is connected again.
const PREFERRED_CHECK: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub enum OutputKind {
    /// `preferred` is an [`OutputDevice::id`]. Without one, or while it isn't
    /// connected, the system default plays.
    Device { preferred: Option<String> },
    /// The file must not exist yet; an existing file is never overwritten.
    Capture {
        path: Option<PathBuf>,
        rate: u32,
        channels: usize,
        /// Pace rendering in real time (like a device). When false, renders as
        /// fast as the decoder can supply audio, without counting underruns.
        realtime: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputDevice {
    /// Stable across runs, so it can be saved.
    pub id: String,
    pub name: String,
}

/// The connected output devices. Slow on some systems (it asks the sound
/// server), so call it when needed rather than often.
pub fn output_devices() -> Vec<OutputDevice> {
    match cpal::default_host().output_devices() {
        Ok(devices) => devices
            .filter_map(|device| {
                Some(OutputDevice {
                    id: device.id().ok()?.to_string(),
                    name: device.to_string(),
                })
            })
            .collect(),
        Err(err) => {
            tracing::warn!(%err, "could not list the audio outputs");
            Vec::new()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSpec {
    pub rate: u32,
    pub channels: usize,
}

/// A running output thread. Dropping it stops the output.
pub struct Output {
    signals: Sender<Signal>,
    thread: Option<JoinHandle<()>>,
}

pub enum Signal {
    Wake,
    Stop,
}

impl Output {
    /// Restarts a stream paused while idle. Call before anything that needs
    /// the callback to run.
    pub fn wake(&self) {
        let _ = self.signals.send(Signal::Wake);
    }

    pub fn waker(&self) -> Waker {
        Waker(self.signals.clone())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        // An explicit stop, since a `Waker` may still hold the channel open.
        let _ = self.signals.send(Signal::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// [`Output::wake`] for threads that don't own the output.
#[derive(Clone)]
pub struct Waker(Sender<Signal>);

impl Waker {
    pub fn wake(&self) {
        let _ = self.0.send(Signal::Wake);
    }

    /// One tied to no output, and the receiving end of its wakes.
    #[cfg(test)]
    pub fn detached() -> (Self, Receiver<Signal>) {
        let (tx, rx) = mpsc::channel();
        (Self(tx), rx)
    }
}

/// Starts the output. `make_renderer` is called on the output thread once the
/// output format is known.
pub fn start(
    kind: OutputKind,
    shared: Arc<Shared>,
    make_renderer: impl FnOnce(OutputSpec) -> Renderer + Send + 'static,
) -> Result<(Output, OutputSpec)> {
    if let OutputKind::Capture { rate, channels, .. } = &kind {
        if !CAPTURE_RATES.contains(rate) {
            return Err(EngineError::OutputConfig(format!(
                "capture rate {rate} Hz is outside {CAPTURE_RATES:?}"
            )));
        }
        if !CAPTURE_CHANNELS.contains(channels) {
            return Err(EngineError::OutputConfig(format!(
                "capture channel count {channels} is outside {CAPTURE_CHANNELS:?}"
            )));
        }
    }
    let (signals_tx, signals_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<OutputSpec>>(1);
    let thread = std::thread::Builder::new()
        .name("lune-output".into())
        .spawn(move || match kind {
            OutputKind::Device { preferred } => run_device(
                make_renderer,
                preferred.as_deref(),
                &shared,
                &ready_tx,
                &signals_rx,
            ),
            OutputKind::Capture {
                path,
                rate,
                channels,
                realtime,
            } => run_capture(
                make_renderer,
                OutputSpec { rate, channels },
                path,
                realtime,
                &shared,
                &ready_tx,
                &signals_rx,
            ),
        })?;
    let spec = ready_rx.recv().map_err(|_| EngineError::EngineStopped)??;
    Ok((
        Output {
            signals: signals_tx,
            thread: Some(thread),
        },
        spec,
    ))
}

fn run_device(
    make_renderer: impl FnOnce(OutputSpec) -> Renderer,
    preferred: Option<&str>,
    shared: &Shared,
    ready: &mpsc::SyncSender<Result<OutputSpec>>,
    signals: &Receiver<Signal>,
) {
    let host = cpal::default_host();
    let find = |id: &str| {
        let id = id.parse::<cpal::DeviceId>().ok()?;
        host.device_by_id(&id)
    };
    let (stream, fell_back) = match choose_device(preferred, find, || host.default_output_device())
        .ok_or(EngineError::NoOutputDevice)
        .and_then(|(device, fell_back)| {
            build_device_stream(&device, make_renderer).map(|built| (built, fell_back))
        }) {
        Ok(((stream, spec), fell_back)) => {
            let _ = ready.send(Ok(spec));
            (stream, fell_back)
        }
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    let mut sleeper = Sleeper::default();
    let mut checked = Instant::now();
    loop {
        match signals.recv_timeout(IDLE_CHECK) {
            Ok(Signal::Wake) => sleeper.wake(&stream, shared),
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                sleeper.check(&stream, shared, now);
                if let Some(preferred) = preferred.filter(|_| fell_back)
                    && now.duration_since(checked) >= PREFERRED_CHECK
                    && !shared.preferred_back.load(Ordering::Relaxed)
                {
                    checked = now;
                    if find(preferred).is_some() {
                        tracing::info!(preferred, "the preferred audio output is connected again");
                        shared.preferred_back.store(true, Ordering::Release);
                    }
                }
            }
            Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stream);
}

/// The part of a stream [`Sleeper`] drives, so tests can stand in for a
/// device.
trait Pausable {
    fn resume(&self) -> std::result::Result<(), cpal::Error>;
    fn suspend(&self) -> std::result::Result<(), cpal::Error>;
}

impl Pausable for cpal::Stream {
    fn resume(&self) -> std::result::Result<(), cpal::Error> {
        self.play()
    }

    fn suspend(&self) -> std::result::Result<(), cpal::Error> {
        self.pause()
    }
}

/// Pauses a stream once the engine has been idle for [`SLEEP_AFTER`], and
/// restarts it on a wake or as soon as the engine is busy again.
#[derive(Debug, Default)]
struct Sleeper {
    idle_since: Option<Instant>,
    asleep: bool,
    cannot_pause: bool,
    restart_failures: u32,
    /// [`RESTART_ATTEMPTS`] restarts failed. Like a failed stream, the output
    /// stays silent until the player is reopened.
    failed: bool,
}

impl Sleeper {
    fn wake(&mut self, stream: &impl Pausable, shared: &Shared) {
        self.idle_since = None;
        self.restart(stream, shared);
    }

    fn check(&mut self, stream: &impl Pausable, shared: &Shared, now: Instant) {
        if !shared.idle() {
            self.idle_since = None;
            // Audio can arrive without a wake, e.g. after a seek whose file
            // took longer than `SLEEP_AFTER` to open.
            self.restart(stream, shared);
            return;
        }
        if self.asleep || self.cannot_pause {
            return;
        }
        let since = *self.idle_since.get_or_insert(now);
        if now.duration_since(since) < SLEEP_AFTER {
            return;
        }
        match stream.suspend() {
            Ok(()) => self.asleep = true,
            Err(err) => {
                self.cannot_pause = true;
                tracing::info!(err = %err, "audio output keeps running while idle");
            }
        }
    }

    fn restart(&mut self, stream: &impl Pausable, shared: &Shared) {
        if !self.asleep || self.failed {
            return;
        }
        match stream.resume() {
            Ok(()) => {
                self.asleep = false;
                self.restart_failures = 0;
            }
            Err(err) if self.restart_failures + 1 < RESTART_ATTEMPTS => {
                self.restart_failures += 1;
                tracing::warn!(err = %err, "could not restart the audio output; will retry");
            }
            Err(err) => {
                self.failed = true;
                shared.output_failed.store(true, Ordering::Release);
                tracing::error!(err = %err, "could not restart the audio output");
            }
        }
    }
}

/// `preferred` if it is connected, else the default, and whether that is in
/// its place.
fn choose_device<D>(
    preferred: Option<&str>,
    find: impl Fn(&str) -> Option<D>,
    default: impl FnOnce() -> Option<D>,
) -> Option<(D, bool)> {
    let Some(preferred) = preferred else {
        return default().map(|device| (device, false));
    };
    match find(preferred) {
        Some(device) => Some((device, false)),
        None => {
            tracing::warn!(
                preferred,
                "the preferred audio output isn't connected; using the default"
            );
            default().map(|device| (device, true))
        }
    }
}

fn build_device_stream(
    device: &cpal::Device,
    make_renderer: impl FnOnce(OutputSpec) -> Renderer,
) -> Result<(cpal::Stream, OutputSpec)> {
    let supported = device
        .default_output_config()
        .map_err(EngineError::output("reading the default output format"))?;
    let config = supported.config();
    let spec = OutputSpec {
        rate: config.sample_rate,
        channels: usize::from(config.channels),
    };
    let renderer = make_renderer(spec);
    tracing::info!(
        device = %device,
        rate = spec.rate,
        channels = spec.channels,
        format = ?supported.sample_format(),
        "opening output device"
    );
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build_stream::<f32>(device, config, renderer),
        SampleFormat::I16 => build_stream::<i16>(device, config, renderer),
        SampleFormat::U16 => build_stream::<u16>(device, config, renderer),
        SampleFormat::I32 => build_stream::<i32>(device, config, renderer),
        SampleFormat::F64 => build_stream::<f64>(device, config, renderer),
        other => Err(EngineError::OutputConfig(format!(
            "unsupported device sample format {other:?}"
        ))),
    }?;
    stream
        .play()
        .map_err(EngineError::output("starting the stream"))?;
    Ok((stream, spec))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut renderer: Renderer,
) -> Result<cpal::Stream>
where
    T: SizedSample + FromSample<f32>,
{
    // Allocated here, before the stream starts, so the callback never allocates.
    let mut scratch = vec![0.0f32; SCRATCH_SAMPLES];
    let channels = usize::from(config.channels).max(1);
    let shared = renderer.shared();
    let callback = move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        // REALTIME: no allocation, locks, I/O, or logging in here.
        let block = (SCRATCH_SAMPLES / channels) * channels;
        for piece in out.chunks_mut(block) {
            let tmp = &mut scratch[..piece.len()];
            let _ = renderer.render(tmp);
            for (d, s) in piece.iter_mut().zip(tmp.iter()) {
                *d = T::from_sample(*s);
            }
        }
    };
    let on_error = move |err: cpal::Error| {
        // Runs on cpal's error path, not inside the data callback.
        match err.kind() {
            // A glitch; macOS reports one now and then under load.
            ErrorKind::Xrun => tracing::debug!(err = %err, "audio output glitch"),
            kind if stream_failed(kind) => {
                shared.output_failed.store(true, Ordering::Release);
                tracing::error!(err = %err, "audio output failed");
            }
            _ => tracing::info!(err = %err, "audio output notice"),
        }
    };
    device
        .build_output_stream::<T, _, _>(config, callback, on_error, None)
        .map_err(EngineError::output("opening the output stream"))
}

/// Whether an error from a running stream means it has stopped producing
/// sound. A glitch (xrun), a route change the stream followed, or a refused
/// realtime priority leave it playing. Kinds added to cpal later count as
/// failures.
fn stream_failed(kind: ErrorKind) -> bool {
    !matches!(
        kind,
        ErrorKind::Xrun | ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied
    )
}

fn run_capture(
    make_renderer: impl FnOnce(OutputSpec) -> Renderer,
    spec: OutputSpec,
    path: Option<PathBuf>,
    realtime: bool,
    shared: &Shared,
    ready: &mpsc::SyncSender<Result<OutputSpec>>,
    signals: &Receiver<Signal>,
) {
    let mut writer = match path.map(|p| WavWriter::create(&p, spec)).transpose() {
        Ok(w) => w,
        Err(err) => {
            let _ = ready.send(Err(err));
            return;
        }
    };
    let mut renderer = make_renderer(spec);
    let _ = ready.send(Ok(spec));

    let mut block = vec![0.0f32; CAPTURE_BLOCK_FRAMES * spec.channels];
    let block_time = frames_to_duration(CAPTURE_BLOCK_FRAMES as u64, spec.rate);
    let started = Instant::now();
    let mut rendered_blocks = 0u32;
    loop {
        let wait = if realtime {
            (started + block_time * rendered_blocks).saturating_duration_since(Instant::now())
        } else if renderer.available_frames() < CAPTURE_BLOCK_FRAMES {
            // As fast as possible, but never ahead of the decoder (and no
            // busy loop while idle).
            Duration::from_millis(1)
        } else {
            Duration::ZERO
        };
        match signals.recv_timeout(wait) {
            // A capture never sleeps; wait out the rest of the block.
            Ok(Signal::Wake) => continue,
            Err(RecvTimeoutError::Timeout) => {}
            Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => break,
        }
        if !realtime
            && renderer.available_frames() < CAPTURE_BLOCK_FRAMES
            && shared.expecting_data.load(Ordering::Acquire)
        {
            continue;
        }
        let frames = renderer.render(&mut block);
        rendered_blocks = rendered_blocks.wrapping_add(1);
        // Realtime mode writes every block, silence included, like a device.
        // Fast mode writes only the frames that came from the ring, so the
        // file holds exactly the audio played (sample-exact lengths).
        let keep = if realtime {
            block.len()
        } else {
            frames * spec.channels
        };
        if let Some(w) = writer.as_mut()
            && keep > 0
            && let Err(err) = w.write(&block[..keep])
        {
            tracing::error!(err = %err, "capture write failed; capture stopped");
            // Finish what was written so the file stays valid.
            if let Some(w) = writer.take()
                && let Err(err) = w.finish()
            {
                tracing::error!(err = %err, "could not finish capture file");
            }
        }
    }
    if let Some(w) = writer
        && let Err(err) = w.finish()
    {
        tracing::error!(err = %err, "could not finish capture file");
    }
}

struct WavWriter {
    file: BufWriter<File>,
    data_bytes: u64,
}

impl WavWriter {
    /// Fails if anything already exists at `path`, so a capture can never
    /// overwrite a music file.
    fn create(path: &std::path::Path, spec: OutputSpec) -> Result<Self> {
        #[expect(
            clippy::disallowed_methods,
            reason = "the capture output is our own file; create_new never overwrites"
        )]
        let file = File::options().write(true).create_new(true).open(path)?;
        let mut file = BufWriter::new(file);
        // `start` checked the range (1..=32), so this never truncates.
        let channels = u16::try_from(spec.channels).unwrap_or(u16::MAX);
        let block_align = channels * 4;
        file.write_all(b"RIFF\0\0\0\0WAVEfmt ")?;
        file.write_all(&16u32.to_le_bytes())?;
        file.write_all(&3u16.to_le_bytes())?; // IEEE float
        file.write_all(&channels.to_le_bytes())?;
        file.write_all(&spec.rate.to_le_bytes())?;
        file.write_all(&(spec.rate * u32::from(block_align)).to_le_bytes())?;
        file.write_all(&block_align.to_le_bytes())?;
        file.write_all(&32u16.to_le_bytes())?;
        file.write_all(b"data\0\0\0\0")?;
        Ok(Self {
            file,
            data_bytes: 0,
        })
    }

    /// Appends samples. Fails, writing nothing, once the data would pass the
    /// 4 GiB limit of WAV's 32-bit size fields; `finish` still works then.
    fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        let bytes = samples.len() as u64 * 4;
        if self.data_bytes + bytes > u64::from(u32::MAX - 36) {
            return Err(std::io::Error::other(
                "capture reached the 4 GiB WAV size limit",
            ));
        }
        for s in samples {
            self.file.write_all(&s.to_le_bytes())?;
        }
        self.data_bytes += bytes;
        Ok(())
    }

    fn finish(mut self) -> std::io::Result<()> {
        let data = u32::try_from(self.data_bytes).unwrap_or(u32::MAX);
        self.file.seek(SeekFrom::Start(4))?;
        self.file
            .write_all(&data.saturating_add(36).to_le_bytes())?;
        self.file.seek(SeekFrom::Start(40))?;
        self.file.write_all(&data.to_le_bytes())?;
        self.file.flush()
    }
}

#[cfg(test)]
#[path = "tests/output_test.rs"]
mod tests;
