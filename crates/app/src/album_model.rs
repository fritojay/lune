//! The main pane's track list (`session::view`), one row per album so QML
//! can draw the cover beside the album's tracks. A track's place in the
//! view is its album's `firstIndex` plus its position in the album.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::HashMap;
use std::ops::Range;
use std::pin::Pin;
use std::rc::Rc;

use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QStringList, QVariant,
};
use lune_core::{Catalog, ListTrack, View};

use crate::{covers, format, list_model, session};

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
        #[qproperty(i32, album_count, cxx_name = "albumCount", READ, NOTIFY)]
        type AlbumListModel = super::AlbumListModelRust;
    }

    unsafe extern "RustQt" {
        /// Reloads from `session::view`.
        #[qinvokable]
        fn refresh(self: Pin<&mut AlbumListModel>);

        /// The row of the album holding view index `index`, or -1.
        #[qinvokable]
        #[cxx_name = "rowOf"]
        fn row_of(self: &AlbumListModel, index: i32) -> i32;

        #[qinvokable]
        #[cxx_override]
        fn data(self: &AlbumListModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &AlbumListModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &AlbumListModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut AlbumListModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut AlbumListModel>);

    }

    impl cxx_qt::Threading for AlbumListModel {}
    impl cxx_qt::Initialize for AlbumListModel {}
}

#[derive(Clone, Copy)]
#[repr(i32)]
enum Role {
    Title = 0x0100, // Qt::UserRole
    Artist,
    Year,
    Duration,
    Cover,
    FirstIndex,
    TrackCount,
    TrackNumbers,
    TrackTitles,
    TrackArtists,
    TrackDurations,
    TrackIds,
}

const ROLES: [(Role, &str); 12] = [
    (Role::Title, "albumTitle"),
    (Role::Artist, "albumArtist"),
    (Role::Year, "year"),
    (Role::Duration, "duration"),
    (Role::Cover, "cover"),
    (Role::FirstIndex, "firstIndex"),
    (Role::TrackCount, "trackCount"),
    (Role::TrackNumbers, "trackNumbers"),
    (Role::TrackTitles, "trackTitles"),
    (Role::TrackArtists, "trackArtists"),
    (Role::TrackDurations, "trackDurations"),
    (Role::TrackIds, "trackIds"),
];

#[derive(Default)]
pub struct AlbumListModelRust {
    album_count: i32,
    catalog: Option<Rc<Catalog>>,
    view: Rc<View>,
    /// The rows showing each cover, so a finished cover doesn't scan every album.
    cover_rows: HashMap<i64, Vec<i32>>,
}

impl cxx_qt::Initialize for qobject::AlbumListModel {
    fn initialize(self: Pin<&mut Self>) {
        covers::on_ready(self.qt_thread(), Self::cover_ready);
    }
}

