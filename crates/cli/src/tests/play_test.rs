use clap::Parser;

use super::*;

#[derive(Parser)]
struct Play {
    #[command(flatten)]
    args: Args,
}

fn play(words: &[&str]) -> Result<Args, clap::Error> {
    Play::try_parse_from(std::iter::once("play").chain(words.iter().copied())).map(|p| p.args)
}

#[test]
fn files_alone_play_on_the_device_in_real_time_at_the_default_rate() {
    assert_eq!(
        play(&["a.flac", "b.m4a"]).unwrap(),
        Args {
            capture: None,
            null: false,
            fast: false,
            rate: None,
            files: vec![PathBuf::from("a.flac"), PathBuf::from("b.m4a")],
        }
    );
}

#[test]
fn capture_null_fast_and_rate_are_read_wherever_they_appear() {
    let args = play(&["a.flac", "--capture", "out.WAV", "--fast", "--rate=44100"]).unwrap();
    assert_eq!(args.capture, Some(PathBuf::from("out.WAV")));
    assert!(args.fast);
    assert_eq!(args.rate, Some(44_100));

    assert!(play(&["--null", "--fast", "a.flac"]).unwrap().null);
}

#[test]
fn arguments_after_a_double_dash_are_files_even_if_they_look_like_options() {
    assert_eq!(
        play(&["--", "--null", "-h"]).unwrap().files,
        [PathBuf::from("--null"), PathBuf::from("-h")]
    );
}

#[test]
fn invalid_combinations_and_values_are_errors() {
    for words in [
        &[][..],
        &["--null"],
        &["--fast", "a.flac"],
        &["--capture", "out.wav", "--null", "a.flac"],
        &["--capture"],
        &["--rate", "44100", "a.flac"],
        &["--rate", "fast", "--null", "a.flac"],
        &["--rate", "-1", "--null", "a.flac"],
        &["--loud", "a.flac"],
    ] {
        assert!(play(words).is_err(), "{words:?}");
    }
}

#[cfg(unix)]
#[test]
fn a_capture_path_need_not_be_utf8() {
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::OsStr::from_bytes(b"caf\xe9.wav");
    let args = Play::try_parse_from([
        "play".as_ref(),
        "--capture".as_ref(),
        path,
        "a.flac".as_ref(),
    ])
    .unwrap()
    .args;
    assert_eq!(args.capture, Some(PathBuf::from(path)));
}

#[test]
fn a_capture_path_without_a_wav_extension_is_refused_before_playing() {
    let err = run(play(&["--capture", "out.mp3", "a.flac"]).unwrap()).unwrap_err();
    assert!(err.to_string().contains("must end in .wav"), "{err}");
}
