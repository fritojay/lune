mod play;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use lune_core::{Library, ListTrack, ScanMode};

/// Headless harness for the engine and library.
#[derive(Parser, Debug)]
#[command(name = "lune-cli", version)]
struct Cli {
    /// Library database (defaults to the platform data directory)
    #[arg(long, value_name = "PATH")]
    db: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Play files in order, gaplessly
    Play(play::Args),
    /// Scan a directory into the library (incremental)
    Scan { dir: PathBuf },
    /// Query the library
    #[command(subcommand)]
    Query(Query),
    /// Create a fake library for UI benchmarks (needs the `synthetic` feature)
    ///
    /// Takes its own DB path rather than --db, so it never touches the real
    /// library by default.
    GenLibrary {
        /// A new or empty database
        db: PathBuf,
        #[arg(long)]
        tracks: usize,
    },
}

#[derive(Subcommand, Debug, PartialEq, Eq)]
enum Query {
    /// Counts of tracks, albums, and art
    Stats,
    /// All albums: id, year, album artist, title
    Albums,
    /// An album's tracks in album order
    Album { id: i64 },
    /// Full-text search (prefix match on every word)
    Search {
        #[arg(required = true, allow_hyphen_values = true)]
        words: Vec<OsString>,
    },
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let db_path = || cli.db.clone().map_or_else(lune_core::default_db_path, Ok);
    match cli.command {
        Command::Play(args) => play::run(args)?,
        Command::Scan { dir } => {
            let mut library = open(&db_path()?)?;
            let report = library.scan(&dir, ScanMode::Thorough, |_| {})?;
            println!("scanned {}", report.root.display());
            println!(
                "  found {} files: {} added, {} updated, {} unchanged, {} missing, {} restored",
                report.files_found,
                report.added,
                report.updated,
                report.unchanged,
                report.missing,
                report.restored
            );
            if report.failed + report.skipped_non_utf8 + report.walk_errors > 0 {
                println!(
                    "  problems: {} unreadable, {} non-UTF-8 paths, {} walk errors (see log)",
                    report.failed, report.skipped_non_utf8, report.walk_errors
                );
            }
            println!(
                "  walk {:.2?}, total {:.2?}",
                report.walk_time, report.total_time
            );
        }
        Command::Query(query) => {
            let library = open(&db_path()?)?;
            match query {
                Query::Stats => {
                    let s = library.stats()?;
                    println!(
                        "{} tracks, {} albums, {} artworks, {:.1} hours",
                        s.tracks,
                        s.albums,
                        s.art,
                        s.total_duration_ms as f64 / 3_600_000.0
                    );
                }
                Query::Albums => {
                    for a in library.albums()? {
                        println!(
                            "{:>6}  {}  {} - {}  ({} tracks)",
                            a.id,
                            year(a.year_max),
                            or_unknown(&a.album_artist),
                            or_unknown(&a.title),
                            a.track_count
                        );
                    }
                }
                Query::Album { id } => print_tracks(&library.album_tracks(id)?),
                Query::Search { words } => {
                    let words: Vec<_> = words.iter().map(|w| w.to_string_lossy()).collect();
                    print_tracks(&library.search(&words.join(" "), 200)?);
                }
            }
        }
        Command::GenLibrary { db, tracks } => gen_library(db, tracks)?,
    }
    Ok(())
}

#[cfg(feature = "synthetic")]
fn gen_library(db: PathBuf, tracks: usize) -> anyhow::Result<()> {
    let mut library = open(&db)?;
    let started = std::time::Instant::now();
    library.generate_synthetic(tracks)?;
    let stats = library.stats()?;
    println!(
        "wrote {} tracks in {} albums to {} in {:.2?}",
        stats.tracks,
        stats.albums,
        db.display(),
        started.elapsed()
    );
    let started = std::time::Instant::now();
    let list = library.track_list()?;
    println!(
        "loading the track list ({} rows) took {:.2?}",
        list.len(),
        started.elapsed()
    );
    Ok(())
}

#[cfg(not(feature = "synthetic"))]
fn gen_library(_db: PathBuf, _tracks: usize) -> anyhow::Result<()> {
    anyhow::bail!("gen-library needs a build with `--features synthetic`");
}

fn open(db_path: &std::path::Path) -> anyhow::Result<Library> {
    Library::open(db_path).with_context(|| format!("opening {}", db_path.display()))
}

fn year(y: Option<i32>) -> String {
    y.map_or_else(|| "Unknown".to_owned(), |y| y.to_string())
}

fn or_unknown(s: &str) -> &str {
    if s.is_empty() { "(unknown)" } else { s }
}

fn print_tracks(tracks: &[ListTrack]) {
    for t in tracks {
        let secs = t.duration_ms / 1000;
        println!(
            "{:>7}  {}-{:<3} {} - {}  [{} - {}, {}]  {}:{:02} {} {}",
            t.id,
            t.disc_no.unwrap_or(1),
            t.track_no.map_or_else(|| "?".to_owned(), |n| n.to_string()),
            t.artist.as_deref().unwrap_or("(unknown)"),
            t.title,
            or_unknown(&t.album_artist),
            or_unknown(&t.album),
            year(t.album_year),
            secs / 60,
            secs % 60,
            t.codec,
            t.sample_rate.map_or_else(String::new, |r| format!("{r}Hz")),
        );
    }
}

#[cfg(test)]
#[path = "tests/main_test.rs"]
mod tests;
