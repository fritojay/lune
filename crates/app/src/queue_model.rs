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
use cxx_qt_lib::{QByteArray, QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use lune_core::{Catalog, MissingTrack};

use crate::{covers, format, list_model, session};

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

const TITLE_ROLE: i32 = 0x0100; // Qt::UserRole
const ARTIST_ROLE: i32 = TITLE_ROLE + 1;
const DURATION_ROLE: i32 = TITLE_ROLE + 2;
const COVER_ROLE: i32 = TITLE_ROLE + 3;
const MISSING_ROLE: i32 = TITLE_ROLE + 4;

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
        self.as_mut().rust_mut().count = count;
        self.as_mut().count_changed();
        self.as_mut().rust_mut().total_duration = QString::from(&format::long_duration(total));
        self.total_duration_changed();
    }

    fn cover_ready(self: Pin<&mut Self>, art_id: i64) {
        if let Some(rows) = self.cover_rows.get(&art_id).cloned() {
            list_model::role_changed(self.upcast_pin(), &rows, COVER_ROLE);
        }
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        let Some(&id) = usize::try_from(index.row())
            .ok()
            .and_then(|row| self.track_ids.get(row))
        else {
            return QVariant::default();
        };
        if let Some(track) = self.catalog.as_ref().and_then(|c| c.by_id(id)) {
            let text = match role {
                TITLE_ROLE => track.title.clone(),
                ARTIST_ROLE => format::artist(track.artist.as_deref().unwrap_or_default()),
                DURATION_ROLE => format::duration(track.duration_ms),
                COVER_ROLE => return covers::url(track.art_id),
                MISSING_ROLE => return QVariant::from(&false),
                _ => return QVariant::default(),
            };
            return QVariant::from(&QString::from(&text));
        }
        if let Some(track) = self.missing.get(&id) {
            let text = match role {
                TITLE_ROLE => track.title.clone(),
                ARTIST_ROLE => format::artist(track.artist.as_deref().unwrap_or_default()),
                MISSING_ROLE => return QVariant::from(&true),
                _ => return QVariant::default(),
            };
            return QVariant::from(&QString::from(&text));
        }
        // A queued track removed from the library is still listed until loaded again.
        match role {
            TITLE_ROLE => QVariant::from(&QString::from("(missing)")),
            MISSING_ROLE => QVariant::from(&false),
            _ => QVariant::default(),
        }
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        if parent.is_valid() {
            return 0;
        }
        i32::try_from(self.track_ids.len()).unwrap_or(i32::MAX)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        let mut roles = QHash::<QHashPair_i32_QByteArray>::default();
        roles.insert(TITLE_ROLE, QByteArray::from("title"));
        roles.insert(ARTIST_ROLE, QByteArray::from("artist"));
        roles.insert(DURATION_ROLE, QByteArray::from("duration"));
        roles.insert(COVER_ROLE, QByteArray::from("cover"));
        roles.insert(MISSING_ROLE, QByteArray::from("missing"));
        roles
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
