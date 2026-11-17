//! This crate must stay free of Qt types and platform-specific (`std::os::*`) APIs.
//! Music files are only ever opened read-only.

mod art;
mod browse;
mod db;
mod error;
mod library;
pub mod normalize;
mod paths;
mod query;
mod queue;
mod scan;
mod settings;
#[cfg(feature = "synthetic")]
mod synthetic;
mod tags;
#[cfg(test)]
#[path = "tests/test_util.rs"]
mod test_util;
mod watch;
mod window_state;

pub use browse::{Catalog, Filter, FilterValue, Grouping, Sort, SortColumn, View};
pub use error::{CoreError, Result};
pub use library::Library;
pub use paths::{default_config_path, default_db_path, default_thumbnail_dir, window_state_path};
pub use query::{AlbumRow, LibraryStats, ListTrack, MissingTrack, TrackRow};
pub use queue::{Queue, QueueItem, SlotId};
pub use scan::{ScanMode, ScanProgress, ScanReport};
pub use settings::{DEFAULT_SCROLL_SPEED, SCROLL_SPEEDS, Settings, clamp_scroll_speed};
pub use tags::{EmbeddedArt, TrackMeta, is_audio_path, read_track_meta};
pub use watch::FolderWatcher;
pub use window_state::{BrowseState, WindowState};
