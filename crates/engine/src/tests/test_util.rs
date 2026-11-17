use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{OutputKind, PlayQueue, PlaybackState, Player, PlayerConfig};

/// A directory under the system temp dir, removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = create_fresh_dir(&std::env::temp_dir(), "lune-engine-test");
        Self(path)
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        #[expect(clippy::disallowed_methods, reason = "removes the test's own temp dir")]
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes a 16-bit PCM WAV of `frames` frames: a quiet 440 Hz sine whose
/// phase continues from `start_frame`, so consecutive segments join smoothly.
pub fn write_wav(path: &Path, rate: u32, channels: u16, start_frame: u64, frames: u64) {
    let data_len = u32::try_from(frames * u64::from(channels) * 2).unwrap();
    let mut bytes = Vec::with_capacity(44 + data_len as usize);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    // Wraps so tests can write hostile rates; decoders ignore this field.
    let byte_rate = rate.wrapping_mul(u32::from(channels) * 2);
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for n in start_frame..start_frame + frames {
        let t = n as f64 / f64::from(rate);
        let s = (0.25 * (2.0 * std::f64::consts::PI * 440.0 * t).sin() * 32767.0) as i16;
        for _ in 0..channels {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
    }
    #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&bytes).unwrap();
}

pub fn capture_frames(path: &Path, channels: usize) -> u64 {
    let bytes = std::fs::read(path).unwrap();
    let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
    u64::from(data) / (4 * channels as u64)
}

pub fn capture(path: &Path, rate: u32) -> PlayerConfig {
    PlayerConfig {
        output: OutputKind::Capture {
            path: Some(path.to_path_buf()),
            rate,
            channels: 2,
            realtime: false,
        },
        ..PlayerConfig::default()
    }
}

/// Waits until `player` reports Stopped. Panics after 20 s.
pub fn wait_stopped(player: &Player) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while player.status().state != PlaybackState::Stopped {
        assert!(Instant::now() < deadline, "player never stopped");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub fn play_to_end(
    dir: &TempDir,
    rate: u32,
    queue: Arc<dyn PlayQueue>,
    first: crate::QueueEntry,
) -> u64 {
    let out = dir.join("capture.wav");
    {
        let player = Player::new(capture(&out, rate), queue).unwrap();
        player.play(first).unwrap();
        wait_stopped(&player);
    }
    capture_frames(&out, 2)
}

/// A new directory `<prefix>-<pid>-<n>` in `parent`. `create_dir` fails on
/// anything already there, so a link planted at a predictable name in a
/// shared temp dir is skipped instead of written through.
pub fn create_fresh_dir(parent: &Path, prefix: &str) -> PathBuf {
    let mut n = 0u64;
    loop {
        let path = parent.join(format!("{prefix}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(err) => panic!("could not create {}: {err}", path.display()),
        }
    }
}

#[path = "test_util_test.rs"]
mod tests;
