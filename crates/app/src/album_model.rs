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
    QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QStringList, QVariant,
};
use lune_core::{Catalog, ListTrack, View};

use crate::list_model::{self, text};
use crate::property::set;
use crate::{covers, format, session};

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

/// An album of the view, as its roles read it.
struct Album<'a> {
    catalog: &'a Catalog,
    view: &'a View,
    range: Range<usize>,
    first: &'a ListTrack,
}

impl Album<'_> {
    fn tracks(&self) -> Vec<&ListTrack> {
        self.view.tracks[self.range.clone()]
            .iter()
            .filter_map(|&i| self.catalog.track(i))
            .collect()
    }

    /// One string per track.
    fn strings(&self, f: impl Fn(&ListTrack) -> String) -> QVariant {
        let list: QStringList = self
            .tracks()
            .into_iter()
            .map(|t| QString::from(&f(t)))
            .collect();
        QVariant::from(&list)
    }
}

/// Not [`list_model::Role`], whose row type can't borrow for any lifetime.
type Role = (&'static str, fn(&Album<'_>) -> QVariant);

const ROLES: &[Role] = &[
    ("albumTitle", |a| text(&format::album(&a.first.album))),
    ("albumArtist", |a| {
        text(&format::artist(&a.first.album_artist))
    }),
    ("year", |a| text(&format::year(a.first.album_year))),
    ("duration", |a| text(&total_duration(&a.tracks()))),
    ("cover", |a| covers::url(shown_cover(a.view, a.first))),
    ("firstIndex", |a| {
        QVariant::from(&i32::try_from(a.range.start).unwrap_or(i32::MAX))
    }),
    ("trackCount", |a| {
        QVariant::from(&i32::try_from(a.range.len()).unwrap_or(i32::MAX))
    }),
    ("trackNumbers", |a| {
        a.strings(|t| {
            let multi_disc = a.catalog.is_multi_disc(t.album_id);
            format::track_number(t.disc_no, t.track_no, multi_disc)
        })
    }),
    ("trackTitles", |a| a.strings(|t| t.title.clone())),
    ("trackArtists", |a| a.strings(track_artist)),
    ("trackDurations", |a| {
        a.strings(|t| format::duration(t.duration_ms))
    }),
    ("trackIds", |a| {
        let mut ids = QList::<i64>::default();
        for t in a.tracks() {
            ids.append(t.id);
        }
        QVariant::from(&ids)
    }),
    ("trackFileTypes", |a| {
        a.strings(|t| format::file_type(&t.codec))
    }),
    ("trackBitRates", |a| {
        a.strings(|t| format::bit_rate(t.bitrate_kbps))
    }),
];

const COVER_ROLE: i32 = list_model::role_id(ROLES, "cover");

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
        set!(self, album_count, album_count_changed, count);
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
            list_model::role_changed(self.upcast_pin(), &rows, COVER_ROLE);
        }
    }

    fn album(&self, row: usize) -> Option<Album<'_>> {
        let catalog = self.catalog.as_deref()?;
        Some(Album {
            catalog,
            view: &self.view,
            range: self.view.albums.get(row)?.clone(),
            first: first_track(Some(catalog), &self.view, row)?,
        })
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let album = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.album(row));
        list_model::data(ROLES, album.as_ref(), role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.view.albums.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

/// The first track of album `row` in `view`; its fields describe the album.
fn first_track<'a>(catalog: Option<&'a Catalog>, view: &View, row: usize) -> Option<&'a ListTrack> {
    let start = view.albums.get(row)?.start;
    catalog?.track(*view.tracks.get(start)?)
}

/// The track's artist, else its album's.
fn track_artist(track: &ListTrack) -> String {
    match track.artist.as_deref() {
        Some(artist) if !artist.trim().is_empty() => format::artist(artist),
        _ => format::artist(&track.album_artist),
    }
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
