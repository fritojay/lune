//! Qt's list model base class, declared once so the models' bridges share it
//! (`#[base = QAbstractListModel]`) and helpers can take any of the models.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt_lib::{
    QByteArray, QHash, QHashPair_i32_QByteArray, QList, QModelIndex, QString, QVariant,
};

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

/// Qt::UserRole. A model's roles are numbered from it in the order of its
/// role table: `(name QML reads it by, its value for a row)` pairs.
const FIRST_ROLE: i32 = 0x0100;

pub type Role<R> = (&'static str, fn(&R) -> QVariant);

/// Generic over `F` rather than taking [`Role`], so a table can have a row
/// type that borrows from its model, such as `fn(&Album<'_>) -> QVariant`.
pub fn data<R, F: Fn(&R) -> QVariant>(roles: &[(&str, F)], row: Option<&R>, role: i32) -> QVariant {
    let value = role
        .checked_sub(FIRST_ROLE)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| roles.get(i));
    match (row, value) {
        (Some(row), Some((_, value))) => value(row),
        _ => QVariant::default(),
    }
}

pub fn role_names<F>(roles: &[(&str, F)]) -> QHash<QHashPair_i32_QByteArray> {
    let mut names = QHash::<QHashPair_i32_QByteArray>::default();
    for (id, (name, _)) in (FIRST_ROLE..).zip(roles) {
        names.insert(id, QByteArray::from(*name));
    }
    names
}

/// Panics on a name not in `roles`, so use it in a `const`, where that is a
/// compile error.
pub const fn role_id<F>(roles: &[(&str, F)], name: &str) -> i32 {
    let mut i = 0;
    while i < roles.len() {
        if bytes_eq(roles[i].0.as_bytes(), name.as_bytes()) {
            return FIRST_ROLE + i as i32;
        }
        i += 1;
    }
    panic!("no such role");
}

const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

pub fn row_count(len: usize, parent: &QModelIndex) -> i32 {
    if parent.is_valid() {
        return 0;
    }
    i32::try_from(len).unwrap_or(i32::MAX)
}

pub fn text(text: &str) -> QVariant {
    QVariant::from(&QString::from(text))
}

#[cfg(test)]
#[path = "tests/list_model_test.rs"]
mod tests;
