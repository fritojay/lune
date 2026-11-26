//! The search/filter object: the source, the filter pane's values, the
//! selected value, the search text, and the column sort. Each change
//! recomputes the main pane's view (`session::set_view`).

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QHash, QHashPair_i32_QByteArray, QModelIndex, QString, QVariant};
use lune_core::{
    BrowseState, Filter, FilterValue, Grouping, Library, ListId, MenuChoice, Playlist, Source,
    TrackFilter, View, after_heading_click, menu_choice, sort_state,
};

use crate::list_model::{self, Role, text};
use crate::property::set;
use crate::session::{self, Change};
use crate::{format, sources_model};

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
        #[qproperty(i32, current_row, cxx_name = "currentRow", READ, NOTIFY)]
        #[qproperty(QString, title, READ, NOTIFY)]
        #[qproperty(i32, track_count, cxx_name = "trackCount", READ, NOTIFY)]
        #[qproperty(QString, sort_column, cxx_name = "sortColumn", READ, NOTIFY)]
        #[qproperty(bool, sort_descending, cxx_name = "sortDescending", READ, NOTIFY)]
        /// Whether album order has a direction to pick in this source.
        #[qproperty(bool, album_directional, cxx_name = "albumDirectional", READ, NOTIFY)]
        /// `SourcesModel`'s `kind` of the source.
        #[qproperty(QString, source_kind, cxx_name = "sourceKind", READ, NOTIFY)]
        /// The main pane is a track list, not albums with covers.
        #[qproperty(bool, flat, READ, NOTIFY)]
        /// A playlist in its own order, with no search, so its tracks can be
        /// moved.
        #[qproperty(bool, reorderable, READ, NOTIFY)]
        type FilterModel = super::FilterModelRust;
    }

    unsafe extern "RustQt" {
        /// Groupings go by `Grouping::name`.
        #[qinvokable]
        #[cxx_name = "setGrouping"]
        fn set_grouping_from_qml(self: Pin<&mut FilterModel>, grouping: &QString);

        /// Playlists go by their `ListId` as text.
        #[qinvokable]
        #[cxx_name = "showPlaylist"]
        fn show_playlist(self: Pin<&mut FilterModel>, id: &QString);

        /// Shows the playlist shown last in this run, or the first by name.
        #[qinvokable]
        #[cxx_name = "showPlaylists"]
        fn show_playlists(self: Pin<&mut FilterModel>);

        #[qinvokable]
        fn select(self: Pin<&mut FilterModel>, row: i32);

        /// Selects track `track_id`'s album in the Albums grouping, and
        /// clears the search, which could hide it.
        #[qinvokable]
        #[cxx_name = "showAlbum"]
        fn show_album(self: Pin<&mut FilterModel>, track_id: i64);

        /// The search was dropped here, so QML empties its field.
        #[qsignal]
        #[cxx_name = "searchCleared"]
        fn search_cleared(self: Pin<&mut FilterModel>);

        /// A click on column `column`'s heading. Columns go by
        /// `SortColumn::name`; `sortColumn` is empty in album order.
        #[qinvokable]
        #[cxx_name = "sortBy"]
        fn sort_by(self: Pin<&mut FilterModel>, column: &QString);

        /// A pick from the sort menu; see `menu_choice`.
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
    ("all", |f| QVariant::from(&(f.value == FilterValue::All))),
];

pub struct FilterModelRust {
    loading: bool,
    current_row: i32,
    title: QString,
    track_count: i32,
    sort_column: QString,
    /// The direction of `sortColumn`'s sort, or of album order without one.
    sort_descending: bool,
    album_directional: bool,
    source_kind: QString,
    flat: bool,
    reorderable: bool,
    /// What the properties show. Until the first `reload`, the last run's.
    state: BrowseState,
    rows: Vec<Filter>,
    last_playlist: Option<ListId>,
    /// Tracks matching the search; `All` without a search.
    search: TrackFilter,
    search_generation: u64,
    searches: Option<Sender<(u64, String)>>,
}

