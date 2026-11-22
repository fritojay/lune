use super::*;
use crate::test_util::qml_values;

#[test]
fn qml_sizes_round_to_whole_pixels() {
    assert_eq!(pixels(299.4), 299);
    assert_eq!(pixels(299.5), 300);
    assert_eq!(pixels(0.0), 0);
}

#[test]
fn out_of_range_qml_sizes_saturate() {
    assert_eq!(pixels(-12.0), 0);
    assert_eq!(pixels(f64::NAN), 0);
    assert_eq!(pixels(f64::INFINITY), u32::MAX);
}

#[test]
fn hiding_a_column_adds_it_and_showing_it_removes_it() {
    assert_eq!(
        with_column_shown(BTreeSet::new(), "file_type", false),
        Some([SortColumn::FileType].into())
    );
    let hidden = [SortColumn::BitRate, SortColumn::FileType].into();
    assert_eq!(
        with_column_shown(hidden, "file_type", true),
        Some([SortColumn::BitRate].into())
    );
}

#[test]
fn a_column_already_in_that_state_changes_nothing() {
    assert_eq!(
        with_column_shown([SortColumn::FileType].into(), "file_type", false),
        None
    );
    assert_eq!(with_column_shown(BTreeSet::new(), "file_type", true), None);
}

#[test]
fn a_column_that_does_not_exist_changes_nothing() {
    assert_eq!(with_column_shown(BTreeSet::new(), "nope", false), None);
    assert_eq!(with_column_shown(BTreeSet::new(), "", false), None);
}

#[test]
fn saved_hidden_columns_reach_qml_by_name() {
    let state = WindowState {
        hidden_columns: [SortColumn::Artist, SortColumn::BitRate].into(),
        ..WindowState::default()
    };
    let names: Vec<String> = WindowLayoutRust::from(&state)
        .hidden_columns
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["artist", "bit_rate"]);
}

fn widths(available: f64, hidden: &[SortColumn]) -> Vec<f64> {
    column_widths(available, &hidden.iter().copied().collect())
}

/// Display order: #, title, artist, file type, bit rate, time.
#[test]
fn a_wide_row_gives_the_title_what_the_others_leave() {
    assert_eq!(widths(1000.0, &[]), [32.0, 494.0, 220.0, 76.0, 72.0, 56.0]);
}

#[test]
fn the_artist_takes_at_most_a_quarter_of_the_row() {
    assert_eq!(widths(800.0, &[])[2], 200.0);
}

#[test]
fn a_narrow_row_shrinks_the_hideable_columns_before_the_title() {
    let w = widths(500.0, &[]);
    assert!((120.0..125.0).contains(&w[1]), "{w:?}");
    assert_eq!([w[0], w[5]], [32.0, 56.0]);
    assert!(
        w[2..5]
            .iter()
            .all(|x| (MIN_SHRUNK_WIDTH..125.0).contains(x)),
        "{w:?}"
    );
    assert!(
        w.iter().sum::<f64>() + 5.0 * COLUMN_SPACING <= 500.0,
        "{w:?}"
    );
}

#[test]
fn squeezed_hideable_columns_drop_from_the_right_with_their_gap() {
    let w = widths(380.0, &[]);
    assert_eq!(w[4], 0.0, "bit rate goes first: {w:?}");
    assert!(w[2] > 0.0 && w[3] > 0.0, "{w:?}");
    let gaps = 4.0 * COLUMN_SPACING;
    assert!(w.iter().sum::<f64>() + gaps <= 380.0, "{w:?}");
}

#[test]
fn the_title_shrinks_only_once_every_hideable_column_is_gone() {
    assert_eq!(widths(206.0, &[]), [32.0, 98.0, 0.0, 0.0, 0.0, 56.0]);
}

#[test]
fn the_number_and_time_keep_their_width_even_when_nothing_fits() {
    assert_eq!(widths(50.0, &[]), [32.0, 0.0, 0.0, 0.0, 0.0, 56.0]);
}

#[test]
fn hidden_columns_get_no_width_entry() {
    assert_eq!(
        widths(1000.0, &[SortColumn::FileType, SortColumn::BitRate]),
        [32.0, 662.0, 220.0, 56.0]
    );
}

#[test]
fn a_row_with_no_hideable_columns_shown_still_fills() {
    let hidden = [
        SortColumn::Artist,
        SortColumn::FileType,
        SortColumn::BitRate,
    ];
    assert_eq!(widths(300.0, &hidden), [32.0, 192.0, 56.0]);
}

#[test]
fn a_meaningless_width_is_treated_as_zero() {
    for available in [f64::NAN, f64::NEG_INFINITY, f64::INFINITY, -10.0] {
        assert_eq!(widths(available, &[]), [32.0, 0.0, 0.0, 0.0, 0.0, 56.0]);
    }
}

#[test]
fn columns_qml_matches_the_rust_column_sizes() {
    let qml = include_str!("../../qml/Columns.qml");
    let order: Vec<&str> = COLUMN_SIZES.iter().map(|size| size.column.name()).collect();
    assert_eq!(qml_values(qml, "id"), order);
    let hideable: Vec<&str> = qml
        .split("{ id: \"")
        .skip(1)
        .filter(|entry| entry.contains("hideable: true"))
        .map(|entry| &entry[..entry.find('"').unwrap()])
        .collect();
    let expected: Vec<&str> = COLUMN_SIZES
        .iter()
        .filter(|size| size.hideable)
        .map(|size| size.column.name())
        .collect();
    assert_eq!(hideable, expected);
    assert!(qml.contains(&format!("readonly property int spacing: {COLUMN_SPACING}")));
}
