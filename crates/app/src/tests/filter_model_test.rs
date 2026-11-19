use super::*;

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

#[test]
fn every_grouping_maps_back_to_its_index() {
    for (index, grouping) in GROUPINGS.into_iter().enumerate() {
        assert_eq!(grouping_index(grouping), i32::try_from(index).unwrap());
    }
}

#[test]
fn a_saved_sort_maps_back_to_the_same_sort() {
    for descending in [false, true] {
        let saved = sort(SortColumn::FileType, descending);
        let (column, descending) = sort_state(saved);
        assert_eq!(sort_of(column, descending), saved);
    }
}

#[test]
fn album_order_is_saved_as_no_column() {
    assert_eq!(sort_state(None), ("", false));
}
