//! The search/filter object: the grouping, the filter pane's values, the
//! selected value, the search text, and the column sort. Each change
//! recomputes the main pane's view (`session::view`) and emits `viewChanged`.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::mpsc::{self, Sender};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use lune_core::{BrowseState, Filter, FilterValue, Grouping, Library, Sort, SortColumn};

use crate::list_model::{self, Role, text};
use crate::property::set;
use crate::{format, session};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!(<QtCore/QAbstractListModel>);
        type QAbstractListModel;

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
        #[qproperty(bool, loading)]
        #[qproperty(i32, grouping, READ, NOTIFY)]
        #[qproperty(i32, current_row, cxx_name = "currentRow", READ, NOTIFY)]
        #[qproperty(QString, title, READ, NOTIFY)]
        #[qproperty(i32, track_count, cxx_name = "trackCount", READ, NOTIFY)]
        #[qproperty(QString, sort_column, cxx_name = "sortColumn", READ, NOTIFY)]
        #[qproperty(bool, sort_descending, cxx_name = "sortDescending", READ, NOTIFY)]
        type FilterModel = super::FilterModelRust;
    }

    unsafe extern "RustQt" {
        /// Rebuilds from `session::catalog`, keeping the selection if it
        /// still exists (after the library loads or changes).
        #[qinvokable]
        fn reload(self: Pin<&mut FilterModel>);

        /// `session::view` changed. `reset` is false when only the library
        /// changed underneath (a rescan), so the list keeps its position.
        #[qsignal]
        #[cxx_name = "viewChanged"]
        fn view_changed(self: Pin<&mut FilterModel>, reset: bool);

        /// 0 Album Artist, 1 Artist, 2 Album, 3 Year, 4 Recently Added,
        /// 5 Recently Played.
        #[qinvokable]
        #[cxx_name = "setGrouping"]
        fn set_grouping_from_qml(self: Pin<&mut FilterModel>, grouping: i32);

        #[qinvokable]
        fn select(self: Pin<&mut FilterModel>, row: i32);

        /// A track started playing (`session::record_play`).
        #[qinvokable]
        fn played(self: Pin<&mut FilterModel>);

        /// A click on column `column`'s heading. Columns go by
        /// `SortColumn::name`; `sortColumn` is empty in album order.
        #[qinvokable]
        #[cxx_name = "sortBy"]
        fn sort_by(self: Pin<&mut FilterModel>, column: &QString);

        /// Sorts by `column`, or restores album order for "".
        #[qinvokable]
        #[cxx_name = "setSort"]
        fn set_sort(self: Pin<&mut FilterModel>, column: &QString, descending: bool);

        #[qinvokable]
        #[cxx_name = "setSearch"]
        fn set_search(self: Pin<&mut FilterModel>, text: &QString);

        #[qinvokable]
        #[cxx_override]
        fn data(self: &FilterModel, index: &QModelIndex, role: i32) -> QVariant;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &FilterModel, parent: &QModelIndex) -> i32;

        #[qinvokable]
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &FilterModel) -> QHash_i32_QByteArray;

        #[inherit]
        #[cxx_name = "beginResetModel"]
        fn begin_reset_model(self: Pin<&mut FilterModel>);

        #[inherit]
        #[cxx_name = "endResetModel"]
        fn end_reset_model(self: Pin<&mut FilterModel>);
    }

    impl cxx_qt::Threading for FilterModel {}
    impl cxx_qt::Initialize for FilterModel {}
}

const ROLES: &[Role<Filter>] = &[
    ("label", |f| text(&format::filter_label(f))),
    ("detail", |f| {
        text(&f.detail.as_deref().map(format::artist).unwrap_or_default())
    }),
    ("count", |f| {
        QVariant::from(&i32::try_from(f.tracks).unwrap_or(i32::MAX))
    }),
];

const GROUPINGS: [Grouping; 6] = [
    Grouping::AlbumArtist,
    Grouping::Artist,
    Grouping::Album,
    Grouping::Year,
    Grouping::RecentlyAdded,
    Grouping::RecentlyPlayed,
];

fn grouping_index(grouping: Grouping) -> i32 {
    GROUPINGS
        .iter()
        .position(|g| *g == grouping)
        .and_then(|g| i32::try_from(g).ok())
        .unwrap_or(0)
}

