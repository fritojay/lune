use std::path::PathBuf;

/// Each message includes its cause, so no variant also returns it from
/// `source()`: a caller printing the chain (as `anyhow` does) would show it
/// twice.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("could not open {path}: {cause}")]
    Open {
        path: PathBuf,
        cause: std::io::Error,
    },

    #[error("could not decode {path}: {cause}")]
    Decode {
        path: PathBuf,
        cause: symphonia::core::errors::Error,
    },

    #[error("the decoder panicked on {path}")]
    DecoderPanic { path: PathBuf },

    #[error("{path} is not playable: {reason}")]
    InvalidFormat { path: PathBuf, reason: &'static str },

    #[error("{path} has no audio track")]
    NoAudioTrack { path: PathBuf },

    #[error("the resampler panicked")]
    ResamplerPanic,

    #[error("resampler error: {0}")]
    Resample(BoxError),

    #[error("unsupported audio format: {rate} Hz, {channels} channels")]
    UnsupportedFormat { rate: u32, channels: usize },

    #[error("no audio output device is available")]
    NoOutputDevice,

    #[error("audio output error: {context}: {cause}")]
    Output {
        context: &'static str,
        cause: BoxError,
    },

    #[error("invalid output configuration: {0}")]
    OutputConfig(String),

    #[error("I/O error: {0}")]
    Io(std::io::Error),

    #[error("the audio engine thread has stopped")]
    EngineStopped,
}

pub type Result<T> = std::result::Result<T, EngineError>;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

impl From<std::io::Error> for EngineError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl EngineError {
    pub(crate) fn resample(err: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Resample(Box::new(err))
    }

    pub(crate) fn output(context: &'static str) -> impl FnOnce(cpal::Error) -> Self {
        move |err| Self::Output {
            context,
            cause: Box::new(err),
        }
    }
}

#[cfg(test)]
#[path = "tests/error_test.rs"]
mod tests;
