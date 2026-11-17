use std::ffi::OsString;
use std::io::BufRead;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use lune_engine::{OutputKind, PlaybackState, Player, PlayerConfig, VecQueue};

const USAGE: &str = "\
usage: lune-cli play [options] <files..>

options:
  --capture <out.wav>  render to a new WAV file instead of the sound card
                       (refuses to overwrite an existing file)
  --null               render to nowhere (no sound card needed)
  --fast               with --capture/--null: render as fast as possible
  --rate <hz>          with --capture/--null: output rate (default 48000)

controls (type a line, then Enter):
  p        pause / resume      n        next track
  b        previous track      s <sec>  seek to <sec>
  v <0-1>  volume              q        quit";

pub fn run(args: &[OsString]) -> anyhow::Result<()> {
    let mut capture: Option<Option<PathBuf>> = None;
    let mut fast = false;
    let mut rate = 48_000u32;
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.to_str() {
            Some("--capture") => {
                let path = PathBuf::from(it.next().context("--capture needs a path")?);
                // The engine already refuses to overwrite (create_new); these
                // checks just give a clear message before playback starts.
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
                {
                    bail!("--capture path must end in .wav: {}", path.display());
                }
                if path.exists() {
                    bail!(
                        "--capture will not overwrite an existing file: {}",
                        path.display()
                    );
                }
                capture = Some(Some(path));
            }
            Some("--null") => capture = Some(None),
            Some("--fast") => fast = true,
            Some("--rate") => {
                rate = it
                    .next()
                    .and_then(|r| r.to_str())
                    .context("--rate needs a number")?
                    .parse()
                    .context("--rate needs a number")?;
            }
            Some("--help" | "-h") => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }
    if files.is_empty() {
        bail!("no files given\n\n{USAGE}");
    }
    if fast && capture.is_none() {
        bail!("--fast only works with --capture or --null");
    }

    let output = match capture {
        Some(path) => OutputKind::Capture {
            path,
            rate,
            channels: 2,
            realtime: !fast,
        },
        None => OutputKind::Device,
    };
    let queue = Arc::new(VecQueue::from_paths(files));
    let first = queue.first().context("empty queue")?;
    let player = Player::new(
        PlayerConfig {
            output,
            ..PlayerConfig::default()
        },
        queue.clone(),
    )?;
    player.play(first)?;

    let (lines_tx, lines) = mpsc::channel::<String>();
    if !fast {
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if lines_tx.send(line).is_err() {
                    break;
                }
            }
        });
    }

    let started = Instant::now();
    let mut last_entry = None;
    let mut last_print = Instant::now();
    loop {
        match lines.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                let mut words = line.split_whitespace();
                match words.next() {
                    Some("p") => player.toggle_pause(),
                    Some("n") => player.next()?,
                    Some("b") => player.previous()?,
                    Some("s") => match words.next().map(|w| w.parse::<f64>()) {
                        Some(Ok(secs)) if secs.is_finite() && secs >= 0.0 => {
                            player.seek(Duration::from_secs_f64(secs.min(1e9)))?;
                        }
                        _ => eprintln!("usage: s <seconds>"),
                    },
                    Some("v") => {
                        if let Some(v) = words.next().and_then(|w| w.parse::<f32>().ok()) {
                            player.set_volume(v);
                        }
                    }
                    Some("q") => break,
                    _ => eprintln!("{USAGE}"),
                }
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }

        let status = player.status();
        if status.state == PlaybackState::Stopped {
            break;
        }
        if status.entry != last_entry || last_print.elapsed() >= Duration::from_secs(1) {
            if status.entry != last_entry
                && let Some(id) = status.entry
            {
                println!("now playing: entry {}", id.0);
            }
            last_entry = status.entry;
            last_print = Instant::now();
            let fmt = |d: Duration| format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60);
            eprintln!(
                "  {:?} {} / {}  (output {} Hz x{}, underruns {}){}",
                status.state,
                fmt(status.position),
                status.duration.map_or_else(|| "?".into(), fmt),
                status.output_rate,
                status.output_channels,
                status.underruns,
                if status.output_failed {
                    "  OUTPUT FAILED"
                } else {
                    ""
                }
            );
        }
    }
    let status = player.status();
    println!(
        "done in {:.2?}; underruns: {}",
        started.elapsed(),
        status.underruns
    );
    Ok(())
}
