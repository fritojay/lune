use lune_core::{Grouping, TrackFilter};

use proptest::prelude::*;

use super::*;
use crate::test_util::list_track as track;

#[test]
fn relaying_a_huge_row_does_not_overflow() {
    assert_eq!(row_holding(usize::MAX, 2, 2), usize::MAX / 2);
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
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    let rows = grid_cover_rows(&catalog, &view, 2);
    assert_eq!(rows[&7], [0, 1]);
    assert_eq!(rows[&8], [1]);
}

proptest! {
    #[test]
    fn rows_of_full_width_hold_every_album_once_in_order(albums in 0..50usize, columns in 1..8usize) {
        let mut next = 0;
        let mut row = 0;
        while let Some(albums_in_row) = grid_row(albums, columns, row) {
            prop_assert_eq!(albums_in_row.start, next);
            prop_assert!(albums_in_row.len() == columns || albums_in_row.end == albums);
            prop_assert!(!albums_in_row.is_empty());
            next = albums_in_row.end;
            row += 1;
        }
        prop_assert_eq!(next, albums);
        prop_assert_eq!(grid_row(albums, columns, usize::MAX), None);
    }

    #[test]
    fn a_relaid_row_holds_the_first_album_of_the_old_row(
        row in 0..10_000usize,
        old_columns in 1..12usize,
        new_columns in 1..12usize,
    ) {
        let first_album = row * old_columns;
        let new_row = row_holding(row, old_columns, new_columns);
        prop_assert!(new_row * new_columns <= first_album && first_album < (new_row + 1) * new_columns);
    }
}
