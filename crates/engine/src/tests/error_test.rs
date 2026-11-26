use std::error::Error;

use super::*;

fn io() -> std::io::Error {
    std::io::Error::other("disk on fire")
}

#[test]
fn an_error_shows_its_cause_once_even_with_its_chain() {
    let path = PathBuf::from("a.flac");
    for err in [
        EngineError::Open {
            path: path.clone(),
            cause: io(),
        },
        EngineError::Decode {
            path,
            cause: symphonia::core::errors::Error::IoError(io()),
        },
        EngineError::resample(io()),
        EngineError::Output {
            context: "starting the stream",
            cause: Box::new(io()),
        },
        EngineError::from(io()),
    ] {
        let mut chain = err.to_string();
        let mut source = err.source();
        while let Some(cause) = source {
            chain.push_str(&format!(": {cause}"));
            source = cause.source();
        }
        assert_eq!(chain.matches("disk on fire").count(), 1, "{chain}");
    }
}

#[test]
fn an_output_error_says_what_failed_and_why() {
    let err = EngineError::Output {
        context: "starting the stream",
        cause: Box::new(io()),
    };
    assert_eq!(
        err.to_string(),
        "audio output error: starting the stream: disk on fire"
    );
}
