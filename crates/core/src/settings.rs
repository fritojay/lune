//! The app's settings, kept in a TOML file the user can also edit by hand.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{CoreError, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub library_folders: Vec<PathBuf>,
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
        toml::from_str(&text)
            .map(Some)
            .map_err(|err| CoreError::Settings {
                path: path.to_path_buf(),
                message: err.to_string(),
            })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|err| CoreError::Settings {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        crate::paths::write_own_file(path, text.as_bytes())
    }

    /// Adds `folder` unless it is already covered: the same folder, or one
    /// inside a listed folder. Listed folders inside the new one are dropped,
    /// since it covers them. Returns whether anything changed.
    pub fn add_folder(&mut self, folder: PathBuf) -> bool {
        if self.library_folders.iter().any(|f| folder.starts_with(f)) {
            return false;
        }
        self.library_folders.retain(|f| !f.starts_with(&folder));
        self.library_folders.push(folder);
        true
    }

    pub fn remove_folder(&mut self, folder: &Path) -> bool {
        let before = self.library_folders.len();
        self.library_folders.retain(|f| f != folder);
        self.library_folders.len() != before
    }
}

#[cfg(test)]
#[path = "tests/settings_test.rs"]
mod tests;
