//! The playlists by name, for the Playlists pane and the menus that add
//! tracks to one, and the edits made to them there.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use lune_core::{ListId, Playlist, Source};

use crate::list_model::{self, Role, text};
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
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ, NOTIFY)]
        /// The shown playlist's row, or -1.
        #[qproperty(i32, current_row, cxx_name = "currentRow", READ, NOTIFY)]
        type PlaylistsModel = super::PlaylistsModelRust;
    }

    // Playlists go by their `ListId` as text.
    unsafe extern "RustQt" {
        #[qinvokable]
        fn select(self: Pin<&mut PlaylistsModel>, row: i32);

        /// The id at `row`, or "".
        #[qinvokable]
        #[cxx_name = "idAt"]
        fn id_at(self: &PlaylistsModel, row: i32) -> QString;

        /// The name at `row`, or "".
        #[qinvokable]
        #[cxx_name = "nameAt"]
        fn name_at(self: &PlaylistsModel, row: i32) -> QString;

        #[qsignal]
        #[cxx_name = "playlistChosen"]
        fn playlist_chosen(self: Pin<&mut PlaylistsModel>, id: QString);

        /// Makes an empty playlist and returns its id, or "" for a blank name.
        #[qinvokable]
        fn create(self: Pin<&mut PlaylistsModel>, name: &QString) -> QString;

        #[qinvokable]
        fn rename(self: Pin<&mut PlaylistsModel>, id: &QString, name: &QString);

        #[qinvokable]
        fn remove(self: Pin<&mut PlaylistsModel>, id: &QString);

        /// `base`, or `base` and a number when a playlist has that name.
        #[qinvokable]
        #[cxx_name = "suggestedName"]
        fn suggested_name_from_qml(self: &PlaylistsModel, base: &QString) -> QString;

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

/// `label`, `detail`, `count` and `all` are the filter pane's roles.
const ROLES: &[Role<Playlist>] = &[
    ("playlistId", |p| text(&p.id.to_string())),
    ("label", |p| text(&p.name)),
    ("detail", |_| text("")),
    ("count", |p| {
        QVariant::from(&i32::try_from(p.track_ids.len()).unwrap_or(i32::MAX))
    }),
    ("all", |_| QVariant::from(&false)),
];

#[derive(Default)]
pub struct PlaylistsModelRust {
    count: i32,
    current_row: i32,
    /// By name.
    playlists: Vec<Playlist>,
}

impl cxx_qt::Initialize for qobject::PlaylistsModel {
    fn initialize(mut self: Pin<&mut Self>) {
        self.as_mut().refresh();
        session::on_change(self.qt_thread(), |model, change| match change {
            Change::Playlists => model.refresh(),
            Change::Browse => model.show_current(),
            _ => {}
        });
    }
}

impl qobject::PlaylistsModel {
    fn refresh(mut self: Pin<&mut Self>) {
        let playlists: Vec<Playlist> = Playlist::by_name(&session::playlists())
            .into_iter()
            .cloned()
            .collect();
        let count = i32::try_from(playlists.len()).unwrap_or(i32::MAX);
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().playlists = playlists;
        self.as_mut().end_reset_model();
        set!(self, count, count_changed, count);
        self.show_current();
    }

    fn show_current(mut self: Pin<&mut Self>) {
        let row = match session::browse().source {
            Source::Playlist { id: Some(id) } => self.playlists.iter().position(|p| p.id == id),
            _ => None,
        };
        let row = row.and_then(|r| i32::try_from(r).ok()).unwrap_or(-1);
        set!(self, current_row, current_row_changed, row);
    }

    fn select(self: Pin<&mut Self>, row: i32) {
        if row == self.current_row {
            return;
        }
        let id = self.id_at(row);
        if !id.is_empty() {
            self.playlist_chosen(id);
        }
    }

    fn playlist(&self, row: i32) -> Option<&Playlist> {
        usize::try_from(row)
            .ok()
            .and_then(|r| self.playlists.get(r))
    }

    fn id_at(&self, row: i32) -> QString {
        let id = self.playlist(row).map(|p| p.id.to_string());
        QString::from(&id.unwrap_or_default())
    }

    fn name_at(&self, row: i32) -> QString {
        let name = self.playlist(row).map(|p| p.name.as_str());
        QString::from(name.unwrap_or_default())
    }

    fn create(self: Pin<&mut Self>, name: &QString) -> QString {
        let Some(name) = playlist_name(name) else {
            return QString::default();
        };
        let mut created = None;
        session::edit_playlists(|playlists| {
            let id = Playlist::new_id(playlists);
            playlists.push(Playlist {
                id,
                name,
                track_ids: Vec::new(),
            });
            created = Some(id);
        });
        created
            .map(|id| QString::from(&id.to_string()))
            .unwrap_or_default()
    }

    fn rename(self: Pin<&mut Self>, id: &QString, name: &QString) {
        let (Some(id), Some(name)) = (list_id(id), playlist_name(name)) else {
            return;
        };
        session::edit_playlists(|playlists| {
            if let Some(playlist) = playlists.iter_mut().find(|p| p.id == id) {
                playlist.name = name;
            }
        });
    }

    fn remove(self: Pin<&mut Self>, id: &QString) {
        let Some(id) = list_id(id) else {
            return;
        };
        session::edit_playlists(|playlists| playlists.retain(|p| p.id != id));
    }

    fn suggested_name_from_qml(&self, base: &QString) -> QString {
        QString::from(&suggested_name(&base.to_string(), &self.playlists))
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        list_model::data(ROLES, self.playlist(index.row()), role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.playlists.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

fn list_id(id: &QString) -> Option<ListId> {
    id.to_string().parse().ok()
}

/// Trimmed; `None` when that leaves nothing.
fn playlist_name(name: &QString) -> Option<String> {
    let name = name.to_string();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn suggested_name(base: &str, playlists: &[Playlist]) -> String {
    let taken = |name: &str| playlists.iter().any(|p| p.name == name);
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken(name))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "tests/playlists_model_test.rs"]
mod tests;
