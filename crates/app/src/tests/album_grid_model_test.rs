use lune_core::{FilterValue, Grouping};

use super::*;
use crate::test_util::list_track as track;

#[test]
fn albums_fill_rows_of_columns_with_the_rest_in_the_last() {
    assert_eq!(grid_row(5, 2, 0), Some(0..2));
    assert_eq!(grid_row(5, 2, 1), Some(2..4));
    assert_eq!(grid_row(5, 2, 2), Some(4..5));
}

#[test]
fn one_column_puts_each_album_in_its_own_row() {
    assert_eq!(grid_row(3, 1, 0), Some(0..1));
    assert_eq!(grid_row(3, 1, 2), Some(2..3));
    assert_eq!(grid_row(3, 1, 3), None);
}

#[test]
fn a_single_album_fills_part_of_the_first_row() {
    assert_eq!(grid_row(1, 4, 0), Some(0..1));
    assert_eq!(grid_row(1, 4, 1), None);
}

#[test]
fn a_narrower_grid_keeps_the_first_album_of_the_row_in_view() {
    // Row 50 of 6 columns starts at album 300, which is row 100 of 3.
    assert_eq!(row_holding(50, 6, 3), 100);
}

#[test]
fn a_wider_grid_keeps_the_row_holding_the_first_album() {
    // Album 9 (row 3 of 3 columns) is in row 2 of 4 columns.
    assert_eq!(row_holding(3, 3, 4), 2);
    assert_eq!(row_holding(0, 3, 4), 0);
}

#[test]
fn an_unchanged_width_keeps_the_row() {
    assert_eq!(row_holding(7, 5, 5), 7);
}

#[test]
fn relaying_a_huge_row_does_not_overflow() {
    assert_eq!(row_holding(usize::MAX, 2, 2), usize::MAX / 2);
}

#[test]
fn there_is_no_row_past_the_last_album() {
    assert_eq!(grid_row(4, 2, 2), None);
    assert_eq!(grid_row(0, 3, 0), None);
    assert_eq!(grid_row(4, 2, usize::MAX), None);
}

#[test]
fn a_cover_is_listed_once_per_grid_row_showing_it() {
    // Albums 1, 2, and 4 share cover 7; album 3 has cover 8.
    let catalog = Catalog::new(vec![
        track(1, 1, Some(7)),
        track(2, 2, Some(7)),
        track(3, 3, Some(8)),
        track(4, 4, Some(7)),
    ]);
    let view = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let rows = grid_cover_rows(&catalog, &view, 2);
    assert_eq!(rows[&7], [0, 1]);
    assert_eq!(rows[&8], [1]);
}
