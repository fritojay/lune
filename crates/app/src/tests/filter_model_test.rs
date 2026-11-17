use super::*;

#[test]
fn each_heading_maps_to_its_column() {
    let columns: Vec<_> = (0..4)
        .map(|c| sort_of(c, false).map(|s| s.column))
        .collect();
    assert_eq!(
        columns,
        [
            Some(SortColumn::TrackNumber),
            Some(SortColumn::Title),
            Some(SortColumn::Artist),
            Some(SortColumn::Duration),
        ]
    );
}

#[test]
fn album_order_and_unknown_columns_have_no_sort() {
    assert_eq!(sort_of(-1, false), None);
    assert_eq!(sort_of(4, true), None);
    assert_eq!(sort_of(i32::MIN, false), None);
}

#[test]
fn three_clicks_on_a_heading_sort_ascending_descending_then_album_order() {
    let state = after_click(-1, false, 1).unwrap();
    assert_eq!(state, (1, false));
    let state = after_click(state.0, state.1, 1).unwrap();
    assert_eq!(state, (1, true));
    assert_eq!(after_click(state.0, state.1, 1), Some((-1, false)));
}

#[test]
fn clicking_another_heading_sorts_by_it_ascending() {
    assert_eq!(after_click(1, true, 2), Some((2, false)));
    assert_eq!(after_click(2, false, 0), Some((0, false)));
}

#[test]
fn clicking_a_heading_that_does_not_exist_changes_nothing() {
    assert_eq!(after_click(1, true, 4), None);
    assert_eq!(after_click(-1, false, -1), None);
}

#[test]
fn the_menu_sets_any_column_and_direction() {
    assert_eq!(chosen((-1, false), 2, true), Some((2, true)));
    assert_eq!(chosen((1, true), 0, false), Some((0, false)));
    assert_eq!(chosen((1, false), 1, true), Some((1, true)));
}

#[test]
fn the_menu_restores_album_order() {
    assert_eq!(chosen((3, true), -1, true), Some((-1, false)));
}

#[test]
fn the_menu_ignores_a_column_that_does_not_exist() {
    assert_eq!(chosen((-1, false), 4, false), None);
    assert_eq!(chosen((1, false), -2, false), None);
}

#[test]
fn the_menu_ignores_the_sort_already_in_effect() {
    assert_eq!(chosen((2, true), 2, true), None);
    assert_eq!(chosen((-1, false), -1, false), None);
    assert_eq!(chosen((-1, false), -1, true), None);
}

#[test]
fn every_grouping_maps_back_to_its_index() {
    for (index, grouping) in GROUPINGS.into_iter().enumerate() {
        assert_eq!(grouping_index(grouping), i32::try_from(index).unwrap());
    }
}

#[test]
fn every_saved_sort_maps_back_to_the_same_sort() {
    for column in SORT_COLUMNS {
        for descending in [false, true] {
            let sort = Some(Sort { column, descending });
            let (column, descending) = sort_state(sort);
            assert_eq!(sort_of(column, descending), sort);
        }
    }
}

#[test]
fn album_order_is_saved_as_no_column() {
    assert_eq!(sort_state(None), (-1, false));
}
