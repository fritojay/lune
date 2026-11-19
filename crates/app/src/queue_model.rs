//! The queue panel. It shows a copy of the queue's track ids, taken on
//! `refresh()` after every edit; the playing row comes from the player
//! controller through the `currentRow` property. Queued tracks whose files
//! are missing are flagged, as playback skips them.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::{HashMap, HashSet};
use std::pin::Pin;
use std::rc::Rc;

use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use lune_core::{Catalog, ListTrack, MissingTrack};

use crate::list_model::{self, text};
use crate::property::set;
use crate::{covers, format, session};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        type QAbstractListModel = crate::list_model::QAbstractListModel;

        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, current_row, cxx_name = "currentRow")]
        #[qproperty(i32, count, READ, NOTIFY)]
        #[qproperty(QString, total_duration, cxx_name = "totalDuration", READ, NOTIFY)]
        type QueueModel = super::QueueModelRust;
    }

    unsafe extern "RustQt" {
        #[qinvokable]
        fn refresh(self: Pin<&mut QueueModel>);

        #[qinvokable]
        #[cxx_override]
        fn data(self: &QueueModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &QueueModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &QueueModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut QueueModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut QueueModel>);
    }

    impl cxx_qt::Threading for QueueModel {}
    impl cxx_qt::Initialize for QueueModel {}
}

/// A queued track: in the library, missing its file, or removed from the
/// library (still listed until the queue is loaded again).
enum Queued<'a> {
    Listed(&'a ListTrack),
    Missing(&'a MissingTrack),
    Removed,
}

/// Not [`list_model::Role`], whose row type can't borrow for any lifetime.
type Role = (&'static str, fn(&Queued<'_>) -> QVariant);

const ROLES: &[Role] = &[
    ("title", |q| match q {
        Queued::Listed(t) => text(&t.title),
        Queued::Missing(t) => text(&t.title),
        Queued::Removed => text("(missing)"),
    }),
    ("artist", |q| match q {
        Queued::Listed(ListTrack { artist, .. }) | Queued::Missing(MissingTrack { artist, .. }) => {
            text(&format::artist(artist.as_deref().unwrap_or_default()))
        }
        Queued::Removed => QVariant::default(),
    }),
    ("duration", |q| match q {
        Queued::Listed(t) => text(&format::duration(t.duration_ms)),
        _ => QVariant::default(),
    }),
    ("cover", |q| match q {
        Queued::Listed(t) => covers::url(t.art_id),
        _ => QVariant::default(),
    }),
    ("missing", |q| {
        QVariant::from(&matches!(q, Queued::Missing(_)))
    }),
];

const COVER_ROLE: i32 = list_model::role_id(ROLES, "cover");

pub struct QueueModelRust {
    current_row: i32,
    count: i32,
    total_duration: QString,
    track_ids: Vec<i64>,
    catalog: Option<Rc<Catalog>>,
    /// The queued tracks whose files are missing (not in the catalog).
    missing: HashMap<i64, MissingTrack>,
    /// The rows showing each cover, so a finished cover doesn't scan the queue.
    cover_rows: HashMap<i64, Vec<i32>>,
}

impl Default for QueueModelRust {
    fn default() -> Self {
        Self {
            current_row: -1,
            count: 0,
            total_duration: QString::default(),
            track_ids: Vec::new(),
            catalog: None,
            missing: HashMap::new(),
            cover_rows: HashMap::new(),
        }
    }
}

impl cxx_qt::Initialize for qobject::QueueModel {
    fn initialize(self: Pin<&mut Self>) {
        covers::on_ready(self.qt_thread(), Self::cover_ready);
    }
}

impl qobject::QueueModel {
    fn refresh(mut self: Pin<&mut Self>) {
        let track_ids: Vec<i64> = session::lock_queue()
            .items()
            .iter()
            .map(|item| item.track_id)
            .collect();
        let catalog = session::catalog();
        let total: u64 = catalog.as_ref().map_or(0, |c| {
            track_ids
                .iter()
                .filter_map(|&id| c.by_id(id))
                .map(|t| t.duration_ms)
                .sum()
        });
        let cover_rows = cover_rows(catalog.as_deref(), &track_ids);
        let missing = queued_missing(&session::missing(), &track_ids);
        let count = i32::try_from(track_ids.len()).unwrap_or(i32::MAX);
        self.as_mut().begin_reset_model();
        {
            let mut rust = self.as_mut().rust_mut();
            rust.track_ids = track_ids;
            rust.catalog = catalog;
            rust.missing = missing;
            rust.cover_rows = cover_rows;
        }
        self.as_mut().end_reset_model();
        set!(self, count, count_changed, count);
        set!(
            self,
            total_duration,
            total_duration_changed,
            QString::from(&format::long_duration(total))
        );
    }

    fn cover_ready(self: Pin<&mut Self>, art_id: i64) {
        if let Some(rows) = self.cover_rows.get(&art_id).cloned() {
            list_model::role_changed(self.upcast_pin(), &rows, COVER_ROLE);
        }
    }

    fn queued(&self, row: i32) -> Option<Queued<'_>> {
        let &id = self.track_ids.get(usize::try_from(row).ok()?)?;
        Some(
            if let Some(track) = self.catalog.as_ref().and_then(|c| c.by_id(id)) {
                Queued::Listed(track)
            } else if let Some(track) = self.missing.get(&id) {
                Queued::Missing(track)
            } else {
                Queued::Removed
            },
        )
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        list_model::data(ROLES, self.queued(index.row()).as_ref(), role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.track_ids.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

/// For each cover, the rows of the queue `track_ids` showing it.
fn cover_rows(catalog: Option<&Catalog>, track_ids: &[i64]) -> HashMap<i64, Vec<i32>> {
    covers::rows_by_cover(track_ids.iter().map(|&id| catalog?.by_id(id)?.art_id))
}

/// The tracks among `missing` that are in the queue `track_ids`, by id.
fn queued_missing(missing: &[MissingTrack], track_ids: &[i64]) -> HashMap<i64, MissingTrack> {
    if missing.is_empty() {
        return HashMap::new();
    }
    let queued: HashSet<i64> = track_ids.iter().copied().collect();
    missing
        .iter()
        .filter(|track| queued.contains(&track.id))
        .map(|track| (track.id, track.clone()))
        .collect()
}

#[cfg(test)]
#[path = "tests/queue_model_test.rs"]
mod tests;
