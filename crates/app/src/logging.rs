//! Logs to the console when there is one, and always to a log file, so a
//! bug report has the log even when the app was started from the desktop.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// False exactly where `windows_subsystem = "windows"` applies (main.rs).
pub const HAS_CONSOLE: bool = !cfg!(all(windows, not(debug_assertions)));

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// The log file this run writes, if one could be opened.
pub fn log_path() -> Option<&'static Path> {
    LOG_PATH.get().map(PathBuf::as_path)
}

/// Warnings are logged too unless `RUST_LOG` says otherwise (it is awkward
/// to set for a GUI app), and so are panics.
pub fn init() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();
    let file = open_first(&log_paths()).map(|(path, file)| {
        let _ = LOG_PATH.set(path);
        fmt::layer().with_ansi(false).with_writer(Mutex::new(file))
    });
    let console = HAS_CONSOLE.then(fmt::layer);
    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(file)
        .init();
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::capture();
        match backtrace.status() {
            std::backtrace::BacktraceStatus::Captured => tracing::error!("{info}\n{backtrace}"),
            _ => tracing::error!("{info}"),
        }
    }));
}

/// The app's own log, then one in the temp folder for when the app's folder
/// can't be written (a portable install unzipped into Program Files, say) or
/// another running instance has the first.
fn log_paths() -> Vec<PathBuf> {
    lune_core::default_log_path()
        .ok()
        .into_iter()
        .chain([std::env::temp_dir().join("lune.log")])
        .collect()
}

/// The first of `paths` that can be created and isn't another running
/// instance's log, emptied so it only holds this run.
fn open_first(paths: &[PathBuf]) -> Option<(PathBuf, File)> {
    paths
        .iter()
        .find_map(|path| Some((path.clone(), open_log(path)?)))
}

#[expect(
    clippy::disallowed_methods,
    reason = "opens the app's own log file, never a music file"
)]
fn open_log(path: &Path) -> Option<File> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    // Not truncated on open: another instance's log is only emptied once we
    // know it isn't in use.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .ok()?;
    file.try_lock().ok()?;
    file.set_len(0).ok()?;
    Some(file)
}

#[cfg(test)]
#[path = "tests/logging_test.rs"]
mod tests;
