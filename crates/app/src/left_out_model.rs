//! The Files Not Added list: what the scans saw but didn't add, with why.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QVariant};
use lune_core::{LeftOut, LeftOutReason};

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
        type LeftOutModel = super::LeftOutModelRust;
    }

    unsafe extern "RustQt" {
        #[qinvokable]
        #[cxx_override]
        fn data(self: &LeftOutModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &LeftOutModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &LeftOutModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut LeftOutModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut LeftOutModel>);
    }

    impl cxx_qt::Threading for LeftOutModel {}
    impl cxx_qt::Initialize for LeftOutModel {}
}

const ROLES: &[Role<LeftOut>] = &[
    ("path", |f| text(&f.path.display().to_string())),
    ("reason", |f| text(&reason(&f.reason))),
];

#[derive(Default)]
pub struct LeftOutModelRust {
    files: Arc<Vec<LeftOut>>,
}

impl cxx_qt::Initialize for qobject::LeftOutModel {
    fn initialize(self: Pin<&mut Self>) {
        session::on_change(self.qt_thread(), Change::LeftOut, Self::refresh);
    }
}

impl qobject::LeftOutModel {
    fn refresh(mut self: Pin<&mut Self>) {
        let files = session::left_out();
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().files = files;
        self.as_mut().end_reset_model();
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let file = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.files.get(row));
        list_model::data(ROLES, file, role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.files.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

fn reason(reason: &LeftOutReason) -> String {
    match reason {
        LeftOutReason::Tags(cause) => format!("Tags couldn't be read: {cause}"),
        LeftOutReason::Link => "Linked file; links aren't followed".to_owned(),
        LeftOutReason::NonUtf8Name => "Name isn't valid UTF-8".to_owned(),
        LeftOutReason::Unreadable(cause) => format!("Couldn't be opened: {cause}"),
    }
}

#[cfg(test)]
#[path = "tests/left_out_model_test.rs"]
mod tests;
