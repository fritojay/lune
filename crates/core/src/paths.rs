use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{CoreError, Result};

/// On Windows, a folder with this name beside the executable makes the
/// install portable: the library, settings, cache, and log all live in it
/// instead of the user's profile, so the app is self-contained. The Windows
/// zip ships with one. Elsewhere the app is installed by a package manager,
/// and a `data` folder beside it is someone else's.
const PORTABLE_DIR: &str = "data";

#[derive(Debug)]
struct Dirs {
    data: PathBuf,
    config: PathBuf,
    cache: PathBuf,
}

fn dirs() -> Result<Dirs> {
    let exe = if cfg!(windows) {
        std::env::current_exe().ok()
    } else {
        None
    };
    dirs_for(exe.as_deref())
}

/// The portable folders of `exe` if it has them, else the user's.
fn dirs_for(exe: Option<&Path>) -> Result<Dirs> {
    if let Some(dirs) = exe.and_then(portable_dirs) {
        return Ok(dirs);
    }
    let project = directories::ProjectDirs::from("io.github", "fritojay", "Lune")
        .ok_or(CoreError::NoHomeDir)?;
    Ok(Dirs {
        data: project.data_dir().to_path_buf(),
        config: project.config_dir().to_path_buf(),
        cache: project.cache_dir().to_path_buf(),
    })
}

/// The folders of a portable install of `exe`, if it is one.
fn portable_dirs(exe: &Path) -> Option<Dirs> {
    let dir = exe.parent()?.join(PORTABLE_DIR);
    dir.is_dir().then(|| Dirs {
        data: dir.clone(),
        config: dir.clone(),
        cache: dir.join("cache"),
    })
}

impl Dirs {
    fn db_path(&self) -> PathBuf {
        self.data.join("library.sqlite3")
    }

    fn thumbnail_dir(&self) -> PathBuf {
        self.cache.join("thumbnails")
    }

    fn config_path(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    fn log_path(&self) -> PathBuf {
        self.data.join("lune.log")
    }
}

pub fn default_db_path() -> Result<PathBuf> {
    Ok(dirs()?.db_path())
}

pub fn default_thumbnail_dir() -> Result<PathBuf> {
    Ok(dirs()?.thumbnail_dir())
}

/// Where the desktop app logs when it has no console (a Windows release
/// build).
pub fn default_log_path() -> Result<PathBuf> {
    Ok(dirs()?.log_path())
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
    Ok(dirs()?.config_path())
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
