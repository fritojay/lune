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
fn groupings_qml_has_every_grouping_once() {
    let mut ids = qml_values(include_str!("../../qml/Groupings.qml"), "id");
    ids.sort_unstable();
    let mut names = Grouping::ALL.map(Grouping::name);
    names.sort_unstable();
    assert_eq!(ids, names);
}

fn every_sort() -> impl Iterator<Item = Option<Sort>> {
    SortColumn::ALL
        .into_iter()
        .flat_map(|column| [false, true].map(|descending| sort(column, descending)))
        .chain([None])
}

/// Every column name QML can send: real ones, album order, and junk.
fn every_name() -> impl Iterator<Item = &'static str> {
    SortColumn::ALL
        .into_iter()
        .map(SortColumn::name)
        .chain(["", "nope", "-1"])
}

#[test]
fn a_heading_click_on_a_known_column_follows_the_click_cycle() {
    let title = after_click(None, "title").unwrap();
    assert_eq!(title, sort(SortColumn::Title, false));
    let title = after_click(title, "title").unwrap();
    assert_eq!(title, sort(SortColumn::Title, true));
    assert_eq!(after_click(title, "title"), Some(None));
    assert_eq!(
        after_click(title, "artist"),
        Some(sort(SortColumn::Artist, false))
    );

    for current in every_sort() {
        for name in every_name() {
            let expected = SortColumn::from_name(name).map(|c| Sort::after_click(current, c));
            assert_eq!(after_click(current, name), expected, "{current:?} {name:?}");
        }
    }
}

#[test]
fn the_menu_reaches_what_was_picked_and_reports_only_changes() {
    let album = |descending| Some(MenuChoice::AlbumDescending(descending));
    let column = |sort| Some(MenuChoice::Sort(sort));
    assert_eq!(menu_choice(None, false, true, "", true), album(true));
    assert_eq!(menu_choice(None, false, true, "", false), None);
    assert_eq!(menu_choice(None, false, false, "", true), None);
    let title = sort(SortColumn::Title, true);
    assert_eq!(menu_choice(title, false, true, "", true), column(None));
    assert_eq!(
        menu_choice(None, true, false, "artist", true),
        column(sort(SortColumn::Artist, true))
    );
    assert_eq!(menu_choice(None, true, true, "nope", true), None);

    for current in every_sort() {
        for album_descending in [false, true] {
            for directional in [false, true] {
                for name in every_name() {
                    for descending in [false, true] {
                        let case = format!(
                            "{current:?} {album_descending} {directional} {name:?} {descending}"
                        );
                        let choice =
                            menu_choice(current, album_descending, directional, name, descending);
                        let picked = match name {
                            "" => None,
                            name => match SortColumn::from_name(name) {
                                Some(column) => Some(Sort { column, descending }),
                                None => {
                                    assert_eq!(choice, None, "{case}");
                                    continue;
                                }
                            },
                        };
                        let picked_direction =
                            if picked.is_none() && current.is_none() && directional {
                                descending
                            } else {
                                album_descending
                            };
                        let after = match choice {
                            Some(MenuChoice::Sort(sort)) => (sort, album_descending),
                            Some(MenuChoice::AlbumDescending(d)) => (current, d),
                            None => (current, album_descending),
                        };
                        assert_eq!(after, (picked, picked_direction), "{case}");
                        assert_eq!(
                            choice.is_some(),
                            after != (current, album_descending),
                            "{case}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_saved_sort_or_album_direction_maps_back_to_itself() {
    for saved in every_sort() {
        for album_descending in [false, true] {
            let (column, descending) = sort_state(saved, album_descending);
            assert_eq!(sort_of(column, descending), saved);
            if saved.is_none() {
                assert_eq!((column, descending), ("", album_descending));
            }
        }
    }
}
