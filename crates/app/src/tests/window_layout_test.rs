use super::*;

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
