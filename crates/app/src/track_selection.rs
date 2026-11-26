//! Which rows of a track list are selected, as clicks with modifier keys
//! change it. QML reads `revision` so its bindings on `contains` update.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::BTreeSet;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::{QList, QString};

use crate::property::set;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(i32, count, READ, NOTIFY)]
        /// Changes with every change to the selection.
        #[qproperty(i32, revision, READ, NOTIFY)]
        type TrackSelection = super::TrackSelectionRust;
    }

    unsafe extern "RustQt" {
        /// `extend` is Shift, `toggle` is Ctrl (Cmd on macOS).
        #[qinvokable]
        fn click(self: Pin<&mut TrackSelection>, row: i32, extend: bool, toggle: bool);

        /// Selects only `row`, unless it is already selected: what a
        /// right-click does before its menu acts on the selection.
        #[qinvokable]
        fn keep(self: Pin<&mut TrackSelection>, row: i32);

        #[qinvokable]
        #[cxx_name = "selectRange"]
        fn select_range(self: Pin<&mut TrackSelection>, start: i32, count: i32);

        #[qinvokable]
        fn clear(self: Pin<&mut TrackSelection>);

        #[qinvokable]
        fn contains(self: &TrackSelection, row: i32) -> bool;

        /// In ascending order.
        #[qinvokable]
        fn rows(self: &TrackSelection) -> QList_i32;

        /// What a drag of `row` moves, in ascending order: the selection if
        /// it holds `row`, else that row alone.
        #[qinvokable]
        fn dragged(self: &TrackSelection, row: i32) -> QList_i32;

        /// Where row `row` of a list whose last row is `last_row` shows the
        /// line for a drop before row `drop_row` (-1 for none): "above",
        /// "below", or "".
        #[qinvokable]
        #[cxx_name = "dropLine"]
        fn drop_line(self: &TrackSelection, row: i32, last_row: i32, drop_row: i32) -> QString;
    }
}

/// The selection itself, apart from Qt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rows {
    selected: BTreeSet<usize>,
    /// Where a Shift-click range starts: the last row clicked without Shift.
    anchor: Option<usize>,
}

impl Rows {
    pub fn click(&mut self, row: usize, extend: bool, toggle: bool) {
        match (extend, self.anchor) {
            (true, Some(anchor)) => {
                if !toggle {
                    self.selected.clear();
                }
                self.selected.extend(anchor.min(row)..=anchor.max(row));
            }
            _ if toggle => {
                if !self.selected.remove(&row) {
                    self.selected.insert(row);
                }
                self.anchor = Some(row);
            }
            _ => self.select_range(row, 1),
        }
    }

    pub fn keep(&mut self, row: usize) {
        if !self.selected.contains(&row) {
            self.select_range(row, 1);
        }
    }

    pub fn select_range(&mut self, start: usize, count: usize) {
        self.selected = (start..start.saturating_add(count)).collect();
        self.anchor = Some(start);
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn contains(&self, row: usize) -> bool {
        self.selected.contains(&row)
    }

    pub fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.selected.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn dragged(&self, row: usize) -> Vec<usize> {
        if self.contains(row) {
            self.rows().collect()
        } else {
            vec![row]
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropLine {
    None,
    Above,
    /// Only the last row, for a drop after it: no row is there to show it
    /// above.
    Below,
}

pub fn drop_line(row: usize, last_row: Option<usize>, drop_row: Option<usize>) -> DropLine {
    match drop_row {
        Some(drop) if drop == row => DropLine::Above,
        Some(drop) if Some(row) == last_row && drop == row + 1 => DropLine::Below,
        _ => DropLine::None,
    }
}

#[derive(Default)]
pub struct TrackSelectionRust {
    count: i32,
    revision: i32,
    rows: Rows,
}

fn index(i: i32) -> Option<usize> {
    usize::try_from(i).ok()
}

fn qml_rows(rows: impl IntoIterator<Item = usize>) -> QList<i32> {
    let mut list = QList::<i32>::default();
    for row in rows {
        list.append(i32::try_from(row).unwrap_or(i32::MAX));
    }
    list
}

impl qobject::TrackSelection {
    fn edit(mut self: Pin<&mut Self>, edit: impl FnOnce(&mut Rows)) {
        let before = self.rows.clone();
        edit(&mut self.as_mut().rust_mut().rows);
        if self.rows == before {
            return;
        }
        let count = i32::try_from(self.rows.len()).unwrap_or(i32::MAX);
        set!(self, count, count_changed, count);
        let revision = self.revision.wrapping_add(1);
        set!(self, revision, revision_changed, revision);
    }

    fn click(self: Pin<&mut Self>, row: i32, extend: bool, toggle: bool) {
        if let Some(row) = index(row) {
            self.edit(|rows| rows.click(row, extend, toggle));
        }
    }

    fn keep(self: Pin<&mut Self>, row: i32) {
        if let Some(row) = index(row) {
            self.edit(|rows| rows.keep(row));
        }
    }

    fn select_range(self: Pin<&mut Self>, start: i32, count: i32) {
        if let (Some(start), Some(count)) = (index(start), index(count)) {
            self.edit(|rows| rows.select_range(start, count));
        }
    }

    fn clear(self: Pin<&mut Self>) {
        self.edit(Rows::clear);
    }

    fn contains(&self, row: i32) -> bool {
        index(row).is_some_and(|row| self.rows.contains(row))
    }

    fn rows(&self) -> QList<i32> {
        qml_rows(self.rows.rows())
    }

    fn dragged(&self, row: i32) -> QList<i32> {
        index(row).map_or_else(QList::default, |row| qml_rows(self.rows.dragged(row)))
    }

    fn drop_line(&self, row: i32, last_row: i32, drop_row: i32) -> QString {
        let line = index(row).map_or(DropLine::None, |row| {
            drop_line(row, index(last_row), index(drop_row))
        });
        QString::from(match line {
            DropLine::None => "",
            DropLine::Above => "above",
            DropLine::Below => "below",
        })
    }
}

#[cfg(test)]
#[path = "tests/track_selection_test.rs"]
mod tests;
