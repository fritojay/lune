//! The main pane's track list (`session::view`), one row per album so QML
//! can draw the cover beside the album's tracks. A track's place in the
//! view is its album's `firstIndex` plus its position in the album. With
//! `oneAlbum` set, it shows only the view's rows of album `albumId`.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::ops::Range;
use std::pin::Pin;
use std::rc::Rc;

use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{
    QHash, QHashPair_i32_QByteArray, QMap, QMapPair_QString_QVariant, QModelIndex, QString,
    QStringList, QVariant,
};
use lune_core::{Catalog, ListTrack, SortColumn, View, file_type};

use crate::columns;
use crate::covers::{self, CoverRows};
use crate::format;
use crate::list_model::{self, text};
use crate::property::set;
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

/// Most tracks one row shows. An album is one delegate with a row per
/// track, so a huge one (such as untagged files, which all share one album)
/// would be built whole, and kept, as soon as any of it scrolls into view.
/// Even, so stripes carry on across an album's rows.
pub const MAX_ROW_TRACKS: usize = 64;

/// A model row: some of the tracks of one of the view's albums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    album: usize,
    /// Of the view's tracks.
    tracks: Range<usize>,
}

/// The part of an album of the view one row shows, as its roles read it.
pub struct Album<'a> {
    catalog: &'a Catalog,
    view: &'a View,
    /// The whole album, of the view's tracks.
    range: Range<usize>,
    part: Range<usize>,
    first: &'a ListTrack,
    /// The track playing, wherever it is.
    playing: Option<i64>,
    /// The columns `trackColumns` holds.
    columns: &'a [SortColumn],
}

impl Album<'_> {
    fn tracks(&self, range: Range<usize>) -> Vec<&ListTrack> {
        self.view.tracks[range]
            .iter()
            .filter_map(|&i| self.catalog.track(i))
            .collect()
    }

    fn shown(&self) -> Vec<&ListTrack> {
        self.tracks(self.part.clone())
    }

    fn is_header(&self) -> bool {
        self.part.start == self.range.start
    }

    /// Which of the tracks shown is playing, if one is.
    pub fn playing_index(&self) -> Option<usize> {
        let playing = self.playing?;
        self.shown().iter().position(|t| t.id == playing)
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
    ("duration", |a| {
        text(&total_duration(&a.tracks(a.range.clone())))
    }),
    ("cover", |a| {
        covers::url(shown_cover(a.view, a.first).filter(|_| a.is_header()))
    }),
    ("albumId", |a| QVariant::from(&a.first.album_id)),
    ("firstIndex", |a| {
        QVariant::from(&i32::try_from(a.part.start).unwrap_or(i32::MAX))
    }),
    ("trackCount", |a| {
        QVariant::from(&i32::try_from(a.part.len()).unwrap_or(i32::MAX))
    }),
    ("albumTrackCount", |a| {
        QVariant::from(&i32::try_from(a.range.len()).unwrap_or(i32::MAX))
    }),
    ("header", |a| QVariant::from(&a.is_header())),
    ("footer", |a| QVariant::from(&(a.part.end == a.range.end))),
    ("trackColumns", track_columns),
    // Of the tracks shown, or -1.
    ("playingIndex", |a| {
        let index = a.playing_index().and_then(|i| i32::try_from(i).ok());
        QVariant::from(&index.unwrap_or(-1))
    }),
];

const COVER_ROLE: i32 = list_model::role_id(ROLES, "cover");
const PLAYING_ROLE: i32 = list_model::role_id(ROLES, "playingIndex");
const TRACK_COLUMNS_ROLE: i32 = list_model::role_id(ROLES, "trackColumns");

pub struct AlbumListModelRust {
    album_count: i32,
    one_album: bool,
    album_id: i64,
    catalog: Option<Rc<Catalog>>,
    view: Rc<View>,
    rows: Vec<Row>,
    /// The rows showing each cover, so a finished cover doesn't scan every album.
    cover_rows: CoverRows,
    /// The track the rows mark as playing.
    playing: Option<i64>,
    shown_columns: Vec<SortColumn>,
}