impl Default for FilterModelRust {
    fn default() -> Self {
        let state = session::browse();
        let (sort_column, sort_descending) = sort_state(state.sort, state.album_descending);
        Self {
            loading: true,
            current_row: 0,
            title: QString::default(),
            track_count: 0,
            sort_column: QString::from(sort_column),
            sort_descending,
            album_directional: state.source.has_album_direction(),
            source_kind: QString::from(sources_model::kind(state.source)),
            flat: false,
            reorderable: false,
            state,
            rows: Vec::new(),
            last_playlist: None,
            search: TrackFilter::All,
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
                    session::report_error(&format!("Search is unavailable: {err}"));
                    return;
                }
            };
            while let Ok(first) = rx.recv() {
                // Only the latest text matters; skip what was typed meanwhile.
                let Some((generation, text)) = std::iter::once(first).chain(rx.try_iter()).last()
                else {
                    continue;
                };
                let matches = match library.search_ids(&text) {
                    Ok(ids) => ids,
                    Err(err) => {
                        session::report_error(&format!("Search failed: {err}"));
                        continue;
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
        session::on_change(self.qt_thread(), |model, change| match change {
            Change::Catalog => model.reload(),
            Change::Played => model.played(),
            Change::Playlists => model.playlists_changed(),
            Change::View { .. }
            | Change::Missing
            | Change::Queue
            | Change::Browse
            | Change::OutputDevice => {}
        });
    }
}

impl qobject::FilterModel {
    /// Rebuilds from `session::catalog`, keeping the selection if it still
    /// exists.
    fn reload(mut self: Pin<&mut Self>) {
        let first = self.loading;
        self.as_mut().recompute(first);
        self.set_loading(false);
    }

    fn apply_search(mut self: Pin<&mut Self>, generation: u64, matches: HashSet<i64>) {
        if generation != self.search_generation {
            return;
        }
        self.as_mut().rust_mut().search = TrackFilter::Ids(Arc::new(matches));
        self.recompute(true);
    }

    fn set_grouping_from_qml(mut self: Pin<&mut Self>, grouping: &QString) {
        let Some(grouping) = Grouping::from_name(&grouping.to_string()) else {
            return;
        };
        self.as_mut()
            .show(Source::Library(grouping), FilterValue::All);
    }

    fn show_playlist(mut self: Pin<&mut Self>, id: &QString) {
        let Ok(id) = id.to_string().parse::<ListId>() else {
            return;
        };
        self.as_mut()
            .show(Source::Playlist { id: Some(id) }, FilterValue::All);
    }

    fn show_playlists(mut self: Pin<&mut Self>) {
        let id = self.last_playlist;
        self.as_mut()
            .show(Source::Playlist { id }, FilterValue::All);
    }

    fn show_album(mut self: Pin<&mut Self>, track_id: i64) {
        let Some(album) = session::catalog().and_then(|catalog| catalog.album_of(track_id)) else {
            return;
        };
        self.as_mut().drop_search();
        self.as_mut().show(Source::Library(Grouping::Album), album);
        self.search_cleared();
    }

    fn show(mut self: Pin<&mut Self>, source: Source, selected: FilterValue) {
        let mut this = self.as_mut().rust_mut();
        this.state.source = source;
        this.state.selected = selected;
        self.recompute(true);
    }

    fn select(mut self: Pin<&mut Self>, row: i32) {
        if row == self.current_row {
            return;
        }
        let Some(value) = self.filter(row).map(|f| f.value.clone()) else {
            return;
        };
        self.as_mut().rust_mut().state.selected = value;
        set!(self, current_row, current_row_changed, row);
        self.update_view();
    }

    fn played(self: Pin<&mut Self>) {
        if self.state.source == Source::Library(Grouping::RecentlyPlayed) {
            self.recompute(false);
        }
    }

    fn playlists_changed(self: Pin<&mut Self>) {
        if matches!(self.state.source, Source::Playlist { .. }) {
            self.recompute(false);
        }
    }

    fn sort_by(mut self: Pin<&mut Self>, column: &QString) {
        if let Some(sort) = after_heading_click(self.state.sort, &column.to_string()) {
            self.as_mut().rust_mut().state.sort = sort;
            self.update_view();
        }
    }

    fn set_sort(mut self: Pin<&mut Self>, column: &QString, descending: bool) {
        let choice = menu_choice(
            self.state.sort,
            self.state.album_descending,
            self.state.source.has_album_direction(),
            &column.to_string(),
            descending,
        );
        match choice {
            Some(MenuChoice::Sort(sort)) => {
                self.as_mut().rust_mut().state.sort = sort;
                self.update_view();
            }
            Some(MenuChoice::AlbumDescending(descending)) => {
                self.as_mut().rust_mut().state.album_descending = descending;
                self.recompute(true);
            }
            None => {}
        }
    }

    /// A blank search is applied here rather than by the search thread, so
    /// emptying an already empty search doesn't rebuild the view.
    fn set_search(mut self: Pin<&mut Self>, text: &QString) {
        let text = text.to_string();
        if text.trim().is_empty() {
            if self.as_mut().drop_search() {
                self.recompute(true);
            }
            return;
        }
        let generation = self.search_generation + 1;
        self.as_mut().rust_mut().search_generation = generation;
        if let Some(searches) = &self.searches {
            let _ = searches.send((generation, text));
        }
    }

    /// Forgets the search and any still running. Returns whether it had
    /// matches, so the view needs a rebuild.
    fn drop_search(mut self: Pin<&mut Self>) -> bool {
        let mut this = self.as_mut().rust_mut();
        this.search_generation += 1;
        !matches!(std::mem::take(&mut this.search), TrackFilter::All)
    }

    fn filter(&self, row: i32) -> Option<&Filter> {
        usize::try_from(row).ok().and_then(|r| self.rows.get(r))
    }

    /// Rebuilds the filter pane, keeping the selection if it still has
    /// tracks (otherwise "All"), and then the view.
    fn recompute(mut self: Pin<&mut Self>, reset: bool) {
        let playlists = session::playlists();
        // Only once the library has loaded: until then there are no
        // playlists, and the saved one would be forgotten.
        if let Source::Playlist { id } = self.state.source
            && session::catalog().is_some()
        {
            let id = Playlist::existing_or_first(id, &playlists);
            self.as_mut().rust_mut().state.source = Source::Playlist { id };
        }
        let browsed =
            session::catalog().map(|catalog| catalog.browse(&self.state, &playlists, &self.search));
        if let Some(browsed) = &browsed {
            let selected = browsed.filters[browsed.selected_row].value.clone();
            self.as_mut().rust_mut().state.selected = selected;
        }
        self.as_mut().show_state();
        let Some(browsed) = browsed else {
            return;
        };
        let row = browsed.selected_row;
        self.as_mut().begin_reset_model();
        self.as_mut().rust_mut().rows = browsed.filters;
        self.as_mut().end_reset_model();
        // Sent even when unchanged: the model reset can move the ListView's
        // current index, and this puts it back.
        self.as_mut().rust_mut().current_row = i32::try_from(row).unwrap_or(0);
        self.as_mut().current_row_changed();
        self.show_view(browsed.view, reset);
    }

    fn update_view(mut self: Pin<&mut Self>) {
        self.as_mut().show_state();
        let Some(catalog) = session::catalog() else {
            return;
        };
        let view = catalog.browsed_view(&self.state, &session::playlists(), &self.search);
        self.show_view(view, true);
    }

    /// Shows `state` in the properties and to `session::browse`.
    fn show_state(mut self: Pin<&mut Self>) {
        let state = self.state.clone();
        if let Source::Playlist { id: Some(id) } = state.source {
            self.as_mut().rust_mut().last_playlist = Some(id);
        }
        set!(
            self,
            source_kind,
            source_kind_changed,
            QString::from(sources_model::kind(state.source))
        );
        let (sort_column, sort_descending) = sort_state(state.sort, state.album_descending);
        set!(
            self,
            album_directional,
            album_directional_changed,
            state.source.has_album_direction()
        );
        set!(
            self,
            sort_column,
            sort_column_changed,
            QString::from(sort_column)
        );
        set!(
            self,
            sort_descending,
            sort_descending_changed,
            sort_descending
        );
        session::set_browse(state);
    }

    fn show_view(mut self: Pin<&mut Self>, view: View, reset: bool) {
        let title = match (self.state.source, self.filter(self.current_row)) {
            (Source::Playlist { id }, _) => id.map(playlist_name).unwrap_or_default(),
            (Source::Library(_), Some(filter)) => format::filter_label(filter),
            (Source::Library(grouping), None) => format::filter_label(&all(grouping)),
        };
        let count = i32::try_from(view.len()).unwrap_or(i32::MAX);
        let flat = view.flat;
        let reorderable = reorderable(&self.state, &self.search);
        session::set_view(view, reset);
        set!(self, flat, flat_changed, flat);
        set!(self, reorderable, reorderable_changed, reorderable);
        set!(self, title, title_changed, QString::from(&title));
        set!(self, track_count, track_count_changed, count);
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

fn playlist_name(id: ListId) -> String {
    let playlists = session::playlists();
    let playlist = playlists.iter().find(|p| p.id == id);
    playlist.map(|p| p.name.clone()).unwrap_or_default()
}

fn all(grouping: Grouping) -> Filter {
    Filter {
        value: FilterValue::All,
        field: grouping.field(),
        label: String::new(),
        detail: None,
        tracks: 0,
    }
}

/// Whether the main pane's tracks can be moved: a playlist in its own order,
/// with nothing hidden by a search.
fn reorderable(state: &BrowseState, search: &TrackFilter) -> bool {
    matches!(state.source, Source::Playlist { id: Some(_) })
        && state.sort.is_none()
        && matches!(search, TrackFilter::All)
}

#[cfg(test)]
#[path = "tests/filter_model_test.rs"]
mod tests;
