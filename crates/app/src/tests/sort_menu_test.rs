use lune_core::{MenuChoice, Sort, SortColumn, menu_choice, sort_state};

use super::*;

/// Every sort the menu can show: album order in either direction, or a
/// column sort in either, with or without an album direction to pick.
fn states() -> Vec<(Option<Sort>, bool, bool)> {
    let sorts = std::iter::once(None).chain(
        SortColumn::ALL
            .into_iter()
            .flat_map(|column| [false, true].map(|descending| Some(Sort { column, descending }))),
    );
    sorts
        .flat_map(|sort| {
            [false, true].into_iter().flat_map(move |album_descending| {
                [false, true].map(|directional| (sort, album_descending, directional))
            })
        })
        .collect()
}

fn menu_for(sort: Option<Sort>, album_descending: bool, directional: bool) -> SortMenu {
    let (column, descending) = sort_state(sort, album_descending);
    sort_menu(column, descending, directional)
}

fn items(menu: &SortMenu) -> impl Iterator<Item = &SortItem> {
    std::iter::once(&menu.album_order)
        .chain(&menu.columns)
        .chain(&menu.directions)
}

#[test]
fn the_menu_checks_the_sort_in_effect_and_its_direction_when_it_has_one() {
    for (sort, album_descending, directional) in states() {
        let menu = menu_for(sort, album_descending, directional);
        let state = format!("{sort:?} {album_descending} {directional}");
        let orders: Vec<&str> = std::iter::once(&menu.album_order)
            .chain(&menu.columns)
            .filter(|item| item.checked)
            .map(|item| item.column)
            .collect();
        assert_eq!(orders, [sort.map_or("", |s| s.column.name())], "{state}");
        let has_direction = sort.is_some() || directional;
        let descending = sort.map_or(album_descending, |s| s.descending);
        for item in &menu.directions {
            assert_eq!(item.enabled, has_direction, "{state}");
            assert_eq!(
                item.checked,
                has_direction && item.descending == descending,
                "{state}"
            );
        }
        assert!(
            std::iter::once(&menu.album_order)
                .chain(&menu.columns)
                .all(|item| item.enabled),
            "{state}"
        );
    }
}

#[test]
fn picking_an_item_checks_it_and_picking_a_checked_one_changes_nothing() {
    for (sort, album_descending, directional) in states() {
        let menu = menu_for(sort, album_descending, directional);
        for item in items(&menu).filter(|item| item.enabled) {
            let picked = format!(
                "{} from {sort:?} {album_descending} {directional}",
                item.text
            );
            let choice = menu_choice(
                sort,
                album_descending,
                directional,
                item.column,
                item.descending,
            );
            if item.checked {
                assert_eq!(choice, None, "{picked}");
                continue;
            }
            let (sort, album_descending) = match choice {
                Some(MenuChoice::Sort(sort)) => (sort, album_descending),
                Some(MenuChoice::AlbumDescending(descending)) => (sort, descending),
                None => panic!("{picked} changed nothing"),
            };
            let after = menu_for(sort, album_descending, directional);
            let now = items(&after).find(|i| i.text == item.text).unwrap();
            assert!(now.checked, "{picked}");
        }
    }
}

#[test]
fn a_column_picked_from_album_order_sorts_ascending_and_from_a_sort_keeps_its_direction() {
    let menu = sort_menu("", true, true);
    assert!(menu.columns.iter().all(|item| !item.descending));
    assert!(menu.album_order.descending);
    let menu = sort_menu("title", true, true);
    assert!(menu.columns.iter().all(|item| item.descending));
}

#[test]
fn the_sorted_heading_is_marked_with_its_direction() {
    for column in std::iter::once("").chain(SortColumn::ALL.map(SortColumn::name)) {
        for descending in [false, true] {
            for (name, text, active) in headings(column, descending) {
                let title = COLUMNS
                    .iter()
                    .find(|def| def.column.name() == name)
                    .unwrap()
                    .title;
                assert_eq!(active, name == column);
                let mark = match (active, descending) {
                    (false, _) => String::new(),
                    (true, false) => " ▲".to_owned(),
                    (true, true) => " ▼".to_owned(),
                };
                assert_eq!(text, format!("{title}{mark}"));
            }
        }
    }
}
