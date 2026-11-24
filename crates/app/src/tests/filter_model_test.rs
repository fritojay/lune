use super::*;
use crate::test_util::qml_values;

fn sort(column: SortColumn, descending: bool) -> Option<Sort> {
    Some(Sort { column, descending })
}

#[test]
fn each_heading_maps_to_its_column() {
    assert_eq!(
        sort_of("track_number", false),
        sort(SortColumn::TrackNumber, false)
    );
    assert_eq!(sort_of("bit_rate", true), sort(SortColumn::BitRate, true));
}

#[test]
fn album_order_and_unknown_columns_have_no_sort() {
    assert_eq!(sort_of("", false), None);
    assert_eq!(sort_of("bitrate", true), None);
}

#[test]
fn three_clicks_on_a_heading_sort_ascending_descending_then_album_order() {
    let state = after_click(None, "title").unwrap();
    assert_eq!(state, sort(SortColumn::Title, false));
    let state = after_click(state, "title").unwrap();
    assert_eq!(state, sort(SortColumn::Title, true));
    assert_eq!(after_click(state, "title"), Some(None));
}

#[test]
fn clicking_another_heading_sorts_by_it_ascending() {
    assert_eq!(
        after_click(sort(SortColumn::Title, true), "artist"),
        Some(sort(SortColumn::Artist, false))
    );
}

#[test]
fn clicking_a_heading_that_does_not_exist_changes_nothing() {
    assert_eq!(after_click(sort(SortColumn::Title, true), "nope"), None);
    assert_eq!(after_click(None, ""), None);
}

#[test]
fn the_menu_sets_any_column_and_direction() {
    assert_eq!(
        chosen(None, "artist", true),
        Some(sort(SortColumn::Artist, true))
    );
    assert_eq!(
        chosen(sort(SortColumn::Title, true), "track_number", false),
        Some(sort(SortColumn::TrackNumber, false))
    );
    assert_eq!(
        chosen(sort(SortColumn::Title, false), "title", true),
        Some(sort(SortColumn::Title, true))
    );
}

#[test]
fn the_menu_restores_album_order() {
    assert_eq!(
        chosen(sort(SortColumn::Duration, true), "", true),
        Some(None)
    );
}

#[test]
fn the_menu_ignores_a_column_that_does_not_exist() {
    assert_eq!(chosen(None, "nope", false), None);
    assert_eq!(chosen(sort(SortColumn::Title, false), "-1", false), None);
}

#[test]
fn the_menu_ignores_the_sort_already_in_effect() {
    assert_eq!(chosen(sort(SortColumn::Artist, true), "artist", true), None);
    assert_eq!(chosen(None, "", false), None);
    assert_eq!(chosen(None, "", true), None);
}

fn album(descending: bool) -> Option<MenuChoice> {
    Some(MenuChoice::AlbumDescending(descending))
}

fn column(sort: Option<Sort>) -> Option<MenuChoice> {
    Some(MenuChoice::Sort(sort))
}

#[test]
fn in_album_order_the_menu_sets_its_direction() {
    assert_eq!(menu_choice(None, false, true, "", true), album(true));
    assert_eq!(menu_choice(None, true, true, "", false), album(false));
}

#[test]
fn in_album_order_the_menu_ignores_the_direction_in_effect() {
    assert_eq!(menu_choice(None, false, true, "", false), None);
    assert_eq!(menu_choice(None, true, true, "", true), None);
}

#[test]
fn the_menu_ignores_album_direction_in_groupings_without_one() {
    assert_eq!(menu_choice(None, false, false, "", true), None);
    assert_eq!(menu_choice(None, true, false, "", false), None);
}

#[test]
fn from_a_column_sort_album_order_comes_back_without_changing_its_direction() {
    for album_descending in [false, true] {
        for descending in [false, true] {
            assert_eq!(
                menu_choice(
                    sort(SortColumn::Title, true),
                    album_descending,
                    true,
                    "",
                    descending
                ),
                column(None)
            );
        }
    }
}

#[test]
fn the_menu_sorts_by_a_column_in_any_album_direction() {
    for album_descending in [false, true] {
        assert_eq!(
            menu_choice(None, album_descending, true, "artist", false),
            column(sort(SortColumn::Artist, false))
        );
        assert_eq!(
            menu_choice(None, album_descending, false, "artist", true),
            column(sort(SortColumn::Artist, true))
        );
    }
    assert_eq!(menu_choice(None, true, true, "nope", true), None);
}

#[test]
fn groupings_qml_has_every_grouping_once() {
    let mut ids = qml_values(include_str!("../../qml/Groupings.qml"), "id");
    ids.sort_unstable();
    let mut names = Grouping::ALL.map(Grouping::name);
    names.sort_unstable();
    assert_eq!(ids, names);
}

#[test]
fn a_saved_sort_maps_back_to_the_same_sort() {
    for descending in [false, true] {
        let saved = sort(SortColumn::FileType, descending);
        let (column, descending) = sort_state(saved, !descending);
        assert_eq!(sort_of(column, descending), saved);
    }
}

#[test]
fn album_order_is_saved_as_no_column_in_its_direction() {
    assert_eq!(sort_state(None, false), ("", false));
    assert_eq!(sort_state(None, true), ("", true));
}
