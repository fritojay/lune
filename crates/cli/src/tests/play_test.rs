use clap::Parser;
use proptest::prelude::*;

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

/// `--name value` or `--name=value`.
fn option(name: &str, value: String, joined: bool) -> Vec<String> {
    if joined {
        vec![format!("--{name}={value}")]
    } else {
        vec![format!("--{name}"), value]
    }
}

/// What to play on: the device, a capture file, or nowhere (`--null`).
fn arb_sink() -> impl Strategy<Value = (Option<PathBuf>, bool)> {
    prop_oneof![
        Just((None, false)),
        Just((None, true)),
        prop::sample::select(&["out.wav", "out.WAV", "a b.wav"][..])
            .prop_map(|p| (Some(PathBuf::from(p)), false)),
    ]
}

proptest! {
    #[test]
    fn options_are_read_in_either_form_wherever_they_appear(
        (capture, null) in arb_sink(),
        fast: bool,
        rate in prop::option::of(prop::sample::select(&[8_000u32, 44_100, 768_000][..])),
        files in prop::collection::vec("[a-z][a-z .]{0,8}", 1..4),
        joined: [bool; 2],
        places: [prop::sample::Index; 4],
    ) {
        let device = capture.is_none() && !null;
        let (fast, rate) = if device { (false, None) } else { (fast, rate) };
        let mut options: Vec<Vec<String>> = Vec::new();
        if let Some(path) = &capture {
            options.push(option("capture", path.display().to_string(), joined[0]));
        }
        if null {
            options.push(vec!["--null".to_owned()]);
        }
        if fast {
            options.push(vec!["--fast".to_owned()]);
        }
        if let Some(rate) = rate {
            options.push(option("rate", rate.to_string(), joined[1]));
        }
        // Each option goes before the file at its place, or after them all.
        let mut words: Vec<(usize, Vec<String>)> = files.iter().cloned().map(|f| vec![f]).enumerate().collect();
        for (option, place) in options.into_iter().zip(places) {
            words.push((place.index(files.len() + 1), option));
        }
        words.sort_by_key(|&(place, ref w)| (place, !w[0].starts_with("--")));
        let words: Vec<String> = words.into_iter().flat_map(|(_, w)| w).collect();
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        prop_assert_eq!(
            play(&words).unwrap(),
            Args {
                capture,
                null,
                fast,
                rate,
                files: files.into_iter().map(PathBuf::from).collect(),
            }
        );
    }
}
