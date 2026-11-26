//! The playlists by name, for the Playlists pane and the menus that add
//! tracks to one, and the edits made to them there.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QVariant};
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
        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
        type QList_i64 = cxx_qt_lib::QList<i64>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, count, READ, NOTIFY)]
        /// Playlists can be made and changed: they have loaded.
        #[qproperty(bool, loaded, READ, NOTIFY)]
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

        /// Makes a playlist of `track_ids` and returns its id, or "" for a
        /// blank name or before the playlists have loaded.
        #[qinvokable]
        fn create(self: Pin<&mut PlaylistsModel>, name: &QString, track_ids: &QList_i64)
        -> QString;

        #[qinvokable]
        fn rename(self: Pin<&mut PlaylistsModel>, id: &QString, name: &QString);

        #[qinvokable]
        fn remove(self: Pin<&mut PlaylistsModel>, id: &QString);

        /// Removes the tracks at the main pane's `rows` from the playlist it
        /// shows.
        #[qinvokable]
        #[cxx_name = "removeTracks"]
        fn remove_tracks(self: Pin<&mut PlaylistsModel>, rows: &QList_i32);

        /// Moves the tracks at the main pane's `rows`, keeping their order, to
        /// before its row `to`, or to the end when `to` is past the last.
        #[qinvokable]
        #[cxx_name = "moveTracks"]
        fn move_tracks(self: Pin<&mut PlaylistsModel>, rows: &QList_i32, to: i32);

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
    ("menuText", |p| text(&menu_text(&p.name))),
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
    loaded: bool,
    current_row: i32,
    /// By name.
    playlists: Vec<Playlist>,
}

impl cxx_qt::Initialize for qobject::PlaylistsModel {
    fn initialize(mut self: Pin<&mut Self>) {
        self.as_mut().refresh();
        session::on_change(self.qt_thread(), Change::Playlists, Self::refresh);
        session::on_change(self.qt_thread(), Change::Browse, Self::show_current);
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
        set!(self, loaded, loaded_changed, session::playlists_loaded());
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

    fn create(self: Pin<&mut Self>, name: &QString, track_ids: &QList<i64>) -> QString {
        let created = playlist_name(name)
            .and_then(|name| session::create_playlist(name, track_ids.iter().copied().collect()));
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

    fn remove_tracks(self: Pin<&mut Self>, rows: &QList<i32>) {
        edit_shown(|playlist, positions| remove_rows(playlist, positions, rows.iter().copied()));
    }

    fn move_tracks(self: Pin<&mut Self>, rows: &QList<i32>, to: i32) {
        edit_shown(|playlist, positions| {
            move_rows(playlist, positions, rows.iter().copied(), to);
        });
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

/// Edits the playlist the main pane shows, given each of its rows' position
/// in the playlist.
fn edit_shown(edit: impl FnOnce(&mut Playlist, &[Option<usize>])) {
    let (Source::Playlist { id: Some(id) }, Some(catalog)) =
        (session::browse().source, session::catalog())
    else {
        return;
    };
    let view = session::view();
    session::edit_playlists(|playlists| {
        if let Some(playlist) = playlists.iter_mut().find(|p| p.id == id) {
            let positions = catalog.playlist_positions(&view, &playlist.track_ids);
            edit(playlist, &positions);
        }
    });
}

/// Removes the tracks a view of `playlist` shows at `rows`, given each of
/// its rows' position (`Catalog::playlist_positions`).
fn remove_rows(
    playlist: &mut Playlist,
    positions: &[Option<usize>],
    rows: impl IntoIterator<Item = i32>,
) {
    playlist.remove(&picked(positions, rows));
}

/// Moves the tracks a view of `playlist` shows at `rows` to before its row
/// `to`, or to the end when `to` is past the last.
fn move_rows(
    playlist: &mut Playlist,
    positions: &[Option<usize>],
    rows: impl IntoIterator<Item = i32>,
    to: i32,
) {
    let moving = picked(positions, rows);
    let to = usize::try_from(to)
        .ok()
        .and_then(|row| positions.get(row).copied().flatten())
        .unwrap_or(playlist.track_ids.len());
    playlist.move_to(&moving, to);
}

fn picked(positions: &[Option<usize>], rows: impl IntoIterator<Item = i32>) -> Vec<usize> {
    rows.into_iter()
        .filter_map(|row| usize::try_from(row).ok())
        .filter_map(|row| positions.get(row).copied().flatten())
        .collect()
}

/// `name` as a menu item shows it: as written, so `&` isn't a mnemonic
/// and markup isn't styled. A word joiner after `<` keeps it from opening
/// a tag, and shows nothing.
fn menu_text(name: &str) -> String {
    name.replace('&', "&&").replace('<', "<\u{2060}")
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
