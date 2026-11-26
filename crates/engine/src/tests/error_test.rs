use super::*;

#[test]
fn an_error_shows_every_cause() {
    let io = std::io::Error::other("disk on fire");
    let err = EngineError::Output {
        context: "starting the stream",
        source: Box::new(io),
    };
    assert_eq!(
        err.to_string(),
        "audio output error: starting the stream: disk on fire"
    );
}
