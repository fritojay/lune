//! The grouping bar's list: every source the main pane can show, and which
//! one it shows.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::pin::Pin;

use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QVariant};
use lune_core::Grouping;
use lune_core::Source as Shown;

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
        type SourcesModel = super::SourcesModelRust;
    }

    unsafe extern "RustQt" {
        #[qinvokable]
        fn choose(self: Pin<&mut SourcesModel>, row: i32);

        #[qinvokable]
        #[cxx_override]
        fn data(self: &SourcesModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &SourcesModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &SourcesModel) -> QHash_i32_QByteArray;
    }

    impl cxx_qt::Threading for SourcesModel {}
    impl cxx_qt::Initialize for SourcesModel {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Source {
    grouping: Grouping,
    /// Whether the main pane shows it.
    current: bool,
}

/// `kind` and `sourceId` are what QML passes back when a source is chosen.
/// `sourceId` is the grouping's `Grouping::name`, which also keys its title
/// in GroupingBar.qml.
const ROLES: &[Role<Source>] = &[
    ("kind", |_| text("grouping")),
    ("sourceId", |s| text(s.grouping.name())),
    ("current", |s| QVariant::from(&s.current)),
];

const CURRENT_ROLE: i32 = list_model::role_id(ROLES, "current");

fn sources(current: Shown) -> Vec<Source> {
    Grouping::ALL
        .into_iter()
        .map(|grouping| Source {
            grouping,
            current: Shown::Library(grouping) == current,
        })
        .collect()
}

/// The rows whose `current` differs between `old` and `new`.
fn changed_rows(old: &[Source], new: &[Source]) -> Vec<i32> {
    (0..)
        .zip(old.iter().zip(new))
        .filter(|(_, (old, new))| old != new)
        .map(|(row, _)| row)
        .collect()
}

pub struct SourcesModelRust {
    sources: Vec<Source>,
}

impl Default for SourcesModelRust {
    fn default() -> Self {
        Self {
            sources: sources(session::browse().source),
        }
    }
}

impl cxx_qt::Initialize for qobject::SourcesModel {
    fn initialize(self: Pin<&mut Self>) {
        session::on_change(self.qt_thread(), |model, change| {
            if change == Change::Browse {
                model.browsed();
            }
        });
    }
}

impl qobject::SourcesModel {
    fn browsed(self: Pin<&mut Self>) {
        self.show(session::browse().source);
    }

    /// Marks the chosen row current at once: the `Change::Browse` that the
    /// choice causes arrives queued, and until then the bar would still
    /// show the old source.
    fn choose(self: Pin<&mut Self>, row: i32) {
        let Some(source) = usize::try_from(row).ok().and_then(|r| self.sources.get(r)) else {
            return;
        };
        let grouping = source.grouping;
        self.show(Shown::Library(grouping));
    }

    fn show(mut self: Pin<&mut Self>, current: Shown) {
        let sources = sources(current);
        let changed = changed_rows(&self.sources, &sources);
        if changed.is_empty() {
            return;
        }
        self.as_mut().rust_mut().sources = sources;
        list_model::role_changed(self.upcast_pin(), &changed, CURRENT_ROLE);
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let source = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.sources.get(row));
        list_model::data(ROLES, source, role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.sources.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

#[cfg(test)]
#[path = "tests/sources_model_test.rs"]
mod tests;
