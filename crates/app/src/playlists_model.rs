//! The playlists, by name, for menus that add tracks to one.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;
use std::rc::Rc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QVariant};
use lune_core::Playlist;

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
        type PlaylistsModel = super::PlaylistsModelRust;
    }

    unsafe extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        fn data(self: &PlaylistsModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &PlaylistsModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &PlaylistsModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut PlaylistsModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut PlaylistsModel>);
    }

    impl cxx_qt::Threading for PlaylistsModel {}
    impl cxx_qt::Initialize for PlaylistsModel {}
}

/// `playlistId` is the `ListId` as text, as `PlayerController.addToPlaylist`
/// takes it.
const ROLES: &[Role<Playlist>] = &[
    ("playlistId", |p| text(&p.id.to_string())),
    ("name", |p| text(&p.name)),
];

pub struct PlaylistsModelRust {
    playlists: Rc<Vec<Playlist>>,
}

impl Default for PlaylistsModelRust {
    fn default() -> Self {
        Self {
            playlists: session::playlists(),
        }
    }
}

impl cxx_qt::Initialize for qobject::PlaylistsModel {
    fn initialize(self: Pin<&mut Self>) {
        session::on_change(self.qt_thread(), |model, change| {
            if change == Change::Playlists {
                model.refresh();
            }
        });
    }
}

impl qobject::PlaylistsModel {
    fn refresh(mut self: Pin<&mut Self>) {
        let playlists = session::playlists();
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().playlists = playlists;
        self.as_mut().end_reset_model();
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let playlist = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.playlists.get(row));
        list_model::data(ROLES, playlist, role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.playlists.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}
