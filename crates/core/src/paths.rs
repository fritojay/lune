use std::io::Write;
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
    beside_db(db_path, ".window.toml")
}

fn beside_db(db_path: &Path, suffix: &str) -> PathBuf {
    let mut path = db_path.as_os_str().to_owned();
    path.push(suffix);
    PathBuf::from(path)
}

/// Links followed to a DB not made yet, as many as Linux follows: past
/// that they likely form a loop.
const MAX_LINKS: usize = 40;

/// `db_path` with links resolved, so every path to one library finds one
/// lock. The DB may not exist yet, even when `db_path` is a link to it; its
/// folder does.
fn resolved(db_path: &Path) -> PathBuf {
    let mut db_path = db_path.to_path_buf();
    for _ in 0..MAX_LINKS {
        if let Ok(path) = std::fs::canonicalize(&db_path) {
            return path;
        }
        let Ok(target) = std::fs::read_link(&db_path) else {
            break;
        };
        db_path = db_path
            .parent()
            .map_or_else(|| target.clone(), |dir| dir.join(&target));
    }
    let dir = db_path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match (std::fs::canonicalize(dir), db_path.file_name()) {
        (Ok(dir), Some(name)) => dir.join(name),
        _ => db_path.to_path_buf(),
    }
}

/// Held while this process uses the library; dropped, it lets another in.
/// Two apps on one library would each save their own playlists and queue
/// over the other's. The OS releases it when the process ends, even in a
/// crash. Not SQLite's own locking, which would also shut out this
/// process's other connections.
#[derive(Debug)]
pub struct LibraryLock {
    _file: std::fs::File,
}

/// Fails with [`CoreError::LibraryInUse`] while another holder has it.
#[expect(
    clippy::disallowed_methods,
    reason = "creates the app's own lock file beside its DB, never a music file"
)]
pub fn lock_library(db_path: &Path) -> Result<LibraryLock> {
    let dir = db_path.parent().filter(|dir| !dir.as_os_str().is_empty());
    if let Some(dir) = dir {
        std::fs::create_dir_all(dir).map_err(|source| CoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    let path = beside_db(&resolved(db_path), ".lock");
    let io_err = |source| CoreError::Io {
        path: path.clone(),
        source,
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(io_err)?;
    match file.try_lock() {
        Ok(()) => Ok(LibraryLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => {
            Err(CoreError::LibraryInUse(db_path.to_path_buf()))
        }
        Err(std::fs::TryLockError::Error(source)) => Err(io_err(source)),
    }
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
    // Synced before the rename, so a crash or power loss leaves the old file
    // or the new one, never an empty one (as some filesystems would).
    let written = std::fs::File::create(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    if let Err(err) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(io_err(&temp)(err));
    }
    std::fs::rename(&temp, target).map_err(|err| {
        let _ = std::fs::remove_file(&temp);
        io_err(target)(err)
    })?;
    // Makes the rename itself durable where a directory can be synced. On
    // Windows a directory can't be opened as a file, and this does nothing.
    if let Ok(dir) = std::fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/paths_test.rs"]
mod tests;
