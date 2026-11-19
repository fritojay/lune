use clap::CommandFactory;

use super::*;

fn cli(words: &[&str]) -> Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("lune-cli").chain(words.iter().copied()))
}

#[test]
fn the_command_definition_is_valid() {
    Cli::command().debug_assert();
}

#[test]
fn db_before_the_command_picks_the_database() {
    let expected = Some(PathBuf::from("/tmp/dev.db"));
    assert_eq!(
        cli(&["--db", "/tmp/dev.db", "query", "stats"]).unwrap().db,
        expected
    );
    assert_eq!(
        cli(&["--db=/tmp/dev.db", "query", "stats"]).unwrap().db,
        expected
    );
    assert_eq!(cli(&["query", "stats"]).unwrap().db, None);
}

#[test]
fn db_after_the_command_is_not_taken_for_the_database() {
    assert!(cli(&["scan", "--db", "x.db", "/music"]).is_err());
    let cli = cli(&["play", "--", "--db"]).unwrap();
    assert_eq!(cli.db, None);
    let Command::Play(args) = cli.command else {
        panic!("not play");
    };
    assert_eq!(args.files, [PathBuf::from("--db")]);
}

#[test]
fn queries_take_their_arguments() {
    let query = |words: &[&str]| match cli(words).unwrap().command {
        Command::Query(query) => query,
        other => panic!("{other:?}"),
    };
    assert_eq!(query(&["query", "album", "42"]), Query::Album { id: 42 });
    assert_eq!(
        query(&["query", "search", "two", "-words"]),
        Query::Search {
            words: vec![OsString::from("two"), OsString::from("-words")]
        }
    );
}

#[test]
fn gen_library_takes_a_db_and_a_track_count_in_either_order() {
    for words in [
        &["gen-library", "bench.db", "--tracks", "100000"][..],
        &["gen-library", "--tracks=100000", "bench.db"],
    ] {
        let Command::GenLibrary { db, tracks } = cli(words).unwrap().command else {
            panic!("not gen-library");
        };
        assert_eq!((db, tracks), (PathBuf::from("bench.db"), 100_000));
    }
}

#[test]
fn missing_or_malformed_arguments_are_errors() {
    for words in [
        &[][..],
        &["--db"],
        &["--db", "x.db"],
        &["bogus"],
        &["scan"],
        &["scan", "a", "b"],
        &["query"],
        &["query", "album"],
        &["query", "album", "first"],
        &["query", "search"],
        &["gen-library", "bench.db"],
        &["gen-library", "--tracks", "many", "bench.db"],
    ] {
        assert!(cli(words).is_err(), "{words:?}");
    }
}
