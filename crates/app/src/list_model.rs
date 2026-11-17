//! Qt's list model base class, declared once so the models' bridges share it
//! (`#[base = QAbstractListModel]`) and helpers can take any of the models.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt_lib::{QList, QModelIndex};

pub use qobject::QAbstractListModel;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        type QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;

        fn index(
            self: &QAbstractListModel,
            row: i32,
            column: i32,
            parent: &QModelIndex,
        ) -> QModelIndex;

        #[cxx_name = "dataChanged"]
        fn data_changed(
            self: Pin<&mut QAbstractListModel>,
            top_left: &QModelIndex,
            bottom_right: &QModelIndex,
            roles: &QList_i32,
        );
    }
}

pub fn role_changed(mut model: Pin<&mut QAbstractListModel>, rows: &[i32], role: i32) {
    let mut roles = QList::<i32>::default();
    roles.append(role);
    for &row in rows {
        let index = model.index(row, 0, &QModelIndex::default());
        model.as_mut().data_changed(&index, &index, &roles);
    }
}
