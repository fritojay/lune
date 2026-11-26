//! The sort menu's items and the column headings' sort marks, for a column
//! sort or album order. A pick goes back through `FilterModel.setSort`
//! (`lune_core::menu_choice`).

use cxx_qt_lib::{QList, QMap, QMapPair_QString_QVariant, QString, QVariant};

use crate::columns::COLUMNS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortItem {
    pub text: &'static str,
    /// What a pick sends: a `SortColumn::name`, or "" for album order.
    pub column: &'static str,
    pub descending: bool,
    pub checked: bool,
    pub enabled: bool,
}

/// Album order, the columns, then the two directions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortMenu {
    pub album_order: SortItem,
    pub columns: Vec<SortItem>,
    pub directions: [SortItem; 2],
}

/// The menu for the sort `column` ("" in album order) and its direction.
/// `album_directional` says whether album order has a direction to pick.
pub fn sort_menu(column: &'static str, descending: bool, album_directional: bool) -> SortMenu {
    let sorted = !column.is_empty();
    let directional = sorted || album_directional;
    let direction = |text, down: bool| SortItem {
        text,
        column,
        descending: down,
        checked: directional && descending == down,
        enabled: directional,
    };
    SortMenu {
        album_order: SortItem {
            text: "Album Order",
            column: "",
            // Album order's direction is its own, kept apart from a column
            // sort's.
            descending,
            checked: !sorted,
            enabled: true,
        },
        columns: COLUMNS
            .iter()
            .map(|def| {
                let name = def.column.name();
                SortItem {
                    text: def.menu_title(),
                    column: name,
                    descending: sorted && descending,
                    checked: name == column,
                    enabled: true,
                }
            })
            .collect(),
        directions: [direction("Ascending", false), direction("Descending", true)],
    }
}

/// Each column heading's text by `SortColumn::name`, the sorted one marked
/// with its direction, and whether it is the sorted one.
pub fn headings(column: &str, descending: bool) -> Vec<(&'static str, String, bool)> {
    COLUMNS
        .iter()
        .map(|def| {
            let name = def.column.name();
            let active = name == column;
            let mark = match (active, descending) {
                (false, _) => "",
                (true, false) => " ▲",
                (true, true) => " ▼",
            };
            (name, format!("{}{mark}", def.title), active)
        })
        .collect()
}

/// `{ albumOrder, columns, directions }` of items `{ text, column,
/// descending, checked, enabled }`.
pub fn menu_variant(menu: &SortMenu) -> QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(QString::from("albumOrder"), item_variant(&menu.album_order));
    map.insert(QString::from("columns"), items_variant(&menu.columns));
    map.insert(QString::from("directions"), items_variant(&menu.directions));
    QVariant::from(&map)
}

/// By `SortColumn::name`, `{ text, active }`.
pub fn headings_variant(headings: &[(&'static str, String, bool)]) -> QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    for (name, text, active) in headings {
        let mut heading = QMap::<QMapPair_QString_QVariant>::default();
        heading.insert(QString::from("text"), QVariant::from(&QString::from(text)));
        heading.insert(QString::from("active"), QVariant::from(active));
        map.insert(QString::from(*name), QVariant::from(&heading));
    }
    QVariant::from(&map)
}

fn items_variant(items: &[SortItem]) -> QVariant {
    let list: QList<QVariant> = items.iter().map(item_variant).collect();
    QVariant::from(&list)
}

fn item_variant(item: &SortItem) -> QVariant {
    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(
        QString::from("text"),
        QVariant::from(&QString::from(item.text)),
    );
    map.insert(
        QString::from("column"),
        QVariant::from(&QString::from(item.column)),
    );
    map.insert(
        QString::from("descending"),
        QVariant::from(&item.descending),
    );
    map.insert(QString::from("checked"), QVariant::from(&item.checked));
    map.insert(QString::from("enabled"), QVariant::from(&item.enabled));
    QVariant::from(&map)
}

#[cfg(test)]
#[path = "tests/sort_menu_test.rs"]
mod tests;