/// The sort that `sortColumn` and `sortDescending` describe.
fn sort_of(column: &str, descending: bool) -> Option<Sort> {
    let column = SortColumn::from_name(column)?;
    Some(Sort { column, descending })
}

/// `sortColumn` and `sortDescending` for `sort`.
fn sort_state(sort: Option<Sort>) -> (&'static str, bool) {
    sort.map_or(("", false), |sort| (sort.column.name(), sort.descending))
}

/// The sort after a click on heading `clicked`, or `None` for a heading
/// that doesn't exist.
fn after_click(current: Option<Sort>, clicked: &str) -> Option<Option<Sort>> {
    Some(Sort::after_click(current, SortColumn::from_name(clicked)?))
}

/// The sort picked from the menu, or `None` for a column that doesn't exist
/// or the sort already in effect.
fn chosen(current: Option<Sort>, column: &str, descending: bool) -> Option<Option<Sort>> {
    let sort = if column.is_empty() {
        None
    } else {
        Some(sort_of(column, descending)?)
    };
    (sort != current).then_some(sort)
}

pub struct FilterModelRust {
    loading: bool,
    grouping: i32,
    current_row: i32,
    title: QString,
    track_count: i32,
    sort_column: QString,
    sort_descending: bool,
    rows: Vec<Filter>,
    /// Track ids matching the search; `None` without a search.
    matches: Option<HashSet<i64>>,
    search_generation: u64,
    searches: Option<Sender<(u64, String)>>,
}

/// Starts from the last run's grouping and sort; the first `reload` restores
/// its selection.
impl Default for FilterModelRust {
    fn default() -> Self {
        let saved = session::browse();
        let (sort_column, sort_descending) = sort_state(saved.sort);
        Self {
            loading: true,
            grouping: grouping_index(saved.grouping),
            current_row: 0,
            title: QString::default(),
            track_count: 0,
            sort_column: QString::from(sort_column),
            sort_descending,
            rows: Vec::new(),
            matches: None,
            search_generation: 0,
            searches: None,
        }
    }
}

impl cxx_qt::Initialize for qobject::FilterModel {
    fn initialize(mut self: Pin<&mut Self>) {
        let (tx, rx) = mpsc::channel::<(u64, String)>();
        let qt_thread = self.qt_thread();
        let db_path = crate::config().db_path.clone();
        std::thread::spawn(move || {
            let library = match Library::open(&db_path) {
                Ok(library) => library,
                Err(err) => {
                    tracing::error!(%err, "search is unavailable");
                    return;
                }
            };
            while let Ok(first) = rx.recv() {
                // Only the latest text matters; skip what was typed meanwhile.
                let Some((generation, text)) = std::iter::once(first).chain(rx.try_iter()).last()
                else {
                    continue;
                };
                let matches = if text.trim().is_empty() {
                    None
                } else {
                    match library.search_ids(&text) {
                        Ok(ids) => Some(ids),
                        Err(err) => {
                            tracing::warn!(%err, "search failed");
                            continue;
                        }
                    }
                };
                if qt_thread
                    .queue(move |model| model.apply_search(generation, matches))
                    .is_err()
                {
                    break;
                }
            }
        });
        self.as_mut().rust_mut().searches = Some(tx);
    }
}

impl qobject::FilterModel {
    fn reload(mut self: Pin<&mut Self>) {
        let first = self.loading;
        let selected = if first {
            Some(session::browse().selected)
        } else {
            self.selected().cloned()
        };
        self.as_mut().recompute(selected, first);
        self.set_loading(false);
    }

    fn apply_search(mut self: Pin<&mut Self>, generation: u64, matches: Option<HashSet<i64>>) {
        if generation != self.search_generation {
            return;
        }
        let selected = self.selected().cloned();
        self.as_mut().rust_mut().matches = matches;
        self.recompute(selected, true);
    }

    fn set_grouping_from_qml(mut self: Pin<&mut Self>, grouping: i32) {
        let valid = usize::try_from(grouping)
            .ok()
            .is_some_and(|g| g < GROUPINGS.len());
        if !valid {
            return;
        }
        set!(self, grouping, grouping_changed, grouping);
        self.recompute(None, true);
    }

    fn select(mut self: Pin<&mut Self>, row: i32) {
        if row == self.current_row || self.filter(row).is_none() {
            return;
        }
        set!(self, current_row, current_row_changed, row);
        self.update_view(true);
    }

