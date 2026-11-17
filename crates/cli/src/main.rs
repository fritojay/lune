mod play;

use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::{Context, bail};
use lune_core::{Library, ScanMode, TrackRow};

const USAGE: &str = "\
usage: lune-cli [--db <path>] <command>

commands:
  play [options] <files..>  play files in order, gaplessly (see `play --help`)
  scan <dir>              scan <dir> into the library (incremental)
  query stats             counts of tracks, albums, and art
  query albums            all albums: id, year, album artist, title
  query album <id>        an album's tracks in album order
  query search <words..>  full-text search (prefix match on every word)
  gen-library <db> --tracks <n>
                          create a fake library for UI benchmarks (needs the
                          `synthetic` feature; <db> must be new or empty)

The database defaults to the platform data directory; --db overrides it.";

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let mut args: Vec<OsString> = std::env::args_os().skip(1).collect();
    // `--db` is only an option before the command, so a file named `--db`
    // among `play`'s arguments is never taken for it.
    let db_path = if args.first().is_some_and(|a| a == "--db") {
        if args.len() < 2 {
            bail!("--db needs a path\n\n{USAGE}");
        }
        let path = PathBuf::from(args.remove(1));
        args.remove(0);
        path
    } else {
        lune_core::default_db_path()?
    };

    let words: Vec<String> = args
        .iter()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let command = args.first().and_then(|a| a.to_str()).unwrap_or("");
    match command {
        "play" => play::run(&args[1..])?,
        "scan" => {
            let [dir] = &args[1..] else {
                bail!("scan takes one directory\n\n{USAGE}");
            };
            let mut library = open(&db_path)?;
            let report = library.scan(&PathBuf::from(dir), ScanMode::Thorough, |_| {})?;
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
        "query" => {
            let library = open(&db_path)?;
            match words.first().map(String::as_str) {
                Some("stats") => {
                    let s = library.stats()?;
                    println!(
                        "{} tracks, {} albums, {} artworks, {:.1} hours",
                        s.tracks,
                        s.albums,
                        s.art,
                        s.total_duration_ms as f64 / 3_600_000.0
                    );
                }
                Some("albums") => {
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
                Some("album") => {
                    let id: i64 = words
                        .get(1)
                        .context("query album needs an album id")?
                        .parse()
                        .context("album id must be a number")?;
                    print_tracks(&library.album_tracks(id)?);
                }
                Some("search") => {
                    print_tracks(&library.search(&words[1..].join(" "), 200)?);
                }
                _ => bail!("unknown query\n\n{USAGE}"),
            }
        }
        "gen-library" => gen_library(&args[1..])?,
        _ => bail!("{USAGE}"),
    }
    Ok(())
}

/// Takes its own DB path rather than `--db`, so it never touches the real
/// library by default.
#[cfg(feature = "synthetic")]
fn gen_library(args: &[OsString]) -> anyhow::Result<()> {
    let [db, flag, count] = args else {
        bail!("usage: gen-library <db> --tracks <n>");
    };
    if flag != "--tracks" {
        bail!("usage: gen-library <db> --tracks <n>");
    }
    let tracks: usize = count
        .to_str()
        .context("--tracks must be a number")?
        .parse()
        .context("--tracks must be a number")?;
    let db = PathBuf::from(db);
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
fn gen_library(_args: &[OsString]) -> anyhow::Result<()> {
    bail!("gen-library needs a build with `--features synthetic`");
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

fn print_tracks(tracks: &[TrackRow]) {
    for t in tracks {
        let title = t.title.clone().unwrap_or_else(|| {
            t.path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        });
        let secs = t.duration_ms / 1000;
        println!(
            "{:>7}  {}-{:<3} {} - {}  [{} - {}, {}]  {}:{:02} {} {}",
            t.id,
            t.disc_no.unwrap_or(1),
            t.track_no.map_or_else(|| "?".to_owned(), |n| n.to_string()),
            t.artist.as_deref().unwrap_or("(unknown)"),
            title,
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
