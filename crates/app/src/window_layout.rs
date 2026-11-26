//! The main window's size and pane layout, restored from the last run and
//! saved, with the browse position (`session::browse`), when the app quits.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::BTreeSet;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QList, QString, QStringList};
use lune_core::{SortColumn, WindowState};

use crate::columns::{COLUMNS, Column, SPACING};
use crate::property::set;
use crate::{format::qml_int, session};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qlist.h");
        type QList_f64 = cxx_qt_lib::QList<f64>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, width, READ, NOTIFY)]
        #[qproperty(i32, height, READ, NOTIFY)]
        #[qproperty(bool, maximized, READ, CONSTANT)]
        #[qproperty(i32, filter_pane_width, cxx_name = "filterPaneWidth", READ, NOTIFY)]
        #[qproperty(i32, queue_pane_width, cxx_name = "queuePaneWidth", READ, NOTIFY)]
        #[qproperty(bool, queue_shown, cxx_name = "queueShown", READ, CONSTANT)]
        /// The main pane's hidden columns, by `SortColumn::name`.
        #[qproperty(QStringList, hidden_columns, cxx_name = "hiddenColumns", READ, NOTIFY)]
        #[qproperty(bool, album_grid, cxx_name = "albumGrid", READ, WRITE, NOTIFY)]
        type WindowLayout = super::WindowLayoutRust;
    }

    extern "RustQt" {
        /// Fits the saved layout to the screen (`WindowState::fitted`).
        #[qinvokable]
        fn fit(
            self: Pin<&mut WindowLayout>,
            available_width: i32,
            available_height: i32,
            main_pane_width: f64,
        );

        /// `width` and `height` are the size when not maximized.
        #[qinvokable]
        fn save(
            self: Pin<&mut WindowLayout>,
            width: f64,
            height: f64,
            maximized: bool,
            filter_pane_width: f64,
            queue_pane_width: f64,
            queue_shown: bool,
        );

        /// Shows or hides column `column` (a `SortColumn::name`).
        #[qinvokable]
        #[cxx_name = "setColumnShown"]
        fn set_column_shown(self: Pin<&mut WindowLayout>, column: &QString, shown: bool);

        /// `hidden` is passed in, though it is `hiddenColumns`, so a QML binding
        /// updates when it changes.
        #[qinvokable]
        #[cxx_name = "columnWidths"]
        fn column_widths(self: &WindowLayout, available: f64, hidden: &QStringList) -> QList_f64;
    }
}

pub struct WindowLayoutRust {
    width: i32,
    height: i32,
    maximized: bool,
    filter_pane_width: i32,
    queue_pane_width: i32,
    queue_shown: bool,
    hidden_columns: QStringList,
    album_grid: bool,
}

impl From<&WindowState> for WindowLayoutRust {
    fn from(state: &WindowState) -> Self {
        Self {
            width: qml_int(state.width),
            height: qml_int(state.height),
            maximized: state.maximized,
            filter_pane_width: qml_int(state.filter_pane_width),
            queue_pane_width: qml_int(state.queue_pane_width),
            queue_shown: state.queue_shown,
            hidden_columns: names(&state.hidden_columns),
            album_grid: state.album_grid,
        }
    }
}

impl Default for WindowLayoutRust {
    fn default() -> Self {
        Self::from(session::saved_window())
    }
}

impl qobject::WindowLayout {
    fn fit(
        mut self: Pin<&mut Self>,
        available_width: i32,
        available_height: i32,
        main_pane_width: f64,
    ) {
        let fitted = session::saved_window().clone().fitted(
            u32::try_from(available_width).unwrap_or(0),
            u32::try_from(available_height).unwrap_or(0),
            pixels(main_pane_width),
        );
        let layout = WindowLayoutRust::from(&fitted);
        set!(self, width, width_changed, layout.width);
        set!(self, height, height_changed, layout.height);
        set!(
            self,
            filter_pane_width,
            filter_pane_width_changed,
            layout.filter_pane_width
        );
        set!(
            self,
            queue_pane_width,
            queue_pane_width_changed,
            layout.queue_pane_width
        );
    }

