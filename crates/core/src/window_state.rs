//! The main window's size, layout, and browse position from the last run,
//! so the next run opens the same way. Unlike [`Settings`](crate::Settings), the app rewrites
//! this file on every quit, so it lives apart from the hand-edited config.

use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{CoreError, FilterValue, Grouping, Result, Sort};

/// Smallest window a saved size can restore, so a bad value can't leave the
/// window too small to find and resize.
const MIN_WINDOW_WIDTH: u32 = 400;
const MIN_WINDOW_HEIGHT: u32 = 300;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    /// The size when not maximized, so leaving maximized restores it.
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
    pub filter_pane_width: u32,
    pub queue_pane_width: u32,
    pub queue_shown: bool,
    #[serde(deserialize_with = "browse_or_default")]
    pub browse: BrowseState,
}

/// What the main pane shows: the grouping, the filter value selected in it,
/// and the column sort (`None` is album order).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowseState {
    pub grouping: Grouping,
    #[serde(
        serialize_with = "serialize_filter",
        deserialize_with = "deserialize_filter"
    )]
    pub selected: FilterValue,
    pub sort: Option<Sort>,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 800,
            maximized: false,
            filter_pane_width: 250,
            queue_pane_width: 300,
            queue_shown: true,
            browse: BrowseState::default(),
        }
    }
}

impl WindowState {
    /// The defaults when there is no file yet. Fields missing from the file
    /// keep their defaults, and unknown ones are ignored, so a file from an
    /// older or newer version still restores what it can.
    pub fn load(path: &Path) -> Result<Self> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(CoreError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let state: Self = toml::from_str(&text).map_err(|err| CoreError::WindowState {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        Ok(Self {
            width: state.width.max(MIN_WINDOW_WIDTH),
            height: state.height.max(MIN_WINDOW_HEIGHT),
            ..state
        })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self).map_err(|err| CoreError::WindowState {
            path: path.to_path_buf(),
            message: err.to_string(),
        })?;
        crate::paths::write_own_file(path, text.as_bytes())
    }

    /// Fits a layout saved on a bigger screen, such as one since unplugged,
    /// to a screen of `available_width` by `available_height` (0 when
    /// unknown). If the side panes leave the main pane less than
    /// `main_pane_width`, they shrink in proportion. The queue pane counts
    /// even while hidden, so it still fits when shown.
    pub fn fitted(self, available_width: u32, available_height: u32, main_pane_width: u32) -> Self {
        let fit = |size: u32, available: u32| match available {
            0 => size,
            available => size.min(available),
        };
        let width = fit(self.width, available_width);
        let height = fit(self.height, available_height);
        let window_width = match (self.maximized, available_width) {
            (true, available) if available > 0 => available,
            _ => width,
        };
        let room = u64::from(window_width.saturating_sub(main_pane_width));
        let sides = u64::from(self.filter_pane_width) + u64::from(self.queue_pane_width);
        let shrink = |pane: u32| {
            if sides <= room {
                return pane;
            }
            u32::try_from(u64::from(pane) * room / sides).unwrap_or(pane)
        };
        Self {
            width,
            height,
            filter_pane_width: shrink(self.filter_pane_width),
            queue_pane_width: shrink(self.queue_pane_width),
            ..self
        }
    }
}

/// A browse section this version can't read, such as a grouping from a newer
/// one, falls back to the defaults without losing the window's size.
fn browse_or_default<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BrowseState, D::Error> {
    let value = toml::Value::deserialize(deserializer)?;
    Ok(value.try_into().unwrap_or_else(|err| {
        tracing::warn!(%err, "the saved browse position is ignored");
        BrowseState::default()
    }))
}

/// [`FilterValue`] as written to the file. TOML has no null, so an unknown
/// year is its own variant.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SavedFilter {
    All,
    AlbumArtist(String),
    Artist(String),
    Album(i64),
    Year(i32),
    UnknownYear,
}

fn serialize_filter<S: Serializer>(
    value: &FilterValue,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    match value.clone() {
        FilterValue::All => SavedFilter::All,
        FilterValue::AlbumArtist(key) => SavedFilter::AlbumArtist(key),
        FilterValue::Artist(key) => SavedFilter::Artist(key),
        FilterValue::Album(id) => SavedFilter::Album(id),
        FilterValue::Year(Some(year)) => SavedFilter::Year(year),
        FilterValue::Year(None) => SavedFilter::UnknownYear,
    }
    .serialize(serializer)
}

fn deserialize_filter<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<FilterValue, D::Error> {
    Ok(match SavedFilter::deserialize(deserializer)? {
        SavedFilter::All => FilterValue::All,
        SavedFilter::AlbumArtist(key) => FilterValue::AlbumArtist(key),
        SavedFilter::Artist(key) => FilterValue::Artist(key),
        SavedFilter::Album(id) => FilterValue::Album(id),
        SavedFilter::Year(year) => FilterValue::Year(Some(year)),
        SavedFilter::UnknownYear => FilterValue::Year(None),
    })
}

#[cfg(test)]
#[path = "tests/window_state_test.rs"]
mod tests;
