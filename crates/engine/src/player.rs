//! Every frame pushed into the ring has an index on one shared timeline. The
//! decoder records where each track starts on it ([`crate::timeline`]); the audio
//! callback counts frames consumed. The playing track and its position come
//! from frames consumed, not frames decoded.

use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use rtrb::RingBuffer;

use crate::decoder::{Command, Decoder};
use crate::output::{self, Output, OutputKind, OutputSpec};
use crate::queue::{EntryId, PlayQueue, QueueEntry};
use crate::render::{Renderer, Shared};
use crate::timeline::{Timeline, lock};
use crate::units::frames_to_duration;
use crate::{EngineError, Result};

const DISCARD_CAPACITY: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerConfig {
    pub output: OutputKind,
    /// Audio buffered ahead of the device. Larger survives longer stalls,
    /// such as a sleeping disk waking to open the next track; it does not
    /// delay pause, seek, or skip (stale audio is discarded).
    pub buffer: Duration,
    /// Off, the next track is opened only after the current one has played
    /// out, as players that don't do gapless playback do.
    pub gapless: bool,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            output: OutputKind::Device { preferred: None },
            buffer: Duration::from_secs(5),
            gapless: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub state: PlaybackState,
    /// The entry whose audio is being played (not decoded).
    pub entry: Option<EntryId>,
    /// Goes up each time playback of an entry starts, not when it seeks, so
    /// an entry played again (repeated) is told apart from its last
    /// playthrough. 0 while stopped.
    pub playthrough: u64,
    pub position: Duration,
    pub duration: Option<Duration>,
    /// Callbacks that ran out of audio while more was expected.
    pub underruns: u64,
    /// The output device reported an error (e.g. it was unplugged) and
    /// playback is silent.
    pub output_failed: bool,
    /// The output plays in place of the preferred device, which is
    /// connected again: reopen the player to switch back.
    pub preferred_back: bool,
    pub output_rate: u32,
    pub output_channels: usize,
}

/// Handle to the audio engine. Dropping it stops playback and joins the threads.
pub struct Player {
    commands: Sender<Command>,
    shared: Arc<Shared>,
    timeline: Arc<Mutex<Timeline>>,
    spec: OutputSpec,
    decoder: Option<JoinHandle<()>>,
    output: Option<Output>,
}

impl Player {
    pub fn new(config: PlayerConfig, queue: Arc<dyn PlayQueue>) -> Result<Self> {
        let shared = Arc::new(Shared::new());
        let (rings_tx, rings_rx) = mpsc::channel();
        let buffer = config.buffer;
        let renderer_shared = Arc::clone(&shared);
        let (output, spec) = output::start(config.output, Arc::clone(&shared), move |spec| {
            let frames = (buffer.as_secs_f64() * f64::from(spec.rate)) as usize;
            let (samples_tx, samples_rx) = RingBuffer::new(frames.max(4096) * spec.channels);
            let (discards_tx, discards_rx) = RingBuffer::new(DISCARD_CAPACITY);
            let _ = rings_tx.send((samples_tx, discards_tx));
            Renderer::new(
                samples_rx,
                discards_rx,
                spec.channels,
                Arc::clone(&renderer_shared),
            )
        })?;
        // The rings of the renderer that plays: the last one made.
        let (producer, discards) = rings_rx
            .try_iter()
            .last()
            .ok_or(EngineError::EngineStopped)?;

        let timeline = Arc::new(Mutex::new(Timeline::new()));
        let (commands, command_rx) = mpsc::channel();
        let decoder = Decoder::new(
            queue,
            Arc::clone(&shared),
            Arc::clone(&timeline),
            spec,
            producer,
            discards,
            command_rx,
            output.waker(),
            config.gapless,
        );
        let decoder = std::thread::Builder::new()
            .name("lune-decoder".into())
            .spawn(move || decoder.run())?;
        Ok(Self {
            commands,
            shared,
            timeline,
            spec,
            decoder: Some(decoder),
            output: Some(output),
        })
    }

