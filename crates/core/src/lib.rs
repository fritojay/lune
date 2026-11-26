//! This crate must stay free of Qt types and platform-specific (`std::os::*`) APIs.
//! Music files are only ever opened read-only.

mod art;
mod browse;
mod db;
mod error;
mod lenient;
mod library;
mod lists;
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

pub use browse::{
    Browsed, Catalog, Field, Filter, FilterValue, Grouping, MenuChoice, Sort, SortColumn, Source,
    TrackFilter, View, after_heading_click, file_type, menu_choice, sort_state,
};
pub use error::{CoreError, Result};
pub use library::Library;
pub use lists::{ListId, Playlist};
pub use paths::{
    LibraryLock, default_config_path, default_db_path, default_log_path, default_thumbnail_dir,
    lock_library, window_state_path,
};
pub use query::{AlbumRow, LibraryStats, ListTrack, MissingTrack};
pub use queue::{Queue, QueueItem, SlotId, TrackRef};
pub use scan::{ScanMode, ScanProgress, ScanReport};
pub use settings::{
    DEFAULT_SCROLL_SPEED, SCROLL_SPEEDS, Settings, add_folder, clamp_scroll_speed, remove_folder,
};
pub use tags::{EmbeddedArt, TrackMeta, is_audio_path, read_track_meta};
pub use watch::{FolderWatcher, collapse_nested};
pub use window_state::{BrowseState, WindowState};
