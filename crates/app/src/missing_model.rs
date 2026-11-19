//! The Unavailable Tracks list: tracks whose files a scan no longer finds.
//! It shows a copy of the list, taken on `refresh()` after each change.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;
use std::rc::Rc;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QVariant};
use lune_core::MissingTrack;

use crate::list_model::{self, Role, text};
use crate::{format, session};

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
        type MissingModel = super::MissingModelRust;
    }

    unsafe extern "RustQt" {
        #[qinvokable]
        fn refresh(self: Pin<&mut MissingModel>);

        #[qinvokable]
        #[cxx_override]
        fn data(self: &MissingModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &MissingModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &MissingModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut MissingModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut MissingModel>);
    }
}

const ROLES: &[Role<MissingTrack>] = &[
    ("label", |t| text(&label(t))),
    ("path", |t| text(&t.path.display().to_string())),
    // A JavaScript time, so QML can show it as a local date.
    ("missingSince", |t| {
        QVariant::from(&format::epoch_ms(t.missing_ns))
    }),
    ("trackId", |t| QVariant::from(&t.id)),
];

#[derive(Default)]
pub struct MissingModelRust {
    tracks: Rc<Vec<MissingTrack>>,
}

impl qobject::MissingModel {
    fn refresh(mut self: Pin<&mut Self>) {
        let tracks = session::missing();
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().tracks = tracks;
        self.as_mut().end_reset_model();
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let track = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.tracks.get(row));
        list_model::data(ROLES, track, role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.tracks.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

/// Built here rather than with QML's `arg()`, which would also fill in
/// placeholders such as `%2` that the tag text itself contains.
fn label(track: &MissingTrack) -> String {
    format!(
        "{} – {} · {}",
        track.title,
        format::artist(track.artist.as_deref().unwrap_or_default()),
        format::album(&track.album)
    )
}

#[cfg(test)]
#[path = "tests/missing_model_test.rs"]
mod tests;