    fn send(&self, command: Command) -> Result<()> {
        self.wake_output();
        self.commands
            .send(command)
            .map_err(|_| EngineError::EngineStopped)
    }

    pub fn play(&self, entry: QueueEntry) -> Result<()> {
        self.play_from(entry, Duration::ZERO)
    }

    pub fn play_from(&self, entry: QueueEntry, position: Duration) -> Result<()> {
        self.shared.starting.store(true, Ordering::Release);
        self.send(Command::Play(entry, position))
    }

    pub fn pause(&self) {
        if self.status().state == PlaybackState::Playing {
            self.shared.paused.store(true, Ordering::Relaxed);
        }
    }

    pub fn resume(&self) {
        self.wake_output();
        self.shared.paused.store(false, Ordering::Relaxed);
        // Only fails once the decoder has stopped.
        let _ = self.commands.send(Command::Resumed);
    }

    fn wake_output(&self) {
        if let Some(output) = &self.output {
            output.wake();
        }
    }

    pub fn toggle_pause(&self) {
        match self.status().state {
            PlaybackState::Playing => self.pause(),
            PlaybackState::Paused => self.resume(),
            PlaybackState::Stopped => {}
        }
    }

    pub fn stop(&self) -> Result<()> {
        self.send(Command::Stop)
    }

    pub fn seek(&self, position: Duration) -> Result<()> {
        self.send(Command::Seek(position))
    }

    pub fn next(&self) -> Result<()> {
        self.send(Command::Next)
    }

    /// Restarts the current track if more than a few seconds have played,
    /// otherwise goes to the previous entry.
    pub fn previous(&self) -> Result<()> {
        self.send(Command::Previous)
    }

    /// Call after any queue edit. If the pre-buffered next track is no longer
    /// next, it is dropped and the new next track is decoded instead.
    pub fn queue_changed(&self) -> Result<()> {
        self.send(Command::QueueChanged)
    }

    /// Changes [`PlayerConfig::gapless`]. Takes effect from the next track
    /// whose decoding ends: one already buffered stays.
    pub fn set_gapless(&self, gapless: bool) -> Result<()> {
        self.send(Command::SetGapless(gapless))
    }

    /// Output volume, 0.0 to 1.0.
    pub fn set_volume(&self, volume: f32) {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn status(&self) -> Status {
        let consumed = self.shared.consumed.load(Ordering::Acquire);
        let timeline = lock(&self.timeline);
        let underruns = self.shared.underruns.load(Ordering::Relaxed);
        let stopped = Status {
            state: PlaybackState::Stopped,
            entry: None,
            playthrough: 0,
            position: Duration::ZERO,
            duration: None,
            underruns,
            output_failed: self.shared.output_failed.load(Ordering::Acquire),
            preferred_back: self.shared.preferred_back.load(Ordering::Acquire),
            output_rate: self.spec.rate,
            output_channels: self.spec.channels,
        };
        let starting = self.shared.starting.load(Ordering::Acquire);
        if !starting && timeline.end.is_some_and(|end| consumed >= end) {
            return stopped;
        }
        let state = if self.shared.paused.load(Ordering::Relaxed) {
            PlaybackState::Paused
        } else {
            PlaybackState::Playing
        };
        let Some(b) = timeline
            .playing(consumed)
            .and_then(|i| timeline.boundaries.get(i))
        else {
            // A track was requested and is still loading.
            return Status { state, ..stopped };
        };
        let mut position =
            b.offset + frames_to_duration(consumed.saturating_sub(b.start), self.spec.rate);
        if let Some(d) = b.duration {
            position = position.min(d);
        }
        Status {
            state,
            entry: Some(b.entry.id),
            playthrough: b.playthrough,
            position,
            duration: b.duration,
            underruns,
            ..stopped
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(decoder) = self.decoder.take() {
            let _ = decoder.join();
        }
        drop(self.output.take());
    }
}

#[cfg(test)]
#[path = "tests/player_test.rs"]
mod tests;
