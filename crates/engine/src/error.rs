use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("could not open {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not decode {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: symphonia::core::errors::Error,
    },

    #[error("the decoder panicked on {path}")]
    DecoderPanic { path: PathBuf },

    #[error("{path} is not playable: {reason}")]
    InvalidFormat { path: PathBuf, reason: &'static str },

    #[error("{path} has no audio track")]
    NoAudioTrack { path: PathBuf },

    #[error("the resampler panicked")]
    ResamplerPanic,

    #[error("resampler error")]
    Resample(#[source] BoxError),

    #[error("unsupported audio format: {rate} Hz, {channels} channels")]
    UnsupportedFormat { rate: u32, channels: usize },

    #[error("no audio output device is available")]
    NoOutputDevice,

    #[error("audio output error: {context}")]
    Output {
        context: &'static str,
        #[source]
        source: BoxError,
    },

    #[error("invalid output configuration: {0}")]
    OutputConfig(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("the audio engine thread has stopped")]
    EngineStopped,
}

pub type Result<T> = std::result::Result<T, EngineError>;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

impl EngineError {
    pub(crate) fn resample(err: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Resample(Box::new(err))
    }

    pub(crate) fn output(context: &'static str) -> impl FnOnce(cpal::Error) -> Self {
        move |err| Self::Output {
            context,
            source: Box::new(err),
        }
    }
}

pub(crate) struct Chain<'a>(pub &'a (dyn std::error::Error + 'static));

impl std::fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(err) = source {
            write!(f, ": {err}")?;
            source = err.source();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/error_test.rs"]
mod tests;
