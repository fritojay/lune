use clap::CommandFactory;

use super::*;

fn args(words: &[&str]) -> Result<Args, clap::Error> {
    Args::try_parse_from(std::iter::once("lune").chain(words.iter().copied()))
}

#[test]
fn the_command_definition_is_valid() {
    Args::command().debug_assert();
}

#[test]
fn no_arguments_use_the_default_paths_without_bench() {
    assert_eq!(
        args(&[]).unwrap(),
        Args {
            db_path: None,
            config_path: None,
            bench: false,
        }
    );
}

#[test]
fn db_config_and_bench_are_read_in_any_order_and_either_form() {
    assert_eq!(
        args(&[
            "--bench",
            "--db",
            "/tmp/bench.db",
            "--config=/tmp/lune.toml"
        ])
        .unwrap(),
        Args {
            db_path: Some(PathBuf::from("/tmp/bench.db")),
            config_path: Some(PathBuf::from("/tmp/lune.toml")),
            bench: true,
        }
    );
}

#[test]
fn a_missing_path_an_unknown_option_or_a_stray_value_is_an_error() {
    for words in [
        &["--db"][..],
        &["--config"],
        &["--fullscreen"],
        &["song.flac"],
        &["--bench=yes"],
    ] {
        assert!(args(words).is_err(), "{words:?}");
    }
}
