use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{CoreError, Result};

fn project_dirs() -> Result<directories::ProjectDirs> {
    directories::ProjectDirs::from("io.github", "fritojay", "Lune").ok_or(CoreError::NoHomeDir)
}

pub fn default_db_path() -> Result<PathBuf> {
    Ok(project_dirs()?.data_dir().join("library.sqlite3"))
}

pub fn default_thumbnail_dir() -> Result<PathBuf> {
    Ok(project_dirs()?.cache_dir().join("thumbnails"))
}

/// Beside the library's DB, because the saved browse position names its
/// albums and artists. The DB's name plus a suffix, like SQLite's own
/// `-wal` file, so it can never be the DB itself.
pub fn window_state_path(db_path: &Path) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(".window.toml");
    PathBuf::from(path)
}

pub fn default_config_path() -> Result<PathBuf> {
    Ok(project_dirs()?.config_dir().join("config.toml"))
}

/// Writes one of the app's own files (cache, config; never a music file),
/// creating its folder. It writes to a temporary name and renames, so a
/// reader never sees half a file.
#[expect(
    clippy::disallowed_methods,
    reason = "writes the app's own cache and config files, never a music file"
)]
pub(crate) fn write_own_file(target: &Path, bytes: &[u8]) -> Result<()> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let io_err = |path: &Path| {
        let path = path.to_path_buf();
        move |source| CoreError::Io { path, source }
    };
    let dir = target.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io_err(dir))?;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!(".tmp-{}-{n}", std::process::id()));
    std::fs::write(&temp, bytes).map_err(io_err(&temp))?;
    std::fs::rename(&temp, target).map_err(|err| {
        let _ = std::fs::remove_file(&temp);
        io_err(target)(err)
    })
}

#[cfg(test)]
#[path = "tests/paths_test.rs"]
mod tests;
