use clap::CommandFactory;
use proptest::prelude::*;

use super::*;

fn args(words: &[&str]) -> Result<Args, clap::Error> {
    Args::try_parse_from(std::iter::once("lune").chain(words.iter().copied()))
}

#[test]
fn the_command_definition_is_valid() {
    Args::command().debug_assert();
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

proptest! {
    #[test]
    fn db_config_and_bench_are_read_in_any_order_and_either_form(
        db in prop::option::of("/[a-z ]{1,8}\\.db"),
        config in prop::option::of("/[a-z ]{1,8}\\.toml"),
        bench: bool,
        joined: [bool; 2],
        order in Just(vec![0, 1, 2]).prop_shuffle(),
    ) {
        let option = |name: &str, value: &Option<String>, joined: bool| match value {
            None => vec![],
            Some(value) if joined => vec![format!("--{name}={value}")],
            Some(value) => vec![format!("--{name}"), value.clone()],
        };
        let groups = [
            option("db", &db, joined[0]),
            option("config", &config, joined[1]),
            if bench { vec!["--bench".to_owned()] } else { vec![] },
        ];
        let words: Vec<&str> = order.iter().flat_map(|&i| &groups[i]).map(String::as_str).collect();
        prop_assert_eq!(
            args(&words).unwrap(),
            Args {
                db_path: db.map(PathBuf::from),
                config_path: config.map(PathBuf::from),
                bench,
            }
        );
    }
}

#[test]
fn a_held_library_says_so_and_one_that_cant_be_locked_runs_with_a_warning() {
    let dir = crate::test_util::TempDir::new();
    let db = dir.path().join("library.sqlite3");
    let (lock, normal) = startup(lune_core::lock_library(&db));
    assert!(lock.is_some());
    assert_eq!(normal, Startup::Normal);
    assert_eq!(startup(lune_core::lock_library(&db)).1, Startup::InUse);

    // A file where the library's folder should be: no lock file can be made.
    let blocked = dir.write("not-a-folder", b"").join("library.sqlite3");
    let (lock, unlocked) = startup(lune_core::lock_library(&blocked));
    assert!(lock.is_none());
    assert!(
        matches!(&unlocked, Startup::Unlocked(warning) if warning.contains("two at once")),
        "{unlocked:?}"
    );
}
