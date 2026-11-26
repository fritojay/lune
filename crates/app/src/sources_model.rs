//! The grouping bar's list: every source the main pane can show, and which
//! one it shows.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QVariant};
use lune_core::Grouping;
use lune_core::Source as Shown;

use crate::list_model::{self, Role, text};
use crate::session::{self, Change};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        type QAbstractListModel = crate::list_model::QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        type SourcesModel = super::SourcesModelRust;
    }

    unsafe extern "RustQt" {
        #[qinvokable]
        fn choose(self: Pin<&mut SourcesModel>, row: i32);

        #[qinvokable]
        #[cxx_override]
        fn data(self: &SourcesModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &SourcesModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &SourcesModel) -> QHash_i32_QByteArray;
    }

    impl cxx_qt::Threading for SourcesModel {}
    impl cxx_qt::Initialize for SourcesModel {}
}

/// One button of the grouping bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Grouping(Grouping),
    Playlists,
}

impl Tab {
    /// What choosing it shows. A playlist source picks which playlist.
    fn source(self) -> Shown {
        match self {
            Self::Grouping(grouping) => Shown::Library(grouping),
            Self::Playlists => Shown::Playlist { id: None },
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Grouping(grouping) => match grouping {
                Grouping::AlbumArtist => "Album Artists",
                Grouping::Artist => "Artists",
                Grouping::Album => "Albums",
                Grouping::Year => "Years",
                Grouping::Genre => "Genres",
                Grouping::FileType => "File Types",
                Grouping::RecentlyAdded => "Recently Added",
                Grouping::RecentlyPlayed => "Recently Played",
            },
            Self::Playlists => "Playlists",
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Grouping(grouping) => grouping.name(),
            Self::Playlists => "playlists",
        }
    }
}

/// The kind of `source`, as QML names it: `FilterModel.sourceKind` and each
/// row's `kind`.
pub fn kind(source: Shown) -> &'static str {
    match source {
        Shown::Library(_) => "grouping",
        Shown::Playlist { .. } => "playlists",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Source {
    tab: Tab,
    /// Whether the main pane shows it.
    current: bool,
}

/// `kind` and `sourceId` are what QML passes back when a source is chosen.
/// `sourceId` is a grouping's `Grouping::name` or "playlists".
const ROLES: &[Role<Source>] = &[
    ("title", |s| text(s.tab.title())),
    ("kind", |s| text(kind(s.tab.source()))),
    ("sourceId", |s| text(s.tab.id())),
    ("current", |s| QVariant::from(&s.current)),
];

const CURRENT_ROLE: i32 = list_model::role_id(ROLES, "current");

fn sources(current: Shown) -> Vec<Source> {
    Grouping::ALL
        .into_iter()
        .map(Tab::Grouping)
        .chain([Tab::Playlists])
        .map(|tab| Source {
            tab,
            current: shows(tab.source(), current),
        })
        .collect()
}

/// Whether `current` belongs to the tab that shows `tab`: every playlist
/// belongs to the Playlists tab.
fn shows(tab: Shown, current: Shown) -> bool {
    match (tab, current) {
        (Shown::Playlist { .. }, Shown::Playlist { .. }) => true,
        _ => tab == current,
    }
}

/// The rows whose `current` differs between `old` and `new`.
fn changed_rows(old: &[Source], new: &[Source]) -> Vec<i32> {
    (0..)
        .zip(old.iter().zip(new))
        .filter(|(_, (old, new))| old != new)
        .map(|(row, _)| row)
        .collect()
}

pub struct SourcesModelRust {
    sources: Vec<Source>,
}

impl Default for SourcesModelRust {
    fn default() -> Self {
        Self {
            sources: sources(session::browse().source),
        }
    }
}

impl cxx_qt::Initialize for qobject::SourcesModel {
    fn initialize(self: Pin<&mut Self>) {
        session::on_change(self.qt_thread(), Change::Browse, Self::browsed);
    }
}

impl qobject::SourcesModel {
    fn browsed(self: Pin<&mut Self>) {
        self.show(session::browse().source);
    }

    /// Marks the chosen row current at once: the `Change::Browse` that the
    /// choice causes arrives queued, and until then the bar would still
    /// show the old source.
    fn choose(self: Pin<&mut Self>, row: i32) {
        let Some(source) = usize::try_from(row).ok().and_then(|r| self.sources.get(r)) else {
            return;
        };
        let shown = source.tab.source();
        self.show(shown);
    }

    fn show(mut self: Pin<&mut Self>, current: Shown) {
        let sources = sources(current);
        let changed = changed_rows(&self.sources, &sources);
        if changed.is_empty() {
            return;
        }
        self.as_mut().rust_mut().sources = sources;
        list_model::role_changed(self.upcast_pin(), &changed, CURRENT_ROLE);
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let source = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.sources.get(row));
        list_model::data(ROLES, source, role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.sources.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

#[cfg(test)]
#[path = "tests/sources_model_test.rs"]
mod tests;
