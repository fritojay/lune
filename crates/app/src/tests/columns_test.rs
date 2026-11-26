use proptest::prelude::*;
use proptest::sample::select;

use super::*;
use crate::test_util::entry_value;

fn ids(entries: &QList<QVariant>) -> Vec<String> {
    entries.iter().map(|entry| text(entry, "id")).collect()
}

fn text(entry: &QVariant, key: &str) -> String {
    entry_value(entry, key)
        .and_then(|value| value.value::<QString>())
        .unwrap()
        .to_string()
}

#[test]
fn every_column_is_listed_once() {
    let mut listed: Vec<SortColumn> = COLUMNS.iter().map(|column| column.column).collect();
    listed.sort_unstable();
    assert_eq!(listed, SortColumn::ALL);
}

#[test]
fn an_entry_says_how_its_column_shows() {
    let columns = ColumnsRust::default();
    assert_eq!(
        ids(&columns.all),
        COLUMNS.map(|column| column.column.name())
    );
    for (entry, column) in columns.all.iter().zip(&COLUMNS) {
        assert_eq!(text(entry, "title"), column.title);
        assert_eq!(text(entry, "menuTitle"), column.menu_title());
        for (key, value) in [
            ("alignRight", column.align_right),
            ("marksPlaying", column.marks_playing),
            ("primary", column.primary),
            ("hideable", column.hideable),
        ] {
            let read = entry_value(entry, key).and_then(|v| v.value::<bool>());
            assert_eq!(read, Some(value), "{} {key}", column.title);
        }
    }
    let menu_titles: Vec<String> = columns.all.iter().map(|e| text(e, "menuTitle")).collect();
    assert_eq!(menu_titles[..2], ["Track Number", "Title"]);
}

#[test]
fn the_hideable_columns_are_listed_in_order() {
    let expected: Vec<&str> = COLUMNS
        .iter()
        .filter(|column| column.hideable)
        .map(|column| column.column.name())
        .collect();
    assert_eq!(ids(&ColumnsRust::default().hideable), expected);
}

proptest! {
    #[test]
    fn the_shown_columns_are_the_others_in_order(
        hidden in prop::collection::vec(
            prop_oneof![select(&SortColumn::ALL[..]).prop_map(|c| c.name().to_owned()), Just("nope".to_owned())],
            0..8,
        ),
    ) {
        let expected: Vec<&str> = COLUMNS
            .iter()
            .map(|column| column.column.name())
            .filter(|name| !hidden.iter().any(|h| h == name))
            .collect();
        prop_assert_eq!(ids(&entries(shown(&hidden))), expected);
    }
}
