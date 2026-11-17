use super::*;
use crate::test_util::list_track as track;

#[test]
fn cover_rows_lists_every_queue_row_showing_each_cover() {
    let catalog = Catalog::new(vec![
        track(1, 1, Some(7)),
        track(2, 1, Some(7)),
        track(3, 2, None),
        track(4, 3, Some(8)),
    ]);
    // Track 2 is queued twice, and an album's tracks need not be adjacent.
    let rows = cover_rows(Some(&catalog), &[2, 3, 4, 1, 2]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[&7], [0, 3, 4]);
    assert_eq!(rows[&8], [2]);
}

#[test]
fn cover_rows_skips_tracks_no_longer_in_the_catalog_but_keeps_row_numbers() {
    let catalog = Catalog::new(vec![track(1, 1, Some(7))]);
    let rows = cover_rows(Some(&catalog), &[99, 1]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[&7], [1]);
}

#[test]
fn cover_rows_of_an_empty_queue_is_empty() {
    let catalog = Catalog::new(vec![track(1, 1, Some(7))]);
    assert!(cover_rows(Some(&catalog), &[]).is_empty());
}

#[test]
fn cover_rows_without_a_catalog_is_empty() {
    assert!(cover_rows(None, &[1, 2]).is_empty());
}

fn missing_track(id: i64) -> MissingTrack {
    MissingTrack {
        id,
        path: format!("{id}.flac").into(),
        title: format!("Track {id}"),
        artist: None,
        album: "Album".to_owned(),
        missing_ns: 0,
    }
}

#[test]
fn queued_missing_keeps_only_the_queued_tracks() {
    let missing = [missing_track(1), missing_track(2), missing_track(3)];
    let queued = queued_missing(&missing, &[3, 9, 1, 3]);
    let mut ids: Vec<i64> = queued.keys().copied().collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 3]);
    assert_eq!(queued[&3].title, "Track 3");
}

#[test]
fn queued_missing_of_an_empty_queue_or_list_is_empty() {
    assert!(queued_missing(&[missing_track(1)], &[]).is_empty());
    assert!(queued_missing(&[], &[1]).is_empty());
}
