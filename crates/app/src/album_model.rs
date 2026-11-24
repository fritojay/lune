//! The main pane's track list (`session::view`), one row per album so QML
//! can draw the cover beside the album's tracks. A track's place in the
//! view is its album's `firstIndex` plus its position in the album. With
//! `oneAlbum` set, it shows only the view's rows of album `albumId`.

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
    QHash, QHashPair_i32_QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString,
    QStringList, QVariant,
};
use lune_core::{Catalog, ListTrack, SortColumn, View};

use crate::list_model::{self, text};
use crate::property::set;
use crate::session::{self, Change};
use crate::{covers, format};

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
        #[qproperty(bool, one_album, cxx_name = "oneAlbum", READ, WRITE = set_one_album, NOTIFY)]
        /// With `oneAlbum`, the album shown; -1 shows none.
        #[qproperty(i64, album_id, cxx_name = "albumId", READ, WRITE = set_album_id, NOTIFY)]
        type AlbumListModel = super::AlbumListModelRust;
    }

    unsafe extern "RustQt" {
        /// Reloaded from `session::view`. `reset` is false when the list
        /// should keep its position (a rescan).
        #[qsignal]
        fn refreshed(self: Pin<&mut AlbumListModel>, reset: bool);

        fn set_one_album(self: Pin<&mut AlbumListModel>, one_album: bool);

        fn set_album_id(self: Pin<&mut AlbumListModel>, album_id: i64);

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
pub struct Album<'a> {
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
}

/// Not [`list_model::Role`], whose row type can't borrow for any lifetime.
pub type Role = (&'static str, fn(&Album<'_>) -> QVariant);

pub const ROLES: &[Role] = &[
    ("albumTitle", |a| text(&format::album(&a.first.album))),
    ("albumArtist", |a| {
        text(&format::artist(&a.first.album_artist))
    }),
    ("year", |a| text(&format::year(a.first.album_year))),
    ("genre", |a| {
        text(a.first.album_genre.as_deref().unwrap_or_default())
    }),
    ("duration", |a| text(&total_duration(&a.tracks()))),
    ("cover", |a| covers::url(shown_cover(a.view, a.first))),
    ("albumId", |a| QVariant::from(&a.first.album_id)),
    ("firstIndex", |a| {
        QVariant::from(&i32::try_from(a.range.start).unwrap_or(i32::MAX))
    }),
    ("trackCount", |a| {
        QVariant::from(&i32::try_from(a.range.len()).unwrap_or(i32::MAX))
    }),
    ("trackColumns", track_columns),
    ("trackIds", |a| {
        let mut ids = QList::<i64>::default();
        for t in a.tracks() {
            ids.append(t.id);
        }
        QVariant::from(&ids)
    }),
];

const COVER_ROLE: i32 = list_model::role_id(ROLES, "cover");

pub struct AlbumListModelRust {
    album_count: i32,
    one_album: bool,
    album_id: i64,
    catalog: Option<Rc<Catalog>>,
    view: Rc<View>,
    /// The view's album indices, one per row.
    rows: Vec<usize>,
    /// The rows showing each cover, so a finished cover doesn't scan every album.
    cover_rows: HashMap<i64, Vec<i32>>,
}

