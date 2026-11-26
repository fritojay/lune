//! The main pane's albums (`session::view`) as a grid: each row holds up to
//! `columns` albums, so QML can open an album's tracks between two rows.

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
    QHash, QHashPair_i32_QByteArray, QList, QMap, QMapPair_QString_QVariant, QModelIndex, QString,
    QVariant,
};
use lune_core::{Catalog, View};

use crate::album_model::{self, Album, album_at};
use crate::covers::{self, CoverRows};
use crate::list_model;
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
        #[qproperty(i32, columns, READ, WRITE = set_columns, NOTIFY)]
        /// Off while the grid is hidden, so it holds no albums and skips
        /// reloading on every view change.
        #[qproperty(bool, active, READ, WRITE = set_active, NOTIFY)]
        /// The album whose tracks show between two rows, or -1. It closes
        /// when the grid is reset or the album leaves the view.
        #[qproperty(i64, open_album_id, cxx_name = "openAlbumId", READ, NOTIFY)]
        type AlbumGridModel = super::AlbumGridModelRust;
    }

    unsafe extern "RustQt" {
        /// Reloaded from `session::view`, or relaid for new `columns`.
        /// `reset` is false when the grid should keep its place. `columns`
        /// still has its old value at `modelAboutToBeReset`.
        #[qsignal]
        fn refreshed(self: Pin<&mut AlbumGridModel>, reset: bool);

        fn set_columns(self: Pin<&mut AlbumGridModel>, columns: i32);

        fn set_active(self: Pin<&mut AlbumGridModel>, active: bool);

        /// The row that now holds the first album of row `row` of a grid
        /// that was `columns` wide.
        /// Opens album `album_id`, or closes it if it is open.
        #[qinvokable]
        #[cxx_name = "toggleAlbum"]
        fn toggle_album(self: Pin<&mut AlbumGridModel>, album_id: i64);

        #[qinvokable]
        #[cxx_name = "closeAlbum"]
        fn close_album(self: Pin<&mut AlbumGridModel>);

        #[qinvokable]
        #[cxx_name = "relaidRow"]
        fn relaid_row(self: &AlbumGridModel, row: i32, columns: i32) -> i32;

        #[qinvokable]
        #[cxx_override]
        fn data(self: &AlbumGridModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &AlbumGridModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &AlbumGridModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut AlbumGridModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut AlbumGridModel>);
    }

    impl cxx_qt::Threading for AlbumGridModel {}
    impl cxx_qt::Initialize for AlbumGridModel {}
}

/// The roles of [`album_model::ROLES`] a tile shows. The track columns are
/// left out: building them for every album in view would be wasted.
const TILE_ROLES: [&str; 7] = [
    "albumTitle",
    "albumArtist",
    "cover",
    "albumId",
    "firstIndex",
    "trackCount",
    "trackIds",
];

/// A grid row: its albums' indices in the view.
type GridRow = Range<usize>;

type Role = (&'static str, fn(&Tiles<'_>) -> QVariant);

/// `openTile` is the open album's place in the row, or -1.
const ROLES: &[Role] = &[
    ("albums", |t| t.maps()),
    ("openTile", |t| QVariant::from(&t.open_tile)),
];

const ALBUMS_ROLE: i32 = list_model::role_id(ROLES, "albums");
const OPEN_TILE_ROLE: i32 = list_model::role_id(ROLES, "openTile");

struct Tiles<'a> {
    catalog: Option<&'a Catalog>,
    view: &'a View,
    albums: GridRow,
    open_tile: i32,
}

impl Tiles<'_> {
    fn maps(&self) -> QVariant {
        let mut tiles = QList::<QVariant>::default();
        for album in self.albums.clone() {
            if let Some(album) = album_at(self.catalog, self.view, album) {
                tiles.append(tile(&album));
            }
        }
        QVariant::from(&tiles)
    }
}

fn tile(album: &Album<'_>) -> QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    for (name, value) in album_model::ROLES {
        if TILE_ROLES.contains(name) {
            map.insert(QString::from(*name), value(album));
        }
    }
    QVariant::from(&map)
}

pub struct AlbumGridModelRust {
    columns: i32,
    active: bool,
    catalog: Option<Rc<Catalog>>,
    view: Rc<View>,
    cover_rows: CoverRows,
    open_album_id: i64,
    /// The open album, of the view's albums.
    open: Option<usize>,
}

