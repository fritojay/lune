//! The main pane's albums (`session::view`) as a grid: each row holds up to
//! `columns` albums, so QML can open an album's tracks between two rows.

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
    QVariant,
};
use lune_core::{Catalog, View};

use crate::album_model::{self, Album, album_at};
use crate::covers;
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

const ROLES: &[Role] = &[("albums", |t| t.maps())];

const ALBUMS_ROLE: i32 = list_model::role_id(ROLES, "albums");

struct Tiles<'a> {
    catalog: Option<&'a Catalog>,
    view: &'a View,
    albums: GridRow,
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
    cover_rows: HashMap<i64, Vec<i32>>,
}

impl Default for AlbumGridModelRust {
    fn default() -> Self {
        Self {
            columns: 1,
            active: false,
            catalog: None,
            view: Rc::default(),
            cover_rows: HashMap::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::AlbumGridModel {
    fn initialize(self: Pin<&mut Self>) {
        covers::on_ready(self.qt_thread(), Self::cover_ready);
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
        self.as_mut().begin_reset_model();
        let old = (self.columns, self.active);
        {
            let mut rust = self.as_mut().rust_mut();
            rust.columns = columns;
            rust.active = active;
            rust.catalog = catalog;
            rust.view = view;
            rust.cover_rows = cover_rows;
        }
        self.as_mut().end_reset_model();
        if old.0 != columns {
            self.as_mut().columns_changed();
        }
        if old.1 != active {
            self.as_mut().active_changed();
        }
        self.refreshed(reset);
    }

    fn relaid_row(&self, row: i32, columns: i32) -> i32 {
        let row = usize::try_from(row).unwrap_or(0);
        let relaid = row_holding(row, width(columns), width(self.columns));
        i32::try_from(relaid).unwrap_or(i32::MAX)
    }

    fn cover_ready(self: Pin<&mut Self>, art_id: i64) {
        if let Some(rows) = self.cover_rows.get(&art_id).cloned() {
            list_model::role_changed(self.upcast_pin(), &rows, ALBUMS_ROLE);
        }
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let tiles = usize::try_from(index.row())
            .ok()
            .and_then(|row| grid_row(self.view.albums.len(), width(self.columns), row))
            .map(|albums| Tiles {
                catalog: self.catalog.as_deref(),
                view: &self.view,
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
fn grid_cover_rows(catalog: &Catalog, view: &View, columns: usize) -> HashMap<i64, Vec<i32>> {
    let every_album: Vec<usize> = (0..view.albums.len()).collect();
    let mut rows = album_model::cover_rows(catalog, view, &every_album);
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
