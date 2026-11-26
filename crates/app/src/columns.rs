//! The main pane's columns, in display order: what QML shows for each (the
//! `Columns` singleton) and how `WindowLayout` sizes them.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::BTreeSet;

use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QStringList, QVariant};
use lune_core::SortColumn;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        /// Every column as an entry: `{ id, title, menuTitle, alignRight,
        /// marksPlaying, primary, hideable }`, `id` being its
        /// `SortColumn::name`.
        #[qproperty(QList_QVariant, all, READ, CONSTANT)]
        #[qproperty(QList_QVariant, hideable, READ, CONSTANT)]
        /// Between two cells of a row.
        #[qproperty(f64, spacing, READ, CONSTANT)]
        type Columns = super::ColumnsRust;
    }

    extern "RustQt" {
        /// The columns not in `hidden` (`SortColumn::name`s), which are the
        /// ones `WindowLayout.columnWidths` sizes. Rows build only these, so
        /// a hidden column costs nothing per row.
        #[qinvokable]
        fn shown(self: &Columns, hidden: &QStringList) -> QList_QVariant;
    }
}

pub struct ColumnsRust {
    all: QList<QVariant>,
    hideable: QList<QVariant>,
    spacing: f64,
}

impl Default for ColumnsRust {
    fn default() -> Self {
        Self {
            all: entries(COLUMNS.iter()),
            hideable: entries(COLUMNS.iter().filter(|column| column.hideable)),
            spacing: SPACING,
        }
    }
}

impl qobject::Columns {
    fn shown(&self, hidden: &QStringList) -> QList<QVariant> {
        entries(shown(&named(hidden)))
    }
}

pub struct Column {
    pub column: SortColumn,
    pub title: &'static str,
    /// In menus, where a heading's short title wouldn't say enough.
    pub menu_title: Option<&'static str>,
    pub align_right: bool,
    /// Shows the now-playing mark in place of its text.
    pub marks_playing: bool,
    /// Its text stands out from the others'.
    pub primary: bool,
    pub width: f64,
    /// The most of the row it takes, when that is less than `width`.
    pub max_share: Option<f64>,
    pub hideable: bool,
}

impl Column {
    pub fn menu_title(&self) -> &'static str {
        self.menu_title.unwrap_or(self.title)
    }
}

const PLAIN: Column = Column {
    column: SortColumn::Title,
    title: "",
    menu_title: None,
    align_right: false,
    marks_playing: false,
    primary: false,
    width: 0.0,
    max_share: None,
    hideable: false,
};

pub const COLUMNS: [Column; 6] = [
    Column {
        column: SortColumn::TrackNumber,
        title: "#",
        menu_title: Some("Track Number"),
        align_right: true,
        marks_playing: true,
        width: 32.0,
        ..PLAIN
    },
    Column {
        column: SortColumn::Title,
        title: "Title",
        primary: true,
        width: 120.0,
        ..PLAIN
    },
    Column {
        column: SortColumn::Artist,
        title: "Artist",
        width: 220.0,
        max_share: Some(0.25),
        hideable: true,
        ..PLAIN
    },
    Column {
        column: SortColumn::FileType,
        title: "File Type",
        width: 76.0,
        hideable: true,
        ..PLAIN
    },
    Column {
        column: SortColumn::BitRate,
        title: "Bit Rate",
        align_right: true,
        width: 72.0,
        hideable: true,
        ..PLAIN
    },
    Column {
        column: SortColumn::Duration,
        title: "Time",
        align_right: true,
        width: 56.0,
        ..PLAIN
    },
];

pub const SPACING: f64 = 10.0;

pub fn shown(hidden: &BTreeSet<SortColumn>) -> impl Iterator<Item = &'static Column> + '_ {
    COLUMNS
        .iter()
        .filter(|column| !hidden.contains(&column.column))
}

/// Names of no column are left out.
pub fn named(names: &QStringList) -> BTreeSet<SortColumn> {
    names
        .iter()
        .filter_map(|name| SortColumn::from_name(&name.to_string()))
        .collect()
}

fn entries<'a>(columns: impl Iterator<Item = &'a Column>) -> QList<QVariant> {
    columns.map(entry).collect()
}

fn entry(column: &Column) -> QVariant {
    let mut entry = QMap::<QMapPair_QString_QVariant>::default();
    let mut insert = |key: &str, value: QVariant| entry.insert(QString::from(key), value);
    insert("id", QVariant::from(&QString::from(column.column.name())));
    insert("title", QVariant::from(&QString::from(column.title)));
    insert(
        "menuTitle",
        QVariant::from(&QString::from(column.menu_title())),
    );
    insert("alignRight", QVariant::from(&column.align_right));
    insert("marksPlaying", QVariant::from(&column.marks_playing));
    insert("primary", QVariant::from(&column.primary));
    insert("hideable", QVariant::from(&column.hideable));
    QVariant::from(&entry)
}

#[cfg(test)]
#[path = "tests/columns_test.rs"]
mod tests;