impl Default for AlbumGridModelRust {
    fn default() -> Self {
        Self {
            columns: 1,
            active: false,
            open_album_id: -1,
            open: None,
            catalog: None,
            view: Rc::default(),
            cover_rows: CoverRows::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::AlbumGridModel {
    fn initialize(self: Pin<&mut Self>) {
        covers::update_rows_on_ready(self.qt_thread(), |model| &model.cover_rows, ALBUMS_ROLE);
        session::on_change(self.qt_thread(), |model, change| {
            if let Change::View { reset } = change {
                model.refresh(reset);
            }
        });
    }
}

impl qobject::AlbumGridModel {
    fn set_columns(self: Pin<&mut Self>, columns: i32) {
        let columns = columns.max(1);
        if self.columns != columns {
            let active = self.active;
            self.reload(columns, active, false);
        }
    }

    fn set_active(self: Pin<&mut Self>, active: bool) {
        if self.active != active {
            let columns = self.columns;
            self.reload(columns, active, true);
        }
    }

    fn refresh(self: Pin<&mut Self>, reset: bool) {
        if self.active {
            let columns = self.columns;
            self.reload(columns, true, reset);
        }
    }

    fn reload(mut self: Pin<&mut Self>, columns: i32, active: bool, reset: bool) {
        let (view, catalog) = if active {
            (session::view(), session::catalog())
        } else {
            (Rc::default(), None)
        };
        let cover_rows = catalog
            .as_deref()
            .map(|catalog| grid_cover_rows(catalog, &view, width(columns)))
            .unwrap_or_default();
        let open = still_open(catalog.as_deref(), &view, self.open_id(), reset);
        self.as_mut().begin_reset_model();
        let old = (self.columns, self.active, self.open_album_id);
        {
            let mut rust = self.as_mut().rust_mut();
            rust.columns = columns;
            rust.active = active;
            rust.catalog = catalog;
            rust.view = view;
            rust.cover_rows = cover_rows;
            rust.open = open;
            rust.open_album_id = open.and_then(|album| rust.album_id(album)).unwrap_or(-1);
        }
        self.as_mut().end_reset_model();
        if old.2 != self.open_album_id {
            self.as_mut().open_album_id_changed();
        }
        if old.0 != columns {
            self.as_mut().columns_changed();
        }
        if old.1 != active {
            self.as_mut().active_changed();
        }
        self.refreshed(reset);
    }

    fn toggle_album(self: Pin<&mut Self>, album_id: i64) {
        let open = if self.open_id() == Some(album_id) {
            None
        } else {
            album_index(self.catalog.as_deref(), &self.view, album_id)
        };
        self.set_open(open);
    }

    fn close_album(self: Pin<&mut Self>) {
        self.set_open(None);
    }

    fn set_open(mut self: Pin<&mut Self>, open: Option<usize>) {
        if open == self.open {
            return;
        }
        let columns = width(self.columns);
        let mut rows: Vec<i32> = [self.open, open]
            .into_iter()
            .flatten()
            .filter_map(|album| i32::try_from(album / columns).ok())
            .collect();
        rows.dedup();
        let id = open.and_then(|album| self.album_id(album)).unwrap_or(-1);
        {
            let mut rust = self.as_mut().rust_mut();
            rust.open = open;
            rust.open_album_id = id;
        }
        list_model::role_changed(self.as_mut().upcast_pin(), &rows, OPEN_TILE_ROLE);
        self.open_album_id_changed();
    }

    fn open_id(&self) -> Option<i64> {
        (self.open_album_id >= 0).then_some(self.open_album_id)
    }

    fn relaid_row(&self, row: i32, columns: i32) -> i32 {
        let row = usize::try_from(row).unwrap_or(0);
        let relaid = row_holding(row, width(columns), width(self.columns));
        i32::try_from(relaid).unwrap_or(i32::MAX)
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let tiles = usize::try_from(index.row())
            .ok()
            .and_then(|row| grid_row(self.view.albums.len(), width(self.columns), row))
            .map(|albums| Tiles {
                catalog: self.catalog.as_deref(),
                view: &self.view,
                open_tile: open_tile(self.open, &albums),
                albums,
            });
        list_model::data(ROLES, tiles.as_ref(), role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.view.albums.len().div_ceil(width(self.columns)), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

impl AlbumGridModelRust {
    fn album_id(&self, album: usize) -> Option<i64> {
        album_id(self.catalog.as_deref()?, &self.view, album)
    }
}

fn album_id(catalog: &Catalog, view: &View, album: usize) -> Option<i64> {
    let first = *view.tracks.get(view.albums.get(album)?.start)?;
    Some(catalog.track(first)?.album_id)
}

/// Where album `album_id` is in `view`'s albums.
fn album_index(catalog: Option<&Catalog>, view: &View, album_id: i64) -> Option<usize> {
    let catalog = catalog?;
    (0..view.albums.len()).find(|&album| self::album_id(catalog, view, album) == Some(album_id))
}

/// The open album after the grid reloads: none after a reset, else album
/// `open` wherever it now is, if it is still there.
fn still_open(
    catalog: Option<&Catalog>,
    view: &View,
    open: Option<i64>,
    reset: bool,
) -> Option<usize> {
    open.filter(|_| !reset)
        .and_then(|id| album_index(catalog, view, id))
}

/// The open album's place in the grid row of `albums`, or -1.
fn open_tile(open: Option<usize>, albums: &GridRow) -> i32 {
    open.filter(|album| albums.contains(album))
        .and_then(|album| i32::try_from(album - albums.start).ok())
        .unwrap_or(-1)
}

fn width(columns: i32) -> usize {
    usize::try_from(columns).unwrap_or(1).max(1)
}

/// The row of a grid `new_columns` wide that holds the first album of row
/// `row` of one `old_columns` wide.
fn row_holding(row: usize, old_columns: usize, new_columns: usize) -> usize {
    row.saturating_mul(old_columns) / new_columns.max(1)
}

/// The albums in grid row `row` of `albums` albums laid out `columns` to a row.
fn grid_row(albums: usize, columns: usize, row: usize) -> Option<GridRow> {
    let start = row.checked_mul(columns)?;
    (start < albums).then(|| start..albums.min(start + columns))
}

/// For each cover in `view`, the grid rows that show it.
fn grid_cover_rows(catalog: &Catalog, view: &View, columns: usize) -> CoverRows {
    let mut rows = covers::rows_by_cover(
        (0..view.albums.len()).map(|album| album_model::album_cover(catalog, view, album)),
    );
    for grid_rows in rows.values_mut() {
        for row in grid_rows.iter_mut() {
            *row /= i32::try_from(columns).unwrap_or(i32::MAX);
        }
        grid_rows.dedup();
    }
    rows
}

#[cfg(test)]
#[path = "tests/album_grid_model_test.rs"]
mod tests;