impl Default for AlbumListModelRust {
    fn default() -> Self {
        Self {
            playing: session::now_playing(),
            shown_columns: shown_columns(),
            album_count: 0,
            one_album: false,
            album_id: -1,
            catalog: None,
            view: Rc::default(),
            rows: Vec::new(),
            cover_rows: CoverRows::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::AlbumListModel {
    fn initialize(self: Pin<&mut Self>) {
        covers::update_rows_on_ready(self.qt_thread(), |model| &model.cover_rows, COVER_ROLE);
        session::on_change(self.qt_thread(), Change::View, |model| model.refresh(true));
        session::on_change(self.qt_thread(), Change::ViewRefreshed, |model| {
            model.refresh(false);
        });
        session::on_change(self.qt_thread(), Change::NowPlaying, Self::now_playing);
        session::on_change(self.qt_thread(), Change::Columns, Self::columns_changed);
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
        let albums = rows.iter().filter(|row| is_header(&view, row)).count();
        let count = i32::try_from(albums).unwrap_or(i32::MAX);
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

    /// Marks the new track playing and unmarks the old, in only the rows
    /// that show them.
    fn now_playing(mut self: Pin<&mut Self>) {
        let playing = session::now_playing();
        let ids: Vec<i64> = [self.playing, playing].into_iter().flatten().collect();
        let rows = rows_showing(self.catalog.as_deref(), &self.view, &self.rows, &ids);
        self.as_mut().rust_mut().playing = playing;
        list_model::role_changed(self.upcast_pin(), &rows, PLAYING_ROLE);
    }

    fn columns_changed(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().shown_columns = shown_columns();
        let rows = self.rows.len();
        list_model::role_changed_everywhere(self.upcast_pin(), rows, TRACK_COLUMNS_ROLE);
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let album = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.rows.get(row))
            .and_then(|row| {
                row_at(
                    self.catalog.as_deref(),
                    &self.view,
                    row,
                    self.playing,
                    &self.shown_columns,
                )
            });
        list_model::data(ROLES, album.as_ref(), role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.rows.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

/// Album `album` of `view` whole, with no `trackColumns`.
pub fn album_at<'a>(
    catalog: Option<&'a Catalog>,
    view: &'a View,
    album: usize,
    playing: Option<i64>,
) -> Option<Album<'a>> {
    let range = view.albums.get(album)?.clone();
    row_at(
        catalog,
        view,
        &Row {
            album,
            tracks: range,
        },
        playing,
        &[],
    )
}

fn row_at<'a>(
    catalog: Option<&'a Catalog>,
    view: &'a View,
    row: &Row,
    playing: Option<i64>,
    columns: &'a [SortColumn],
) -> Option<Album<'a>> {
    let catalog = catalog?;
    Some(Album {
        catalog,
        view,
        range: view.albums.get(row.album)?.clone(),
        part: row.tracks.clone(),
        first: first_track(Some(catalog), view, row.album)?,
        playing,
        columns,
    })
}

/// The `rows` that show any of tracks `ids`.
pub fn rows_showing(catalog: Option<&Catalog>, view: &View, rows: &[Row], ids: &[i64]) -> Vec<i32> {
    (0..)
        .zip(rows)
        .filter(|(_, row)| shows(catalog, view, &row.tracks, ids))
        .map(|(index, _)| index)
        .collect()
}

/// The albums of `view` that hold any of tracks `ids`.
pub fn albums_showing(catalog: Option<&Catalog>, view: &View, ids: &[i64]) -> Vec<usize> {
    (0..view.albums.len())
        .filter(|&album| shows(catalog, view, &view.albums[album], ids))
        .collect()
}

/// Whether the view's `tracks` hold any of tracks `ids`.
fn shows(catalog: Option<&Catalog>, view: &View, tracks: &Range<usize>, ids: &[i64]) -> bool {
    let (Some(catalog), Some(tracks)) = (catalog, view.tracks.get(tracks.clone())) else {
        return false;
    };
    tracks
        .iter()
        .filter_map(|&i| catalog.track(i))
        .any(|t| ids.contains(&t.id))
}

fn is_header(view: &View, row: &Row) -> bool {
    view.albums
        .get(row.album)
        .is_some_and(|album| album.start == row.tracks.start)
}

/// The first track of album `row` in `view`; its fields describe the album.
fn first_track<'a>(catalog: Option<&'a Catalog>, view: &View, row: usize) -> Option<&'a ListTrack> {
    let start = view.albums.get(row)?.start;
    catalog?.track(*view.tracks.get(start)?)
}

/// Each shown column's text for each track, by column name.
fn track_columns(album: &Album<'_>) -> QVariant {
    let tracks = album.shown();
    let mut columns = QMap::<QMapPair_QString_QVariant>::default();
    for &column in album.columns {
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
        SortColumn::FileType => file_type(&track.codec),
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

fn shown_columns() -> Vec<SortColumn> {
    columns::shown(&session::hidden_columns())
        .map(|column| column.column)
        .collect()
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

/// The cover of the album starting with `first`. A flat view is a track
/// list without covers, so asking for one would only queue thumbnails
/// nobody sees.
fn shown_cover(view: &View, first: &ListTrack) -> Option<i64> {
    first.art_id.filter(|_| !view.flat)
}

/// The rows of `view`'s albums: with `one_album`, album `album_id` (none
/// for -1) whole, otherwise every album split into parts of at most
/// [`MAX_ROW_TRACKS`]. One album is shown in a column that makes all of its
/// rows at once, so splitting it gains nothing, and closing it mid-build made
/// Qt warn about the parts it had yet to make.
fn shown_rows(catalog: Option<&Catalog>, view: &View, one_album: bool, album_id: i64) -> Vec<Row> {
    let shown = |album: usize| {
        !one_album || first_track(catalog, view, album).is_some_and(|t| t.album_id == album_id)
    };
    let part = if one_album {
        usize::MAX
    } else {
        MAX_ROW_TRACKS
    };
    (0..view.albums.len())
        .filter(|&album| shown(album))
        .flat_map(|album| {
            let range = view.albums[album].clone();
            range.clone().step_by(part).map(move |start| Row {
                album,
                tracks: start..range.end.min(start.saturating_add(part)),
            })
        })
        .collect()
}

pub fn album_cover(catalog: &Catalog, view: &View, album: usize) -> Option<i64> {
    first_track(Some(catalog), view, album).and_then(|t| shown_cover(view, t))
}

/// For each cover, the `rows` that show it: the first of each album's.
fn cover_rows(catalog: &Catalog, view: &View, rows: &[Row]) -> CoverRows {
    covers::rows_by_cover(rows.iter().map(|row| {
        is_header(view, row)
            .then(|| album_cover(catalog, view, row.album))
            .flatten()
    }))
}

#[cfg(test)]
#[path = "tests/album_model_test.rs"]
mod tests;
