//! Logs to stderr, or without a console (a Windows release build) to a log
//! file, so a failure that would otherwise go unseen is recorded.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;

/// False exactly where `windows_subsystem = "windows"` applies (main.rs).
pub const HAS_CONSOLE: bool = !cfg!(all(windows, not(debug_assertions)));

/// Without a console, warnings are logged too unless `RUST_LOG` says
/// otherwise (it is awkward to set for a GUI app), and so are panics.
pub fn init() {
    if HAS_CONSOLE {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .init();
        return;
    }
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false);
    match open_first(&log_paths()) {
        Some(file) => subscriber.with_writer(Mutex::new(file)).init(),
        None => subscriber.init(),
    }
    std::panic::set_hook(Box::new(|info| tracing::error!("{info}")));
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
fn open_first(paths: &[PathBuf]) -> Option<File> {
    paths.iter().find_map(|path| open_log(path))
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
