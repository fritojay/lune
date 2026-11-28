//! The app's settings, kept in a TOML file the user can also edit by hand.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result};

const FOLDERS_KEY: &str = "library_folders";

pub const SCROLL_SPEEDS: RangeInclusive<u32> = 1..=10;
pub const DEFAULT_SCROLL_SPEED: u32 = 3;

pub fn clamp_scroll_speed(speed: u32) -> u32 {
    speed.clamp(*SCROLL_SPEEDS.start(), *SCROLL_SPEEDS.end())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub library_folders: Vec<PathBuf>,
    /// Multiplies how far one mouse wheel step scrolls a list. Always within
    /// [`SCROLL_SPEEDS`]; an out-of-range value in the file is clamped on load.
    pub scroll_speed: u32,
    /// The audio output the user chose, as the engine names it; `None` is
    /// the system default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_device: Option<String>,
    /// Off: each track plays out to its end before the next one is opened,
    /// which leaves a short gap between them.
    pub gapless: bool,
    /// Whether the file lists the library folders. One that doesn't (left
    /// empty by a crash, or edited by hand) leaves the library as it is
    /// rather than pruning it to no folders, and is saved without the list
    /// until a folder is set.
    #[serde(skip)]
    pub folders_listed: bool,
    /// Keys this version doesn't know, such as a newer version's settings or
    /// a typo, kept so saving writes them back.
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library_folders: Vec::new(),
            scroll_speed: DEFAULT_SCROLL_SPEED,
            output_device: None,
            gapless: true,
            folders_listed: true,
            extra: toml::Table::new(),
        }
    }
}

impl Settings {
    /// `None` when there is no settings file yet.
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(CoreError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let error = |err: toml::de::Error| CoreError::Settings {
            path: path.to_path_buf(),
            message: err.to_string(),
        };
        // An unreadable folder list must block saving, so it can't
        // overwrite the user's folders.
        let table: toml::Table = toml::from_str(&text).map_err(error)?;
        let folders_listed = table.contains_key(FOLDERS_KEY);
        let mut settings: Self =
            crate::lenient::from_table(table, &[FOLDERS_KEY]).map_err(error)?;
        settings.folders_listed = folders_listed;
        for key in settings.extra.keys() {
            tracing::warn!(key, "unknown setting is ignored");
        }
        settings.set_scroll_speed(settings.scroll_speed);
        Ok(Some(settings))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let error = |message: String| CoreError::Settings {
            path: path.to_path_buf(),
            message,
        };
        let mut table = toml::Table::try_from(self).map_err(|err| error(err.to_string()))?;
        if !self.folders_listed && self.library_folders.is_empty() {
            table.remove(FOLDERS_KEY);
        }
        let text = toml::to_string_pretty(&table).map_err(|err| error(err.to_string()))?;
        crate::paths::write_own_file(path, text.as_bytes())
    }

    /// Returns whether they changed.
    pub fn set_library_folders(&mut self, folders: &[PathBuf]) -> bool {
        if self.library_folders == folders {
            return false;
        }
        folders.clone_into(&mut self.library_folders);
        self.folders_listed = true;
        true
    }

    /// Clamps `speed` into [`SCROLL_SPEEDS`]. Returns whether it changed.
    pub fn set_scroll_speed(&mut self, speed: u32) -> bool {
        let speed = clamp_scroll_speed(speed);
        let changed = self.scroll_speed != speed;
        self.scroll_speed = speed;
        changed
    }
}

/// Adds `folder` to `folders` unless it is already covered: the same folder,
/// or one inside a listed folder. Listed folders inside the new one are
/// dropped, since it covers them. Returns whether anything changed.
pub fn add_folder(folders: &mut Vec<PathBuf>, folder: PathBuf) -> bool {
    if folders.iter().any(|f| folder.starts_with(f)) {
        return false;
    }
    folders.retain(|f| !f.starts_with(&folder));
    folders.push(folder);
    true
}

pub fn remove_folder(folders: &mut Vec<PathBuf>, folder: &Path) -> bool {
    let before = folders.len();
    folders.retain(|f| f != folder);
    folders.len() != before
}

#[cfg(test)]
#[path = "tests/settings_test.rs"]
mod tests;