    fn played(mut self: Pin<&mut Self>) {
        if self.grouping_kind() == Grouping::RecentlyPlayed {
            let selected = self.selected().cloned();
            self.as_mut().recompute(selected, false);
        }
    }

    fn sort_by(self: Pin<&mut Self>, column: &QString) {
        let sort = after_click(self.sort(), &column.to_string());
        self.apply_sort(sort);
    }

    fn set_sort(self: Pin<&mut Self>, column: &QString, descending: bool) {
        let sort = chosen(self.sort(), &column.to_string(), descending);
        self.apply_sort(sort);
    }

    fn apply_sort(mut self: Pin<&mut Self>, sort: Option<Option<Sort>>) {
        let Some(sort) = sort else {
            return;
        };
        let (column, descending) = sort_state(sort);
        set!(
            self,
            sort_column,
            sort_column_changed,
            QString::from(column)
        );
        set!(self, sort_descending, sort_descending_changed, descending);
        self.update_view(true);
    }

    fn sort(&self) -> Option<Sort> {
        sort_of(&self.sort_column.to_string(), self.sort_descending)
    }

    fn set_search(mut self: Pin<&mut Self>, text: &QString) {
        let generation = self.search_generation + 1;
        self.as_mut().rust_mut().search_generation = generation;
        if let Some(searches) = &self.searches {
            let _ = searches.send((generation, text.to_string()));
        }
    }

    fn grouping_kind(&self) -> Grouping {
        usize::try_from(self.grouping)
            .ok()
            .and_then(|g| GROUPINGS.get(g))
            .copied()
            .unwrap_or(Grouping::AlbumArtist)
    }

    fn filter(&self, row: i32) -> Option<&Filter> {
        usize::try_from(row).ok().and_then(|r| self.rows.get(r))
    }

    fn selected(&self) -> Option<&FilterValue> {
        self.filter(self.current_row).map(|f| &f.value)
    }

    /// Rebuilds the filter pane, keeping `keep` selected if it still has
    /// tracks (otherwise "All"), and then the view.
    fn recompute(mut self: Pin<&mut Self>, keep: Option<FilterValue>, reset: bool) {
        let Some(catalog) = session::catalog() else {
            return;
        };
        let rows = catalog.filters(self.grouping_kind(), self.matches.as_ref());
        let row = keep
            .and_then(|value| rows.iter().position(|f| f.value == value))
            .unwrap_or(0);
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = rows;
        self.as_mut().end_reset_model();
        // Sent even when unchanged: the model reset can move the ListView's
        // current index, and this puts it back.
        self.as_mut().rust_mut().current_row = i32::try_from(row).unwrap_or(0);
        self.as_mut().current_row_changed();
        self.update_view(reset);
    }

    fn update_view(mut self: Pin<&mut Self>, reset: bool) {
        let Some(catalog) = session::catalog() else {
            return;
        };
        let (value, title) = match self.filter(self.current_row) {
            Some(filter) => (filter.value.clone(), format::filter_label(filter)),
            None => (FilterValue::All, format::filter_label(&all())),
        };
        let mut view = catalog.view(self.grouping_kind(), &value, self.matches.as_ref());
        if let Some(sort) = self.sort() {
            view = catalog.sorted(view, sort);
        }
        session::set_browse(BrowseState {
            grouping: self.grouping_kind(),
            selected: value,
            sort: self.sort(),
        });
        let count = i32::try_from(view.len()).unwrap_or(i32::MAX);
        session::set_view(view);
        set!(self, title, title_changed, QString::from(&title));
        set!(self, track_count, track_count_changed, count);
        self.view_changed(reset);
    }

    fn data(&self, index: &QModelIndex, role: i32) -> QVariant {
        list_model::data(ROLES, self.filter(index.row()), role)
    }

    fn row_count(&self, parent: &QModelIndex) -> i32 {
        list_model::row_count(self.rows.len(), parent)
    }

    fn role_names(&self) -> QHash<QHashPair_i32_QByteArray> {
        list_model::role_names(ROLES)
    }
}

fn all() -> Filter {
    Filter {
        value: FilterValue::All,
        label: String::new(),
        detail: None,
        tracks: 0,
    }
}

#[cfg(test)]
#[path = "tests/filter_model_test.rs"]
mod tests;
