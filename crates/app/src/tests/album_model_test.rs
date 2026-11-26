use lune_core::{Grouping, Sort, SortColumn, TrackFilter};

use super::*;
use crate::test_util::{list_track as track, qml_values};

#[test]
fn cover_rows_lists_every_album_showing_each_cover() {
    // Albums 1 and 3 share cover 7; album 2 has none; album 4 has cover 8.
    let catalog = Catalog::new(vec![
        track(1, 1, Some(7)),
        track(2, 1, Some(7)),
        track(3, 2, None),
        track(4, 3, Some(7)),
        track(5, 4, Some(8)),
    ]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    let rows = cover_rows(&catalog, &view, &[0, 1, 2, 3]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[&7], [0, 2]);
    assert_eq!(rows[&8], [3]);
}

#[test]
fn cover_rows_numbers_only_the_albums_shown() {
    let catalog = Catalog::new(vec![
        track(1, 1, Some(7)),
        track(2, 2, Some(8)),
        track(3, 3, Some(9)),
    ]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    let rows = cover_rows(&catalog, &view, &[2]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[&9], [0]);
}

#[test]
fn cover_rows_of_an_empty_view_is_empty() {
    let catalog = Catalog::new(Vec::new());
    assert!(cover_rows(&catalog, &View::default(), &[]).is_empty());
}

#[test]
fn every_album_is_shown_unless_one_album_is_asked_for() {
    let catalog = Catalog::new(vec![
        track(1, 1, None),
        track(2, 2, None),
        track(3, 2, None),
    ]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    assert_eq!(shown_rows(Some(&catalog), &view, false, -1), [0, 1]);
}

#[test]
fn an_album_id_shows_only_that_albums_rows() {
    let catalog = Catalog::new(vec![
        track(1, 1, None),
        track(2, 2, None),
        track(3, 2, None),
    ]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    assert_eq!(shown_rows(Some(&catalog), &view, true, 2), [1]);
}

#[test]
fn no_album_id_shows_nothing_rather_than_every_album() {
    let catalog = Catalog::new(vec![track(1, 1, None), track(2, 2, None)]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    assert!(shown_rows(Some(&catalog), &view, true, -1).is_empty());
}

#[test]
fn an_album_not_in_the_view_or_without_a_catalog_shows_nothing() {
    let catalog = Catalog::new(vec![track(1, 1, None)]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    assert!(shown_rows(Some(&catalog), &view, true, 9).is_empty());
    assert!(shown_rows(None, &view, true, 1).is_empty());
}

#[test]
fn a_sorted_view_shows_no_covers() {
    let catalog = Catalog::new(vec![track(1, 1, Some(7)), track(2, 2, Some(8))]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    let sort = Sort {
        column: SortColumn::Title,
        descending: false,
    };
    let view = catalog.sorted(view, sort);
    assert!(cover_rows(&catalog, &view, &[0, 1]).is_empty());
}

#[test]
fn first_track_is_none_past_the_last_album_or_without_a_catalog() {
    let catalog = Catalog::new(vec![track(1, 1, None), track(2, 2, None)]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    assert_eq!(first_track(Some(&catalog), &view, 1).map(|t| t.id), Some(2));
    assert!(first_track(Some(&catalog), &view, 2).is_none());
    assert!(first_track(None, &view, 0).is_none());
}

fn lasting(ms: u64) -> ListTrack {
    ListTrack {
        duration_ms: ms,
        ..track(1, 1, None)
    }
}

#[test]
fn an_albums_duration_adds_up_its_tracks() {
    let (a, b) = (lasting(200_000), lasting(185_500));
    assert_eq!(total_duration(&[&a, &b]), "6:25");
}

#[test]
fn an_albums_duration_shows_hours_once_there_are_any() {
    let (a, b) = (lasting(3_000_000), lasting(600_000));
    assert_eq!(total_duration(&[&a, &b]), "1:00:00");
}

#[test]
fn tracks_of_unknown_length_add_nothing() {
    let (a, b) = (lasting(0), lasting(61_000));
    assert_eq!(total_duration(&[&a, &b]), "1:01");
}

#[test]
fn an_album_with_no_known_length_shows_no_duration() {
    let a = lasting(0);
    assert_eq!(total_duration(&[&a]), "");
    assert_eq!(total_duration(&[]), "");
}

#[test]
fn a_track_shows_its_own_artist_even_when_it_is_the_album_artist() {
    let mut t = track(1, 1, None);
    t.artist = Some("A".to_owned());
    assert_eq!(track_artist(&t), "A");
    t.artist = Some("Guest".to_owned());
    assert_eq!(track_artist(&t), "Guest");
}

#[test]
fn a_track_without_an_artist_shows_the_album_artist_or_unknown() {
    let mut t = track(1, 1, None);
    for blank in [None, Some("  ".to_owned())] {
        t.artist = blank;
        assert_eq!(track_artist(&t), "A");
    }
    t.album_artist = String::new();
    assert_eq!(track_artist(&t), "Unknown Artist");
}

#[test]
fn columns_qml_has_every_column_once() {
    let mut ids = qml_values(include_str!("../../qml/Columns.qml"), "id");
    ids.sort_unstable();
    let mut names = SortColumn::ALL.map(SortColumn::name);
    names.sort_unstable();
    assert_eq!(ids, names);
}

fn assert_column_texts(track: &ListTrack, texts: [(SortColumn, &str); SortColumn::ALL.len()]) {
    assert_eq!(texts.map(|(column, _)| column), SortColumn::ALL);
    let catalog = Catalog::new(vec![track.clone()]);
    for (column, text) in texts {
        assert_eq!(column_text(column, track, &catalog), text, "{column:?}");
    }
}

#[test]
fn every_column_has_its_text() {
    let mut t = track(1, 1, None);
    t.disc_no = Some(2);
    t.track_no = Some(7);
    t.artist = Some("Guest".to_owned());
    t.duration_ms = 185_500;
    t.bitrate_kbps = Some(320);
    assert_column_texts(
        &t,
        [
            (SortColumn::TrackNumber, "2.7"),
            (SortColumn::Title, "Track 1"),
            (SortColumn::Artist, "Guest"),
            (SortColumn::Duration, "3:05"),
            (SortColumn::FileType, "FLAC"),
            (SortColumn::BitRate, "320 kbps"),
        ],
    );
}

#[test]
fn unknown_values_have_empty_or_placeholder_text() {
    let mut t = track(1, 1, None);
    t.album_artist = String::new();
    assert_column_texts(
        &t,
        [
            (SortColumn::TrackNumber, ""),
            (SortColumn::Title, "Track 1"),
            (SortColumn::Artist, "Unknown Artist"),
            (SortColumn::Duration, "0:00"),
            (SortColumn::FileType, "FLAC"),
            (SortColumn::BitRate, ""),
        ],
    );
}