impl qobject::AlbumListModel {
    fn refresh(mut self: Pin<&mut Self>) {
        let view = session::view();
        let count = i32::try_from(view.albums.len()).unwrap_or(i32::MAX);
        let catalog = session::catalog();
        let cover_rows = catalog
            .as_deref()
            .map(|catalog| cover_rows(catalog, &view))
            .unwrap_or_default();
        self.as_mut().begin_reset_model();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.catalog = catalog;
            rust.view = view;
            rust.cover_rows = cover_rows;
        }
        self.as_mut().end_reset_model();
        self.as_mut().rust_mut().album_count = count;
        self.album_count_changed();
    }

    fn row_of(&self, index: i32) -> i32 {
        let Ok(index) = usize::try_from(index) else {
            return -1;
        };
        // Albums are consecutive ranges in view order.
        let albums = &self.view.albums;
        let row = albums.partition_point(|album| album.end <= index);
        if albums.get(row).is_some_and(|a| a.contains(&index)) {
            i32::try_from(row).unwrap_or(-1)
        } else {
            -1
        }
    }

    fn cover_ready(self: Pin<&mut Self>, art_id: i64) {
        if let Some(rows) = self.cover_rows.get(&art_id).cloned() {
            list_model::role_changed(self.upcast_pin(), &rows, Role::Cover as i32);
        }
    }

    fn album(&self, row: usize) -> Vec<&ListTrack> {
        let (Some(catalog), Some(range)) = (&self.catalog, self.view.albums.get(row)) else {
            return Vec::new();
        };
        let Range { start, end } = range.clone();
        self.view.tracks[start..end]
            .iter()
            .filter_map(|&i| catalog.track(i))
            .collect()
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Ok(row) = usize::try_from(index.row()) else {
            return QVariant::default();
        };
        let Some(range) = self.view.albums.get(row) else {
            return QVariant::default();
        };
        let Some(first) = first_track(self.catalog.as_deref(), &self.view, row) else {
            return QVariant::default();
        };
        // Only the per-track roles need the whole album.
        let strings = |f: &dyn Fn(&ListTrack) -> String| -> QVariant {
            let list: QStringList = self
                .album(row)
                .iter()
                .map(|t| QString::from(&f(t)))
                .collect();
            QVariant::from(&list)
        };
        let Some(&(role, _)) = ROLES.iter().find(|(r, _)| *r as i32 == role) else {
            return QVariant::default();
        };
        match role {
            Role::Title => QVariant::from(&QString::from(&format::album(&first.album))),
            Role::Artist => QVariant::from(&QString::from(&format::artist(&first.album_artist))),
            Role::Year => QVariant::from(&QString::from(&format::year(first.album_year))),
            Role::Duration => QVariant::from(&QString::from(&total_duration(&self.album(row)))),
            Role::Cover => covers::url(shown_cover(&self.view, first)),
            Role::FirstIndex => QVariant::from(&i32::try_from(range.start).unwrap_or(i32::MAX)),
            Role::TrackCount => QVariant::from(&i32::try_from(range.len()).unwrap_or(i32::MAX)),
            Role::TrackNumbers => strings(&|t| {
                let multi_disc = self
                    .catalog
                    .as_deref()
                    .is_some_and(|c| c.is_multi_disc(t.album_id));
                format::track_number(t.disc_no, t.track_no, multi_disc)
            }),
            Role::TrackTitles => strings(&|t| t.title.clone()),
            Role::TrackArtists => strings(&|t| {
                // Only shown where it differs from the album artist.
                let artist = t.artist.as_deref().unwrap_or_default();
                if lune_core::normalize::key(artist) == t.album_artist_key {
                    String::new()
                } else {
                    format::artist(artist)
                }
            }),
            Role::TrackDurations => strings(&|t| format::duration(t.duration_ms)),
            Role::TrackIds => {
                let mut ids = QList::<i64>::default();
                for t in self.album(row) {
                    ids.append(t.id);
                }
                QVariant::from(&ids)
            }
        }
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            return 0;
        }
        i32::try_from(self.view.albums.len()).unwrap_or(i32::MAX)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        for (role, name) in ROLES {
            roles.insert(role as i32, QByteArray::from(name));
        }
        roles
    }
}

/// The first track of album `row` in `view`; its fields describe the album.
fn first_track<'a>(catalog: Option<&'a Catalog>, view: &View, row: usize) -> Option<&'a ListTrack> {
    let start = view.albums.get(row)?.start;
    catalog?.track(*view.tracks.get(start)?)
}

/// The length of the tracks shown, or empty when none of them has a known
/// length (lofty reports 0).
fn total_duration(tracks: &[&ListTrack]) -> String {
    let total: u64 = tracks.iter().map(|t| t.duration_ms).sum();
    if total == 0 {
        String::new()
    } else {
        format::long_duration(total)
    }
}

/// The cover of the album starting with `first`. A sorted view is a flat
/// track list without covers, so asking for one would only queue thumbnails
/// nobody sees.
fn shown_cover(view: &View, first: &ListTrack) -> Option<i64> {
    first.art_id.filter(|_| view.sort.is_none())
}

/// For each cover in `view`, the album rows that show it.
pub fn cover_rows(catalog: &Catalog, view: &View) -> HashMap<i64, Vec<i32>> {
    covers::rows_by_cover(
        (0..view.albums.len())
            .map(|row| first_track(Some(catalog), view, row).and_then(|t| shown_cover(view, t))),
    )
}

#[cfg(test)]
#[path = "tests/album_model_test.rs"]
mod tests;