impl Default for AlbumListModelRust {
    fn default() -> Self {
        Self {
            album_count: 0,
            one_album: false,
            album_id: -1,
            catalog: None,
            view: Rc::default(),
            rows: Vec::new(),
            cover_rows: HashMap::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::AlbumListModel {
    fn initialize(self: Pin<&mut Self>) {
        covers::on_ready(self.qt_thread(), Self::cover_ready);
        session::on_change(self.qt_thread(), |model, change| {
            if let Change::View { reset } = change {
                model.refresh(reset);
            }
        });
    }
}

impl qobject::AlbumListModel {
    fn set_one_album(mut self: Pin<&mut Self>, one_album: bool) {
        if self.one_album != one_album {
            self.as_mut().rust_mut().one_album = one_album;
            self.as_mut().one_album_changed();
            self.refresh(true);
        }
    }

    fn set_album_id(mut self: Pin<&mut Self>, album_id: i64) {
        if self.album_id != album_id {
            self.as_mut().rust_mut().album_id = album_id;
            self.as_mut().album_id_changed();
            self.refresh(true);
        }
    }

    fn refresh(mut self: Pin<&mut Self>, reset: bool) {
        let view = session::view();
        let catalog = session::catalog();
        let rows = shown_rows(catalog.as_deref(), &view, self.one_album, self.album_id);
        let count = i32::try_from(rows.len()).unwrap_or(i32::MAX);
        let cover_rows = catalog
            .as_deref()
            .map(|catalog| cover_rows(catalog, &view, &rows))
            .unwrap_or_default();
        self.as_mut().begin_reset_model();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.catalog = catalog;
            rust.view = view;
            rust.rows = rows;
            rust.cover_rows = cover_rows;
        }
        self.as_mut().end_reset_model();
        set!(self, album_count, album_count_changed, count);
        self.refreshed(reset);
    }

    fn cover_ready(self: Pin<&mut Self>, art_id: i64) {
        if let Some(rows) = self.cover_rows.get(&art_id).cloned() {
            list_model::role_changed(self.upcast_pin(), &rows, COVER_ROLE);
        }
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let album = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.rows.get(row))
            .and_then(|&album| album_at(self.catalog.as_deref(), &self.view, album));
        list_model::data(ROLES, album.as_ref(), role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.rows.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

pub fn album_at<'a>(
    catalog: Option<&'a Catalog>,
    view: &'a View,
    album: usize,
) -> Option<Album<'a>> {
    let catalog = catalog?;
    Some(Album {
        catalog,
        view,
        range: view.albums.get(album)?.clone(),
        first: first_track(Some(catalog), view, album)?,
    })
}

/// The first track of album `row` in `view`; its fields describe the album.
fn first_track<'a>(catalog: Option<&'a Catalog>, view: &View, row: usize) -> Option<&'a ListTrack> {
    let start = view.albums.get(row)?.start;
    catalog?.track(*view.tracks.get(start)?)
}

/// Each column's text for each track, by column name.
fn track_columns(album: &Album<'_>) -> QVariant {
    let tracks = album.tracks();
    let mut columns = QMap::<QMapPair_QString_QVariant>::default();
    for column in SortColumn::ALL {
        let texts: QStringList = tracks
            .iter()
            .map(|t| QString::from(&column_text(column, t, album.catalog)))
            .collect();
        columns.insert(QString::from(column.name()), QVariant::from(&texts));
    }
    QVariant::from(&columns)
}

fn column_text(column: SortColumn, track: &ListTrack, catalog: &Catalog) -> String {
    match column {
        SortColumn::TrackNumber => format::track_number(
            track.disc_no,
            track.track_no,
            catalog.is_multi_disc(track.album_id),
        ),
        SortColumn::Title => track.title.clone(),
        SortColumn::Artist => track_artist(track),
        SortColumn::Duration => format::duration(track.duration_ms),
        SortColumn::FileType => format::file_type(&track.codec),
        SortColumn::BitRate => format::bit_rate(track.bitrate_kbps),
    }
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

/// The indices of `view`'s albums: with `one_album`, those whose album id
/// is `album_id` (none for -1), otherwise all of them.
fn shown_rows(
    catalog: Option<&Catalog>,
    view: &View,
    one_album: bool,
    album_id: i64,
) -> Vec<usize> {
    if !one_album {
        return (0..view.albums.len()).collect();
    }
    (0..view.albums.len())
        .filter(|&album| first_track(catalog, view, album).is_some_and(|t| t.album_id == album_id))
        .collect()
}

/// For each cover of `view`'s albums at `rows`, the rows that show it.
pub fn cover_rows(catalog: &Catalog, view: &View, rows: &[usize]) -> HashMap<i64, Vec<i32>> {
    covers::rows_by_cover(
        rows.iter().map(|&album| {
            first_track(Some(catalog), view, album).and_then(|t| shown_cover(view, t))
        }),
    )
}

#[cfg(test)]
#[path = "tests/album_model_test.rs"]
mod tests;