    fn save(
        self: Pin<&mut Self>,
        width: f64,
        height: f64,
        maximized: bool,
        filter_pane_width: f64,
        queue_pane_width: f64,
        queue_shown: bool,
    ) {
        let config = crate::config();
        if config.bench {
            return;
        }
        let state = WindowState {
            width: pixels(width),
            height: pixels(height),
            maximized,
            filter_pane_width: pixels(filter_pane_width),
            queue_pane_width: pixels(queue_pane_width),
            queue_shown,
            hidden_columns: self.hidden(),
            album_grid: self.album_grid,
            browse: session::browse(),
        };
        if let Err(err) = state.save(&config.window_state_path) {
            tracing::warn!(%err, "the window layout was not saved");
        }
    }

    fn set_column_shown(mut self: Pin<&mut Self>, column: &QString, shown: bool) {
        if let Some(hidden) = with_column_shown(self.hidden(), &column.to_string(), shown) {
            set!(self, hidden_columns, hidden_columns_changed, names(&hidden));
        }
    }

    fn hidden(&self) -> BTreeSet<SortColumn> {
        columns_named(&self.hidden_columns)
    }

    fn column_widths(&self, available: f64, hidden: &QStringList) -> QList<f64> {
        column_widths(available, &columns_named(hidden))
            .into_iter()
            .collect()
    }
}

fn columns_named(names: &QStringList) -> BTreeSet<SortColumn> {
    names
        .iter()
        .filter_map(|name| SortColumn::from_name(&name.to_string()))
        .collect()
}

fn names(columns: &BTreeSet<SortColumn>) -> QStringList {
    columns
        .iter()
        .map(|column| QString::from(column.name()))
        .collect()
}

/// `hidden` with `column` shown or hidden, or `None` for a column that
/// doesn't exist or no change.
fn with_column_shown(
    mut hidden: BTreeSet<SortColumn>,
    column: &str,
    shown: bool,
) -> Option<BTreeSet<SortColumn>> {
    let column = SortColumn::from_name(column)?;
    let changed = if shown {
        hidden.remove(&column)
    } else {
        hidden.insert(column)
    };
    changed.then_some(hidden)
}

const MIN_SHRUNK_WIDTH: f64 = 40.0;

/// When the row is too narrow, hideable columns shrink, then drop out from the
/// right; only then does the title, which takes what the others leave, shrink.
fn column_widths(available: f64, hidden: &BTreeSet<SortColumn>) -> Vec<f64> {
    let available = if available.is_finite() {
        available.max(0.0)
    } else {
        0.0
    };
    let shown: Vec<&Column> = COLUMNS
        .iter()
        .filter(|size| !hidden.contains(&size.column))
        .collect();
    let natural = |size: &Column| {
        size.max_share
            .map_or(size.width, |share| size.width.min(available * share))
    };
    let mut kept = vec![true; shown.len()];
    loop {
        let gaps = SPACING * kept.iter().filter(|&&k| k).count().saturating_sub(1) as f64;
        let (mut fixed, mut flexible) = (0.0, 0.0);
        for (size, _) in shown.iter().zip(&kept).filter(|(_, k)| **k) {
            if size.hideable {
                flexible += natural(size);
            } else {
                fixed += natural(size);
            }
        }
        let scale = if flexible > 0.0 {
            ((available - fixed - gaps) / flexible).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let squeezed = shown
            .iter()
            .zip(&kept)
            .any(|(size, k)| *k && size.hideable && natural(size) * scale < MIN_SHRUNK_WIDTH);
        if squeezed
            && let Some(last) = shown
                .iter()
                .zip(&kept)
                .rposition(|(size, k)| *k && size.hideable)
        {
            kept[last] = false;
            continue;
        }
        let mut widths: Vec<f64> = shown
            .iter()
            .zip(&kept)
            .map(|(size, k)| match (k, size.hideable) {
                (false, _) => 0.0,
                (true, true) => (natural(size) * scale).floor(),
                (true, false) => natural(size),
            })
            .collect();
        if let Some(title) = shown
            .iter()
            .position(|size| size.column == SortColumn::Title)
        {
            let others: f64 = widths.iter().sum::<f64>() - widths[title];
            widths[title] = (available - gaps - others).max(0.0).floor();
        }
        return widths;
    }
}

/// QML sizes are reals. The cast saturates, and NaN becomes 0.
fn pixels(value: f64) -> u32 {
    value.round() as u32
}

#[cfg(test)]
#[path = "tests/window_layout_test.rs"]
mod tests;
