//! Media keys and the OS "Now Playing" display (MPRIS on Linux, Now Playing
//! on macOS), behind [`MediaControls`] so the player doesn't depend on the
//! platform library.

use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection,
};

/// How far a "seek" without an amount (a media key) moves.
const SEEK_STEP: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// To an absolute position.
    Seek(Duration),
    /// Relative, in seconds; negative is backward.
    SeekBy(f64),
    SetVolume(f64),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<Duration>,
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Playback {
    Stopped,
    Paused(Duration),
    Playing(Duration),
}

pub trait MediaControls {
    fn set_now_playing(&mut self, now: &NowPlaying);
    fn set_playback(&mut self, playback: Playback);
    fn set_volume(&mut self, volume: f64);
}

/// The OS media controls, or `None` where they can't be set up (e.g. no
/// D-Bus session). `on_command` is called from a platform thread.
pub fn system(
    on_command: impl Fn(MediaCommand) + Send + 'static,
) -> Option<Box<dyn MediaControls>> {
    let config = PlatformConfig {
        display_name: "Lune",
        dbus_name: "lune",
        hwnd: None,
    };
    let mut controls = match souvlaki::MediaControls::new(config) {
        Ok(controls) => controls,
        Err(err) => {
            tracing::warn!(?err, "no media key or Now Playing support");
            return None;
        }
    };
    let attached = controls.attach(move |event| {
        if let Some(command) = command_for(event) {
            on_command(command);
        }
    });
    if let Err(err) = attached {
        tracing::warn!(?err, "no media key or Now Playing support");
        return None;
    }
    Some(Box::new(Souvlaki(controls)))
}

/// What the player does for a platform event; `None` for events it ignores.
pub fn command_for(event: MediaControlEvent) -> Option<MediaCommand> {
    let signed = |direction, amount: Duration| match direction {
        SeekDirection::Forward => amount.as_secs_f64(),
        SeekDirection::Backward => -amount.as_secs_f64(),
    };
    Some(match event {
        MediaControlEvent::Play => MediaCommand::Play,
        MediaControlEvent::Pause => MediaCommand::Pause,
        MediaControlEvent::Toggle => MediaCommand::Toggle,
        MediaControlEvent::Next => MediaCommand::Next,
        MediaControlEvent::Previous => MediaCommand::Previous,
        MediaControlEvent::Stop => MediaCommand::Stop,
        MediaControlEvent::Seek(direction) => MediaCommand::SeekBy(signed(direction, SEEK_STEP)),
        MediaControlEvent::SeekBy(direction, amount) => {
            MediaCommand::SeekBy(signed(direction, amount))
        }
        MediaControlEvent::SetPosition(MediaPosition(position)) => MediaCommand::Seek(position),
        MediaControlEvent::SetVolume(volume) if volume.is_finite() => {
            MediaCommand::SetVolume(volume.clamp(0.0, 1.0))
        }
        MediaControlEvent::SetVolume(_)
        | MediaControlEvent::SetShuffle(_)
        | MediaControlEvent::OpenUri(_)
        | MediaControlEvent::Raise
        | MediaControlEvent::Quit => return None,
        // The enum is non-exhaustive: events added later are ignored too.
        _ => return None,
    })
}

struct Souvlaki(souvlaki::MediaControls);

impl MediaControls for Souvlaki {
    fn set_now_playing(&mut self, now: &NowPlaying) {
        let metadata = MediaMetadata {
            title: Some(&now.title),
            album: Some(&now.album),
            artist: Some(&now.artist),
            cover_url: now.cover.as_deref(),
            duration: now.duration,
        };
        if let Err(err) = self.0.set_metadata(metadata) {
            tracing::debug!(?err, "could not update Now Playing");
        }
    }

    fn set_playback(&mut self, playback: Playback) {
        let playback = match playback {
            Playback::Stopped => MediaPlayback::Stopped,
            Playback::Paused(at) => MediaPlayback::Paused {
                progress: Some(MediaPosition(at)),
            },
            Playback::Playing(at) => MediaPlayback::Playing {
                progress: Some(MediaPosition(at)),
            },
        };
        if let Err(err) = self.0.set_playback(playback) {
            tracing::debug!(?err, "could not update the playback state");
        }
    }

    fn set_volume(&mut self, _volume: f64) {
        // Only MPRIS has a volume to report back.
        #[cfg(target_os = "linux")]
        if let Err(err) = self.0.set_volume(_volume) {
            tracing::debug!(?err, "could not update the volume");
        }
    }
}

/// A cover image file as the platforms expect it. macOS strips `file://` and
/// uses the rest as a plain path, so it is not percent-encoded; thumbnail
/// paths are the app's own (a cache folder and a hex name). `None` for a
/// path that isn't UTF-8.
pub fn cover_url(path: &std::path::Path) -> Option<String> {
    Some(format!("file://{}", path.to_str()?))
}

#[cfg(test)]
#[path = "tests/media_controls_test.rs"]
mod tests;
