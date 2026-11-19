//! The app's settings, kept in a TOML file the user can also edit by hand.

use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{CoreError, Result};

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
    #[serde(deserialize_with = "scroll_speed_or_default")]
    pub scroll_speed: u32,
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
        let mut settings: Self = toml::from_str(&text).map_err(|err| CoreError::Settings {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        for key in settings.extra.keys() {
            tracing::warn!(key, "unknown setting is ignored");
        }
        settings.set_scroll_speed(settings.scroll_speed);
        Ok(Some(settings))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|err| CoreError::Settings {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        crate::paths::write_own_file(path, text.as_bytes())
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

/// A speed this version can't read falls back to the default rather than
/// making the whole file unreadable. Not done for `library_folders`: an
/// unreadable list must block saving so it can't overwrite the user's folders.
fn scroll_speed_or_default<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<u32, D::Error> {
    let value = toml::Value::deserialize(deserializer)?;
    Ok(value.try_into().unwrap_or_else(|err| {
        tracing::warn!(%err, "the scroll speed is ignored");
        DEFAULT_SCROLL_SPEED
    }))
}

#[cfg(test)]
#[path = "tests/settings_test.rs"]
mod tests;
