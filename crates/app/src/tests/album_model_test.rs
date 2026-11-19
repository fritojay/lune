use lune_core::{FilterValue, Grouping, Sort, SortColumn};

use super::*;
use crate::test_util::list_track as track;

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
    let view = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let rows = cover_rows(&catalog, &view);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[&7], [0, 2]);
    assert_eq!(rows[&8], [3]);
}

#[test]
fn cover_rows_of_an_empty_view_is_empty() {
    let catalog = Catalog::new(Vec::new());
    assert!(cover_rows(&catalog, &View::default()).is_empty());
}

#[test]
fn a_sorted_view_shows_no_covers() {
    let catalog = Catalog::new(vec![track(1, 1, Some(7)), track(2, 2, Some(8))]);
    let view = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let sort = Sort {
        column: SortColumn::Title,
        descending: false,
    };
    assert!(cover_rows(&catalog, &catalog.sorted(view, sort)).is_empty());
}

#[test]
fn first_track_is_none_past_the_last_album_or_without_a_catalog() {
    let catalog = Catalog::new(vec![track(1, 1, None), track(2, 2, None)]);
    let view = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
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

/// Each column's `key: "value"` in `Columns.qml`, in order.
fn qml_columns(key: &str) -> Vec<&'static str> {
    const QML: &str = include_str!("../../qml/Columns.qml");
    let pattern = format!("{key}: \"");
    QML.match_indices(&pattern)
        .map(|(at, _)| {
            let value = &QML[at + pattern.len()..];
            &value[..value.find('"').unwrap()]
        })
        .collect()
}

#[test]
fn columns_qml_has_every_column_once_with_a_role_that_exists() {
    let ids = qml_columns("id");
    let columns: Vec<_> = ids
        .iter()
        .map(|id| SortColumn::from_name(id).unwrap_or_else(|| panic!("no column {id:?}")))
        .collect();
    let mut sorted = columns.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted,
        [
            SortColumn::TrackNumber,
            SortColumn::Title,
            SortColumn::Artist,
            SortColumn::Duration,
            SortColumn::FileType,
            SortColumn::BitRate,
        ]
    );
    let roles = qml_columns("role");
    assert_eq!(roles.len(), ids.len());
    for role in roles {
        assert!(
            ROLES.iter().any(|&(_, name)| name == role),
            "no role {role:?}"
        );
    }
}
