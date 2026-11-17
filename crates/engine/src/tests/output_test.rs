use std::cell::Cell;

use super::*;

#[test]
fn capture_rejects_out_of_range_formats() {
    for (rate, channels) in [(0, 2), (1_000_000, 2), (48_000, 0), (48_000, 1000)] {
        let kind = OutputKind::Capture {
            path: None,
            rate,
            channels,
            realtime: false,
        };
        let shared = Arc::new(Shared::new());
        let started = start(kind, Arc::clone(&shared), |_| unreachable!());
        assert!(started.is_err(), "{rate} Hz x{channels} was accepted");
    }
}

#[test]
fn wav_writer_never_overwrites_an_existing_file() {
    let path = std::env::temp_dir().join(format!("lune-wav-exists-{}.wav", std::process::id()));
    #[expect(clippy::disallowed_methods, reason = "test fixture in the temp dir")]
    std::fs::write(&path, b"precious music").unwrap();
    let spec = OutputSpec {
        rate: 48_000,
        channels: 2,
    };
    assert!(WavWriter::create(&path, spec).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"precious music");
    #[expect(clippy::disallowed_methods, reason = "removes the test's own file")]
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn only_errors_that_stop_the_stream_count_as_a_failed_output() {
    for kind in [
        ErrorKind::Xrun,
        ErrorKind::DeviceChanged,
        ErrorKind::RealtimeDenied,
    ] {
        assert!(!stream_failed(kind), "{kind:?}");
    }
    for kind in [
        ErrorKind::DeviceNotAvailable,
        ErrorKind::StreamInvalidated,
        ErrorKind::BackendError,
        ErrorKind::Other,
    ] {
        assert!(stream_failed(kind), "{kind:?}");
    }
}

struct FakeStream {
    playing: Cell<bool>,
    resumes: Cell<u32>,
    suspends: Cell<u32>,
    /// How many of the next resumes fail.
    failing_resumes: Cell<u32>,
    fail_suspend: bool,
}

impl FakeStream {
    fn running() -> Self {
        Self {
            playing: Cell::new(true),
            resumes: Cell::new(0),
            suspends: Cell::new(0),
            failing_resumes: Cell::new(0),
            fail_suspend: false,
        }
    }
}

impl Pausable for FakeStream {
    fn resume(&self) -> std::result::Result<(), cpal::Error> {
        self.resumes.set(self.resumes.get() + 1);
        if self.failing_resumes.get() > 0 {
            self.failing_resumes.set(self.failing_resumes.get() - 1);
            return Err(cpal::Error::new(ErrorKind::DeviceNotAvailable));
        }
        self.playing.set(true);
        Ok(())
    }

    fn suspend(&self) -> std::result::Result<(), cpal::Error> {
        self.suspends.set(self.suspends.get() + 1);
        if self.fail_suspend {
            return Err(cpal::Error::new(ErrorKind::UnsupportedOperation));
        }
        self.playing.set(false);
        Ok(())
    }
}

fn put_to_sleep(stream: &FakeStream, shared: &Shared, start: Instant) -> Sleeper {
    let mut sleeper = Sleeper::default();
    sleeper.check(stream, shared, start);
    sleeper.check(stream, shared, start + SLEEP_AFTER);
    assert!(!stream.playing.get(), "setup: the stream did not sleep");
    sleeper
}

#[test]
fn the_stream_sleeps_only_after_staying_idle() {
    let (stream, shared) = (FakeStream::running(), Shared::new());
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    let mut sleeper = Sleeper::default();
    sleeper.check(&stream, &shared, at(0));
    sleeper.check(&stream, &shared, at(1999));
    assert!(stream.playing.get());

    shared.expecting_data.store(true, Ordering::Release);
    sleeper.check(&stream, &shared, at(2000));
    shared.expecting_data.store(false, Ordering::Release);
    sleeper.check(&stream, &shared, at(2100));
    sleeper.check(&stream, &shared, at(4099));
    assert!(stream.playing.get(), "activity resets the wait");
    sleeper.check(&stream, &shared, at(4100));
    assert!(!stream.playing.get());
}

#[test]
fn a_busy_stream_never_sleeps() {
    let (stream, shared) = (FakeStream::running(), Shared::new());
    shared.expecting_data.store(true, Ordering::Release);
    let start = Instant::now();
    let mut sleeper = Sleeper::default();
    for s in 0..10 {
        sleeper.check(&stream, &shared, start + Duration::from_secs(s));
    }
    assert!(stream.playing.get());
}

#[test]
fn a_wake_restarts_a_sleeping_stream_and_resets_the_wait() {
    let (stream, shared) = (FakeStream::running(), Shared::new());
    let start = Instant::now();
    let mut sleeper = put_to_sleep(&stream, &shared, start);
    sleeper.wake(&stream, &shared);
    assert!(stream.playing.get());
    sleeper.check(&stream, &shared, start + SLEEP_AFTER * 2);
    assert!(stream.playing.get(), "it slept again without a full wait");
}

#[test]
fn a_sleeping_stream_restarts_when_audio_arrives_without_a_wake() {
    let (stream, shared) = (FakeStream::running(), Shared::new());
    let start = Instant::now();
    let mut sleeper = put_to_sleep(&stream, &shared, start);
    // E.g. a seek whose file took longer than SLEEP_AFTER to open.
    shared.expecting_data.store(true, Ordering::Release);
    sleeper.check(&stream, &shared, start + SLEEP_AFTER * 2);
    assert!(stream.playing.get());
}

#[test]
fn a_stream_that_cannot_pause_keeps_running_and_stops_trying() {
    let stream = FakeStream {
        fail_suspend: true,
        ..FakeStream::running()
    };
    let shared = Shared::new();
    let start = Instant::now();
    let mut sleeper = Sleeper::default();
    sleeper.check(&stream, &shared, start);
    for s in 1..=5 {
        sleeper.check(&stream, &shared, start + SLEEP_AFTER * s);
    }
    assert_eq!(stream.suspends.get(), 1, "it kept trying to pause");
    sleeper.wake(&stream, &shared);
    assert!(stream.playing.get());
    assert_eq!(stream.resumes.get(), 0);
    assert!(!shared.output_failed.load(Ordering::Acquire));
}

#[test]
fn a_failed_restart_is_retried_on_the_next_wake() {
    let stream = FakeStream::running();
    let shared = Shared::new();
    let mut sleeper = put_to_sleep(&stream, &shared, Instant::now());
    stream.failing_resumes.set(1);
    sleeper.wake(&stream, &shared);
    assert!(!stream.playing.get());
    sleeper.wake(&stream, &shared);
    assert!(stream.playing.get());
    assert!(!shared.output_failed.load(Ordering::Acquire));
}

#[test]
fn a_failed_restart_is_retried_once_audio_is_coming() {
    let stream = FakeStream::running();
    let shared = Shared::new();
    let start = Instant::now();
    let mut sleeper = put_to_sleep(&stream, &shared, start);
    stream.failing_resumes.set(1);
    sleeper.wake(&stream, &shared);
    shared.expecting_data.store(true, Ordering::Release);
    sleeper.check(&stream, &shared, start + SLEEP_AFTER * 2);
    assert!(stream.playing.get());
}

#[test]
fn only_restarts_failing_in_a_row_fail_the_output() {
    let stream = FakeStream::running();
    let shared = Shared::new();
    let start = Instant::now();
    let mut sleeper = put_to_sleep(&stream, &shared, start);
    stream.failing_resumes.set(RESTART_ATTEMPTS - 1);
    for _ in 0..RESTART_ATTEMPTS {
        sleeper.wake(&stream, &shared);
    }
    assert!(stream.playing.get());

    sleeper.check(&stream, &shared, start + SLEEP_AFTER * 2);
    sleeper.check(&stream, &shared, start + SLEEP_AFTER * 3);
    assert!(
        !stream.playing.get(),
        "setup: the stream did not sleep again"
    );
    stream.failing_resumes.set(RESTART_ATTEMPTS - 1);
    for _ in 0..RESTART_ATTEMPTS {
        sleeper.wake(&stream, &shared);
    }
    assert!(stream.playing.get());
    assert!(!shared.output_failed.load(Ordering::Acquire));
}

#[test]
fn restarts_that_keep_failing_fail_the_output_for_good() {
    let stream = FakeStream {
        failing_resumes: Cell::new(u32::MAX),
        ..FakeStream::running()
    };
    let shared = Shared::new();
    let start = Instant::now();
    let mut sleeper = put_to_sleep(&stream, &shared, start);
    for _ in 1..RESTART_ATTEMPTS {
        sleeper.wake(&stream, &shared);
    }
    assert!(!shared.output_failed.load(Ordering::Acquire));
    sleeper.wake(&stream, &shared);
    assert!(shared.output_failed.load(Ordering::Acquire));

    sleeper.wake(&stream, &shared);
    shared.expecting_data.store(true, Ordering::Release);
    sleeper.check(&stream, &shared, start + SLEEP_AFTER * 2);
    assert_eq!(
        stream.resumes.get(),
        RESTART_ATTEMPTS,
        "a failed output was restarted"
    );
}
