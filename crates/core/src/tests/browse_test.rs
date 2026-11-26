use std::path::PathBuf;

use proptest::prelude::*;
use proptest::sample::Index;

use super::*;
use crate::test_util::{
    arb_filter_value, arb_grouping, arb_list_id, arb_search, arb_sort, arb_sort_column, arb_tracks,
};

/// Tracks must be passed in catalog (album artist) order, like `track_list`.
fn track(id: i64, album_artist: &str, album_id: i64, album: &str, year: Option<i32>) -> ListTrack {
    ListTrack {
        id,
        path: PathBuf::from(format!("{id}.mp3")),
        title: format!("Track {id}"),
        artist: Some(album_artist.to_owned()),
        album_id,
        album: album.to_owned(),
        album_key: normalize::key(album),
        album_artist: album_artist.to_owned(),
        album_artist_key: normalize::key(album_artist),
        album_year: year,
        track_no: Some(id as u32),
        duration_ms: 1000,
        codec: "mp3".to_owned(),
        bitrate_kbps: Some(320),
        ..Default::default()
    }
}

fn with_artist(mut t: ListTrack, artist: Option<&str>) -> ListTrack {
    t.artist = artist.map(str::to_owned);
    t
}

fn with_genre(mut t: ListTrack, genre: Option<&str>) -> ListTrack {
    t.genre = genre.map(str::to_owned);
    t
}

fn with_codec(mut t: ListTrack, codec: &str) -> ListTrack {
    t.codec = codec.to_owned();
    t
}

fn library() -> Catalog {
    Catalog::new(vec![
        track(1, "Portishead", 10, "Dummy", Some(1994)),
        track(2, "Portishead", 10, "Dummy", Some(1994)),
        track(3, "Radiohead", 20, "OK Computer", Some(1997)),
        track(4, "Radiohead", 20, "OK Computer", Some(1997)),
        with_artist(
            track(5, "Radiohead", 21, "Kid A", Some(2000)),
            Some("radiohead"),
        ),
        track(6, "Various", 30, "Mix", None),
        with_artist(track(7, "Various", 30, "Mix", None), Some("Portishead")),
        with_artist(track(8, "Various", 30, "Mix", None), None),
    ])
}

fn ids(catalog: &Catalog, view: &View) -> Vec<i64> {
    view.tracks
        .iter()
        .map(|&i| catalog.track(i).unwrap().id)
        .collect()
}

fn ids_filter<const N: usize>(ids: [i64; N]) -> TrackFilter {
    TrackFilter::Ids(Arc::new(HashSet::from(ids)))
}

fn labels(filters: &[Filter]) -> Vec<(&str, usize)> {
    filters
        .iter()
        .map(|f| (f.label.as_str(), f.tracks))
        .collect()
}

#[test]
fn an_empty_catalog_has_only_the_all_filter_and_empty_views() {
    let catalog = Catalog::new(Vec::new());
    for grouping in Grouping::ALL {
        let filters = catalog.filters(grouping, &TrackFilter::All);
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].value, FilterValue::All);
        assert_eq!(filters[0].tracks, 0);
        assert!(catalog.view(grouping, &TrackFilter::All).is_empty());
    }
}

#[test]
fn album_artist_filters_list_each_album_artist_once_in_key_order() {
    let catalog = library();
    let filters = catalog.filters(Grouping::AlbumArtist, &TrackFilter::All);
    assert_eq!(
        labels(&filters),
        [("", 8), ("Portishead", 2), ("Radiohead", 3), ("Various", 3)]
    );
    assert_eq!(filters[2].value, FilterValue::Text("radiohead".to_owned()));
}

#[test]
fn selecting_an_album_artist_lists_all_their_albums_grouped_in_album_order() {
    let catalog = library();
    let view = catalog.view(
        Grouping::AlbumArtist,
        &TrackFilter::Field(
            Field::AlbumArtist,
            FilterValue::Text("radiohead".to_owned()),
        ),
    );
    assert_eq!(ids(&catalog, &view), [3, 4, 5]);
    assert_eq!(view.albums, [0..2, 2..3]);
}

#[test]
fn artist_filters_use_the_track_artist_and_merge_case_variants() {
    let catalog = library();
    let filters = catalog.filters(Grouping::Artist, &TrackFilter::All);
    // Track 8 has no artist tag: it is listed under an empty (unknown) label.
    assert_eq!(
        labels(&filters),
        [
            ("", 8),
            ("", 1),
            ("Portishead", 3),
            ("Radiohead", 3),
            ("Various", 1)
        ]
    );
    let view = catalog.view(
        Grouping::Artist,
        &TrackFilter::Field(Field::Artist, FilterValue::Text("portishead".to_owned())),
    );
    // A track on someone else's album is grouped under that album.
    assert_eq!(ids(&catalog, &view), [1, 2, 7]);
    assert_eq!(view.albums, [0..2, 2..3]);
}

#[test]
fn genre_filters_use_the_track_genre_and_merge_case_variants() {
    let catalog = Catalog::new(vec![
        with_genre(track(1, "Portishead", 10, "Dummy", None), Some("Trip Hop")),
        with_genre(track(2, "Portishead", 10, "Dummy", None), Some("Rock")),
        with_genre(track(3, "Radiohead", 20, "Kid A", None), Some("rock")),
        with_genre(track(4, "Various", 30, "Mix", None), None),
        with_genre(track(5, "Various", 30, "Mix", None), Some("Trip Hop")),
    ]);
    let filters = catalog.filters(Grouping::Genre, &TrackFilter::All);
    // Track 4 has no genre tag: it is listed under an empty (unknown) label.
    assert_eq!(
        labels(&filters),
        [("", 5), ("", 1), ("Rock", 2), ("Trip Hop", 2)]
    );
    assert_eq!(filters[1].value, FilterValue::Text(String::new()));

    let view = catalog.view(
        Grouping::Genre,
        &TrackFilter::Field(Field::Genre, FilterValue::Text("trip hop".to_owned())),
    );
    // Only the matching tracks of each album are selected.
    assert_eq!(ids(&catalog, &view), [1, 5]);
    assert_eq!(view.albums, [0..1, 1..2]);
}

#[test]
fn file_type_filters_list_each_codec_as_the_file_type_column_shows_it() {
    let catalog = Catalog::new(vec![
        with_codec(track(1, "A", 10, "One", None), "flac"),
        track(2, "A", 10, "One", None),
        with_codec(track(3, "B", 20, "Two", None), "flac"),
    ]);
    let filters = catalog.filters(Grouping::FileType, &TrackFilter::All);
    assert_eq!(labels(&filters), [("", 3), ("FLAC", 2), ("MP3", 1)]);
    assert_eq!(file_type("flac"), "FLAC");

    let view = catalog.view(
        Grouping::FileType,
        &TrackFilter::Field(Field::FileType, FilterValue::Text("flac".to_owned())),
    );
    assert_eq!(ids(&catalog, &view), [1, 3]);
}

#[test]
fn album_filters_are_in_album_artist_then_year_order_with_the_album_artist_as_detail() {
    let catalog = library();
    let filters = catalog.filters(Grouping::Album, &TrackFilter::All);
    let albums: Vec<(&str, Option<&str>)> = filters[1..]
        .iter()
        .map(|f| (f.label.as_str(), f.detail.as_deref()))
        .collect();
    assert_eq!(
        albums,
        [
            ("Dummy", Some("Portishead")),
            ("OK Computer", Some("Radiohead")),
            ("Kid A", Some("Radiohead")),
            ("Mix", Some("Various")),
        ]
    );
    let view = catalog.view(
        Grouping::Album,
        &TrackFilter::Field(Field::Album, FilterValue::Album(21)),
    );
    assert_eq!(ids(&catalog, &view), [5]);
}

#[test]
fn year_filters_ascend_with_unknown_last() {
    let catalog = library();
    let values: Vec<FilterValue> = catalog
        .filters(Grouping::Year, &TrackFilter::All)
        .into_iter()
        .map(|f| f.value)
        .collect();
    assert_eq!(
        values,
        [
            FilterValue::All,
            FilterValue::Year(1994),
            FilterValue::Year(1997),
            FilterValue::Year(2000),
            FilterValue::UnknownYear,
        ]
    );
}

#[test]
fn the_year_view_orders_albums_by_max_year_and_keeps_album_track_order() {
    let catalog = Catalog::new(vec![
        track(1, "A", 10, "Late", Some(2010)),
        track(2, "A", 10, "Late", Some(2010)),
        track(3, "B", 20, "Early", Some(1990)),
        track(4, "C", 30, "Undated", None),
        track(5, "D", 40, "Same Year", Some(1990)),
    ]);
    let view = catalog.view(Grouping::Year, &TrackFilter::All);
    // 1990 (B, then D by album artist), 2010, then Unknown.
    assert_eq!(ids(&catalog, &view), [3, 5, 1, 2, 4]);
    assert_eq!(view.albums, [0..1, 1..2, 2..4, 4..5]);

    let view = catalog.view(
        Grouping::Year,
        &TrackFilter::Field(Field::Year, FilterValue::Year(1990)),
    );
    assert_eq!(ids(&catalog, &view), [3, 5]);
    let view = catalog.view(
        Grouping::Year,
        &TrackFilter::Field(Field::Year, FilterValue::UnknownYear),
    );
    assert_eq!(ids(&catalog, &view), [4]);
}

#[test]
fn newest_first_puts_each_album_artists_newest_album_first_and_unknown_last() {
    let catalog = Catalog::new(vec![
        track(1, "A", 10, "Alpha", Some(1990)),
        track(2, "A", 11, "Beta", Some(2000)),
        track(3, "A", 12, "Gamma", Some(2000)),
        track(4, "A", 13, "Undated", None),
        track(5, "B", 20, "Old", Some(1980)),
        track(6, "B", 20, "Old", Some(1980)),
        track(7, "B", 21, "New", Some(2020)),
    ]);
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    let view = catalog.newest_first(view, Grouping::AlbumArtist);
    // Artists stay A to Z, and same-year albums keep title order.
    assert_eq!(ids(&catalog, &view), [2, 3, 1, 4, 7, 5, 6]);
    assert_eq!(view.albums, [0..1, 1..2, 2..3, 3..4, 4..5, 5..7]);
}

#[test]
fn newest_first_puts_the_newest_year_first_in_the_year_view() {
    let catalog = Catalog::new(vec![
        track(1, "A", 10, "Late", Some(2010)),
        track(2, "A", 10, "Late", Some(2010)),
        track(3, "B", 20, "Early", Some(1990)),
        track(4, "C", 30, "Undated", None),
        track(5, "D", 40, "Same Year", Some(1990)),
    ]);
    let view = catalog.view(Grouping::Year, &TrackFilter::All);
    let view = catalog.newest_first(view, Grouping::Year);
    // 2010, 1990 (B, then D by album artist), then Unknown.
    assert_eq!(ids(&catalog, &view), [1, 2, 3, 5, 4]);
    assert_eq!(view.albums, [0..2, 2..3, 3..4, 4..5]);
}

#[test]
fn album_filters_newest_first_follow_the_newest_first_view() {
    let catalog = Catalog::new(vec![
        track(1, "A", 10, "Alpha", Some(1990)),
        track(2, "A", 11, "Beta", Some(2000)),
        track(3, "A", 12, "Undated", None),
        track(4, "B", 20, "Old", Some(1980)),
        track(5, "B", 21, "New", Some(2020)),
    ]);
    let labels: Vec<String> = catalog
        .filters_newest_first(Grouping::Album, &TrackFilter::All)
        .into_iter()
        .skip(1)
        .map(|f| f.label)
        .collect();
    assert_eq!(labels, ["Beta", "Alpha", "Undated", "New", "Old"]);
}

#[test]
fn only_groupings_ordered_by_date_have_no_album_direction() {
    for grouping in Grouping::ALL {
        let by_date = matches!(grouping, Grouping::RecentlyAdded | Grouping::RecentlyPlayed);
        assert_eq!(grouping.has_album_direction(), !by_date, "{grouping:?}");
    }
}

fn added(mut t: ListTrack, at: i64) -> ListTrack {
    t.added_ns = Some(at);
    t
}

fn played(mut t: ListTrack, at: i64) -> ListTrack {
    t.played_ns = Some(at);
    t
}

fn album_values(filters: &[Filter]) -> Vec<FilterValue> {
    filters.iter().skip(1).map(|f| f.value.clone()).collect()
}

/// Two singles came out ahead of their album: the album's other tracks
/// arrive on release day and bring the whole album back to the top.
fn release_day() -> Catalog {
    Catalog::new(vec![
        added(track(1, "A", 10, "Album", Some(2026)), 300),
        added(track(2, "A", 10, "Album", Some(2026)), 100),
        added(track(3, "A", 10, "Album", Some(2026)), 300),
        added(track(4, "A", 10, "Album", Some(2026)), 150),
        added(track(5, "B", 20, "Older", Some(2020)), 200),
        track(6, "C", 30, "Undated", None),
    ])
}

#[test]
fn recently_added_puts_the_album_with_the_newest_track_first_and_skips_undated_ones() {
    let catalog = release_day();
    let filters = catalog.filters(Grouping::RecentlyAdded, &TrackFilter::All);
    assert_eq!(
        album_values(&filters),
        [FilterValue::Album(10), FilterValue::Album(20)]
    );
    assert_eq!(filters[0].tracks, 5);
    assert_eq!(labels(&filters[1..]), [("Album", 4), ("Older", 1)]);
    assert_eq!(filters[1].detail.as_deref(), Some("A"));

    let view = catalog.view(Grouping::RecentlyAdded, &TrackFilter::All);
    // The whole album in album track order, not in the order it arrived.
    assert_eq!(ids(&catalog, &view), [1, 2, 3, 4, 5]);
    assert_eq!(view.albums, [0..4, 4..5]);
}

#[test]
fn before_release_day_the_singles_date_their_album() {
    let catalog = Catalog::new(vec![
        added(track(1, "A", 10, "Album", None), 100),
        track(2, "A", 10, "Album", None),
        added(track(3, "B", 20, "Older", None), 200),
    ]);
    let filters = catalog.filters(Grouping::RecentlyAdded, &TrackFilter::All);
    assert_eq!(
        album_values(&filters),
        [FilterValue::Album(20), FilterValue::Album(10)]
    );
}

#[test]
fn albums_added_at_the_same_moment_stay_whole_in_catalog_order() {
    let catalog = Catalog::new(vec![
        added(track(1, "A", 10, "One", None), 100),
        added(track(2, "A", 10, "One", None), 100),
        added(track(3, "B", 20, "Two", None), 100),
        added(track(4, "B", 20, "Two", None), 100),
    ]);
    let view = catalog.view(Grouping::RecentlyAdded, &TrackFilter::All);
    assert_eq!(ids(&catalog, &view), [1, 2, 3, 4]);
    assert_eq!(view.albums, [0..2, 2..4]);
}

#[test]
fn selecting_a_recently_added_album_lists_only_its_tracks() {
    let catalog = release_day();
    let view = catalog.view(
        Grouping::RecentlyAdded,
        &TrackFilter::Field(Field::Album, FilterValue::Album(20)),
    );
    assert_eq!(ids(&catalog, &view), [5]);
}

#[test]
fn recently_played_lists_only_played_albums_by_their_latest_play() {
    let catalog = Catalog::new(vec![
        played(track(1, "A", 10, "One", None), 100),
        track(2, "A", 10, "One", None),
        track(3, "B", 20, "Two", None),
        played(track(4, "C", 30, "Three", None), 50),
        played(track(5, "C", 30, "Three", None), 400),
    ]);
    let view = catalog.view(Grouping::RecentlyPlayed, &TrackFilter::All);
    assert_eq!(ids(&catalog, &view), [4, 5, 1, 2]);
    assert_eq!(
        album_values(&catalog.filters(Grouping::RecentlyPlayed, &TrackFilter::All)),
        [FilterValue::Album(30), FilterValue::Album(10)]
    );
}

#[test]
fn a_recorded_play_moves_its_album_to_the_top_of_recently_played() {
    let mut catalog = Catalog::new(vec![
        played(track(1, "A", 10, "One", None), 100),
        track(2, "B", 20, "Two", None),
    ]);
    let at = SystemTime::now();
    catalog.record_play(2, at);
    assert_eq!(catalog.by_id(2).unwrap().played_ns, Some(unix_ns(at)));
    let view = catalog.view(Grouping::RecentlyPlayed, &TrackFilter::All);
    assert_eq!(ids(&catalog, &view), [2, 1]);
    // Recently Added is unaffected.
    assert!(
        catalog
            .view(Grouping::RecentlyAdded, &TrackFilter::All)
            .is_empty()
    );
}

#[test]
fn a_play_of_an_unknown_track_changes_nothing() {
    let mut catalog = Catalog::new(vec![played(track(1, "A", 10, "One", None), 100)]);
    catalog.record_play(99, SystemTime::now());
    let view = catalog.view(Grouping::RecentlyPlayed, &TrackFilter::All);
    assert_eq!(ids(&catalog, &view), [1]);
}

#[test]
fn search_matches_narrow_the_recency_groupings() {
    let catalog = release_day();
    let matches = ids_filter([2, 5]);
    let filters = catalog.filters(Grouping::RecentlyAdded, &matches);
    assert_eq!(labels(&filters[1..]), [("Album", 1), ("Older", 1)]);
    let view = catalog.view(Grouping::RecentlyAdded, &matches);
    assert_eq!(ids(&catalog, &view), [2, 5]);
}

#[test]
fn search_matches_narrow_both_the_filters_and_the_view() {
    let catalog = library();
    let matches = ids_filter([3, 5, 7]);
    let filters = catalog.filters(Grouping::AlbumArtist, &matches);
    assert_eq!(
        labels(&filters),
        [("", 3), ("Radiohead", 2), ("Various", 1)]
    );

    let view = catalog.view(Grouping::AlbumArtist, &matches);
    assert_eq!(ids(&catalog, &view), [3, 5, 7]);
    assert_eq!(view.albums, [0..1, 1..2, 2..3]);

    let none = ids_filter([]);
    assert_eq!(catalog.filters(Grouping::Artist, &none).len(), 1);
    assert!(catalog.view(Grouping::Artist, &none).is_empty());
}

#[test]
fn a_value_that_no_longer_exists_selects_nothing() {
    let catalog = library();
    let view = catalog.view(
        Grouping::AlbumArtist,
        &TrackFilter::Field(Field::AlbumArtist, FilterValue::Text("gone".to_owned())),
    );
    assert!(view.is_empty());
    assert!(view.albums.is_empty());
}

#[test]
fn tracks_are_found_by_id() {
    let catalog = library();
    assert_eq!(catalog.by_id(5).unwrap().album, "Kid A");
    assert!(catalog.by_id(99).is_none());
    assert_eq!(catalog.album_of(99), None);
}

#[test]
fn an_album_view_has_no_sort() {
    let catalog = library();
    let view = catalog.view(Grouping::AlbumArtist, &TrackFilter::All);
    assert_eq!(view.sort, None);
}

#[test]
fn an_album_is_multi_disc_when_any_track_is_past_the_first_disc() {
    let catalog = Catalog::new(vec![
        ListTrack {
            disc_no: Some(1),
            ..track(1, "A", 10, "One", None)
        },
        ListTrack {
            disc_no: Some(2),
            ..track(2, "A", 10, "One", None)
        },
        ListTrack {
            disc_no: Some(1),
            ..track(3, "A", 20, "Two", None)
        },
        track(4, "A", 30, "Three", None),
    ]);
    assert!(catalog.is_multi_disc(10));
    assert!(!catalog.is_multi_disc(20));
    assert!(!catalog.is_multi_disc(30));
    assert!(!catalog.is_multi_disc(99));
}

#[test]
fn a_columns_name_is_the_one_it_is_saved_under() {
    for column in SortColumn::ALL {
        assert_eq!(
            toml::Value::try_from(column).unwrap().as_str(),
            Some(column.name())
        );
        assert_eq!(SortColumn::from_name(column.name()), Some(column));
    }
}

/// No wildcard arm, so a new column doesn't compile until it's counted here.
fn column_count() -> usize {
    match SortColumn::TrackNumber {
        SortColumn::TrackNumber
        | SortColumn::Title
        | SortColumn::Artist
        | SortColumn::Duration
        | SortColumn::FileType
        | SortColumn::BitRate => 6,
    }
}

#[test]
fn all_has_every_column_once_in_declaration_order() {
    assert_eq!(SortColumn::ALL.len(), column_count());
    for (i, column) in SortColumn::ALL.into_iter().enumerate() {
        assert_eq!(column as usize, i, "{column:?}");
    }
}

#[test]
fn an_unknown_column_name_is_no_column() {
    assert_eq!(SortColumn::from_name(""), None);
    assert_eq!(SortColumn::from_name("BitRate"), None);
    assert_eq!(SortColumn::from_name("bit_rate "), None);
}

/// No wildcard arm, so a new grouping doesn't compile until it's counted here.
fn grouping_count() -> usize {
    match Grouping::AlbumArtist {
        Grouping::AlbumArtist
        | Grouping::Artist
        | Grouping::Album
        | Grouping::Year
        | Grouping::Genre
        | Grouping::FileType
        | Grouping::RecentlyAdded
        | Grouping::RecentlyPlayed => 8,
    }
}

#[test]
fn all_has_every_grouping_once_in_declaration_order() {
    assert_eq!(Grouping::ALL.len(), grouping_count());
    for (i, grouping) in Grouping::ALL.into_iter().enumerate() {
        assert_eq!(grouping as usize, i, "{grouping:?}");
    }
}

/// No wildcard arm, so a new field doesn't compile until it's counted here.
fn field_count() -> usize {
    match Field::AlbumArtist {
        Field::AlbumArtist
        | Field::Artist
        | Field::Album
        | Field::Year
        | Field::Genre
        | Field::FileType => 6,
    }
}

#[test]
fn all_has_every_field_once_in_declaration_order() {
    assert_eq!(Field::ALL.len(), field_count());
    for (i, field) in Field::ALL.into_iter().enumerate() {
        assert_eq!(field as usize, i, "{field:?}");
    }
}

/// No wildcard arm, so a new order doesn't compile until it's counted here.
fn order_count() -> usize {
    match Order::Catalog {
        Order::Catalog | Order::Year | Order::Added | Order::Played => 4,
    }
}

#[test]
fn all_has_every_order_once_in_declaration_order() {
    assert_eq!(Order::ALL.len(), order_count());
    for (i, order) in Order::ALL.into_iter().enumerate() {
        assert_eq!(order as usize, i);
    }
}

#[test]
fn a_groupings_name_is_the_one_it_is_saved_under() {
    for grouping in Grouping::ALL {
        assert_eq!(
            toml::Value::try_from(grouping).unwrap().as_str(),
            Some(grouping.name())
        );
        assert_eq!(Grouping::from_name(grouping.name()), Some(grouping));
    }
}

#[test]
fn an_unknown_grouping_name_is_no_grouping() {
    assert_eq!(Grouping::from_name(""), None);
    assert_eq!(Grouping::from_name("AlbumArtist"), None);
    assert_eq!(Grouping::from_name("album"), Some(Grouping::Album));
    assert_eq!(Grouping::from_name("albums"), None);
}

/// What `column` sorts `track` by; `None` sorts last in both directions.
fn sort_key(track: &ListTrack, column: SortColumn) -> Option<(u64, u64, String)> {
    let number = |n: u64| Some((n, 0, String::new()));
    let text = |s: &str| Some((0, 0, normalize::key(s)));
    match column {
        SortColumn::TrackNumber => track
            .track_no
            .map(|no| (track.disc_no.unwrap_or(1).into(), no.into(), String::new())),
        SortColumn::Title => text(&track.title),
        SortColumn::Artist => {
            text(track.artist.as_deref().unwrap_or_default()).filter(|(_, _, key)| !key.is_empty())
        }
        // lofty reports 0 when it can't read the length.
        SortColumn::Duration => number(track.duration_ms).filter(|_| track.duration_ms > 0),
        SortColumn::FileType => text(&track.codec),
        SortColumn::BitRate => track.bitrate_kbps.and_then(|kbps| number(kbps.into())),
    }
}

/// One of the values the filter pane for `grouping` lists under `filter`.
fn picked_value(
    catalog: &Catalog,
    grouping: Grouping,
    pick: Index,
    filter: &TrackFilter,
) -> TrackFilter {
    let filters = catalog.filters(grouping, filter);
    let value = filters[pick.index(filters.len())].value.clone();
    TrackFilter::Field(grouping.field(), value)
}

/// A value of `field`'s kind that no track from [`arb_tracks`] has.
fn gone_value(field: Field) -> FilterValue {
    match field.def().key {
        KeyKind::Text(_) | KeyKind::Stored(_) => FilterValue::Text("gone".to_owned()),
        KeyKind::Album => FilterValue::Album(-1),
        KeyKind::AlbumYear => FilterValue::Year(1),
    }
}

fn picked_view(catalog: &Catalog, grouping: Grouping, pick: Index, search: &TrackFilter) -> View {
    let value = picked_value(catalog, grouping, pick, search);
    catalog.view(grouping, &search.clone().and(value))
}

#[test]
fn clicking_a_heading_sorts_ascending_then_descending_then_restores_album_order() {
    let states = SortColumn::ALL
        .into_iter()
        .flat_map(|column| [false, true].map(|descending| Some(Sort { column, descending })));
    for current in states.chain([None]) {
        for column in SortColumn::ALL {
            let ascending = Some(Sort {
                column,
                descending: false,
            });
            let descending = Some(Sort {
                column,
                descending: true,
            });
            let next = Sort::after_click(current, column);
            let expected = match current {
                Some(sort) if sort.column != column => ascending,
                _ if current == ascending => descending,
                _ if current == descending => None,
                _ => ascending,
            };
            assert_eq!(next, expected, "{current:?} then {column:?}");
        }
    }
}

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
    let title = after_heading_click(None, "title").unwrap();
    assert_eq!(title, sort(SortColumn::Title, false));
    let title = after_heading_click(title, "title").unwrap();
    assert_eq!(title, sort(SortColumn::Title, true));
    assert_eq!(after_heading_click(title, "title"), Some(None));
    assert_eq!(
        after_heading_click(title, "artist"),
        Some(sort(SortColumn::Artist, false))
    );

    for current in every_sort() {
        for name in every_name() {
            let expected = SortColumn::from_name(name).map(|c| Sort::after_click(current, c));
            assert_eq!(
                after_heading_click(current, name),
                expected,
                "{current:?} {name:?}"
            );
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

proptest! {
    #[test]
    fn the_filter_counts_add_up_to_all_and_each_is_the_size_of_its_view_of_whole_album_runs(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
        search in arb_search(),
        other in arb_grouping(),
        other_pick in any::<Index>(),
    ) {
        let catalog = Catalog::new(tracks);
        let other_value = picked_value(&catalog, other, other_pick, &TrackFilter::All);
        let shown = search.and(other_value);
        let filters = catalog.filters(grouping, &shown);
        let values: usize = filters[1..].iter().map(|f| f.tracks).sum();
        prop_assert_eq!(values, filters[0].tracks);
        for filter in filters {
            let value = TrackFilter::Field(grouping.field(), filter.value);
            let view = catalog.view(grouping, &shown.clone().and(value));
            prop_assert_eq!(view.len(), filter.tracks);
            prop_assert_eq!(view.sort, None);
            let mut end = 0;
            for album in &view.albums {
                prop_assert_eq!(album.start, end);
                end = album.end;
                let ids: HashSet<i64> = view.tracks[album.clone()]
                    .iter()
                    .map(|&i| catalog.track(i).unwrap().album_id)
                    .collect();
                prop_assert_eq!(ids.len(), 1);
            }
            prop_assert_eq!(end, view.len());
        }
    }

    #[test]
    fn and_selects_the_tracks_both_filters_select_in_view_order(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
        search in arb_search(),
        other in arb_grouping(),
        pick in any::<Index>(),
    ) {
        let catalog = Catalog::new(tracks);
        let value = picked_value(&catalog, other, pick, &TrackFilter::All);
        let both = catalog.view(grouping, &search.clone().and(value.clone()));
        let in_value: HashSet<usize> = catalog.view(grouping, &value).tracks.into_iter().collect();
        let mut expected = catalog.view(grouping, &search).tracks;
        expected.retain(|i| in_value.contains(i));
        prop_assert_eq!(both.tracks, expected);
    }

    #[test]
    fn filters_that_pass_every_track_list_and_select_what_all_does(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
        field in prop::sample::select(&Field::ALL[..]),
    ) {
        let catalog = Catalog::new(tracks);
        let every_id = (0..catalog.len()).map(|i| catalog.track(i).unwrap().id).collect();
        let filters = catalog.filters(grouping, &TrackFilter::All);
        let view = catalog.view(grouping, &TrackFilter::All);
        for filter in [
            TrackFilter::Ids(Arc::new(every_id)),
            TrackFilter::Field(field, FilterValue::All),
            TrackFilter::And(Vec::new()),
            TrackFilter::All.and(TrackFilter::All),
        ] {
            prop_assert_eq!(&catalog.filters(grouping, &filter), &filters);
            prop_assert_eq!(&catalog.view(grouping, &filter), &view);
        }
    }

    #[test]
    fn a_tracks_album_value_lists_its_album_and_only_it(
        tracks in arb_tracks(),
        pick in any::<Index>(),
    ) {
        prop_assume!(!tracks.is_empty());
        let catalog = Catalog::new(tracks);
        let track = catalog.track(pick.index(catalog.len())).unwrap();
        let value = catalog.album_of(track.id).unwrap();
        for filters in [
            catalog.filters(Grouping::Album, &TrackFilter::All),
            catalog.filters_newest_first(Grouping::Album, &TrackFilter::All),
        ] {
            prop_assert_eq!(filters.iter().filter(|f| f.value == value).count(), 1);
        }
        let view = catalog.view(Grouping::Album, &TrackFilter::Field(Field::Album, value));
        let ids: Vec<i64> = view.tracks.iter().map(|&i| catalog.track(i).unwrap().id).collect();
        prop_assert!(ids.contains(&track.id));
        prop_assert!(view.tracks.iter().all(|&i| catalog.track(i).unwrap().album_id == track.album_id));
    }

    #[test]
    fn sorting_orders_by_the_column_with_unknowns_last_and_ties_in_view_order(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
        pick in any::<Index>(),
        search in arb_search(),
        column in arb_sort_column(),
        descending: bool,
    ) {
        let catalog = Catalog::new(tracks);
        let view = picked_view(&catalog, grouping, pick, &search);
        let key = |i: usize| sort_key(catalog.track(i).unwrap(), column);
        let mut expected = view.tracks.clone();
        expected.sort_by(|&a, &b| {
            let (a, b) = (key(a), key(b));
            a.is_none().cmp(&b.is_none()).then(if descending { b.cmp(&a) } else { a.cmp(&b) })
        });
        let sort = Sort { column, descending };
        let sorted = catalog.sorted(view, sort);
        prop_assert_eq!(&sorted.tracks, &expected);
        prop_assert_eq!(sorted.albums, (0..expected.len()).map(|i| i..i + 1).collect::<Vec<_>>());
        prop_assert_eq!(sorted.sort, Some(sort));
    }

    #[test]
    fn newest_first_reorders_whole_albums_by_year_descending_with_unknown_last(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
        pick in any::<Index>(),
        search in arb_search(),
    ) {
        let catalog = Catalog::new(tracks);
        let view = picked_view(&catalog, grouping, pick, &search);
        let newest = catalog.newest_first(view.clone(), grouping);
        if matches!(grouping, Grouping::RecentlyAdded | Grouping::RecentlyPlayed) {
            prop_assert_eq!(newest, view);
            return Ok(());
        }
        let by_year = grouping == Grouping::Year;
        let mut runs: Vec<&[usize]> = view.albums.iter().map(|a| &view.tracks[a.clone()]).collect();
        runs.sort_by_key(|run| {
            let first = catalog.track(run[0]).unwrap();
            let artist = if by_year { "" } else { first.album_artist_key.as_str() };
            (artist, first.album_year.is_none(), std::cmp::Reverse(first.album_year))
        });
        prop_assert_eq!(&newest.tracks, &runs.concat());
        let mut start = 0;
        let albums: Vec<Range<usize>> = runs
            .iter()
            .map(|run| {
                start += run.len();
                start - run.len()..start
            })
            .collect();
        prop_assert_eq!(newest.albums, albums);
        prop_assert_eq!(newest.sort, None);
    }

    #[test]
    fn filters_newest_first_list_the_same_values_in_the_newest_first_order(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
        search in arb_search(),
    ) {
        let catalog = Catalog::new(tracks);
        let filters = catalog.filters(grouping, &search);
        let newest = catalog.filters_newest_first(grouping, &search);
        prop_assert_eq!(&newest[0], &filters[0]);
        let mut same = newest.clone();
        same.sort_by(|a, b| a.value.cmp(&b.value));
        let mut sorted = filters.clone();
        sorted.sort_by(|a, b| a.value.cmp(&b.value));
        prop_assert_eq!(same, sorted);
        match grouping {
            Grouping::Year => {
                let mut years: Vec<FilterValue> = filters[1..]
                    .iter()
                    .map(|f| f.value.clone())
                    .filter(|v| matches!(v, FilterValue::Year(_)))
                    .collect();
                years.reverse();
                years.extend(filters.iter().map(|f| f.value.clone()).filter(|v| *v == FilterValue::UnknownYear));
                let values: Vec<FilterValue> = newest[1..].iter().map(|f| f.value.clone()).collect();
                prop_assert_eq!(values, years);
            }
            Grouping::Album | Grouping::RecentlyAdded | Grouping::RecentlyPlayed => {
                let view = catalog.view(grouping, &search);
                let mut albums: Vec<FilterValue> = Vec::new();
                for &i in &catalog.newest_first(view, grouping).tracks {
                    let album = FilterValue::Album(catalog.track(i).unwrap().album_id);
                    if !albums.contains(&album) {
                        albums.push(album);
                    }
                }
                let values: Vec<FilterValue> = newest[1..].iter().map(|f| f.value.clone()).collect();
                prop_assert_eq!(values, albums);
            }
            _ => prop_assert_eq!(newest, filters),
        }
    }

    #[test]
    fn an_order_lists_tracks_at_most_once_with_whole_albums_in_catalog_order(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
    ) {
        let catalog = Catalog::new(tracks);
        let order: Vec<usize> = catalog.order(grouping).collect();
        let mut seen = HashSet::new();
        let mut done_albums = HashSet::new();
        for (at, &i) in order.iter().enumerate() {
            prop_assert!(i < catalog.len());
            prop_assert!(seen.insert(i));
            let album = catalog.track(i).unwrap().album_id;
            match at.checked_sub(1).map(|prev| order[prev]) {
                Some(prev) if catalog.track(prev).unwrap().album_id == album => {
                    prop_assert!(prev < i);
                }
                Some(prev) => {
                    done_albums.insert(catalog.track(prev).unwrap().album_id);
                    prop_assert!(!done_albums.contains(&album));
                }
                None => {}
            }
        }
    }

    #[test]
    fn a_recorded_play_tops_recently_played_and_leaves_the_other_orders_alone(
        tracks in arb_tracks(),
        pick in any::<Index>(),
    ) {
        prop_assume!(!tracks.is_empty());
        let mut catalog = Catalog::new(tracks);
        let before = catalog.clone();
        let played = catalog.track(pick.index(catalog.len())).unwrap().clone();
        catalog.record_play(played.id, SystemTime::now());
        let top = catalog.order(Grouping::RecentlyPlayed).next();
        prop_assert_eq!(top.map(|i| catalog.track(i).unwrap().album_id), Some(played.album_id));
        for grouping in Grouping::ALL {
            if grouping != Grouping::RecentlyPlayed {
                prop_assert!(catalog.order(grouping).eq(before.order(grouping)), "{:?}", grouping);
            }
        }
    }

    #[test]
    fn browsing_selects_the_saved_value_or_all_and_shows_its_tracks_in_the_states_order(
        tracks in arb_tracks(),
        grouping in arb_grouping(),
        pick in any::<Index>(),
        gone in any::<bool>(),
        sort in prop::option::of(arb_sort()),
        album_descending in any::<bool>(),
        search in arb_search(),
    ) {
        let catalog = Catalog::new(tracks);
        let selected = if gone {
            gone_value(grouping.field())
        } else {
            let filters = catalog.filters(grouping, &TrackFilter::All);
            filters[pick.index(filters.len())].value.clone()
        };
        let state = BrowseState {
            source: Source::Library(grouping),
            selected,
            sort,
            album_descending,
        };
        let browsed = catalog.browse(&state, &[], &search);
        let filters = if album_descending {
            catalog.filters_newest_first(grouping, &search)
        } else {
            catalog.filters(grouping, &search)
        };
        prop_assert_eq!(&browsed.filters, &filters);
        let listed = filters.iter().position(|f| f.value == state.selected);
        prop_assert!(!gone || listed.is_none());
        prop_assert_eq!(browsed.selected_row, listed.unwrap_or(0));

        let shown = &filters[browsed.selected_row];
        let passing = search.clone().and(TrackFilter::Field(grouping.field(), shown.value.clone()));
        let view = &browsed.view;
        prop_assert_eq!(view.len(), shown.tracks);
        prop_assert_eq!(view.tracks.iter().collect::<HashSet<_>>().len(), view.len());
        prop_assert!(view.tracks.iter().all(|&i| passing.passes(&catalog, i)));
        prop_assert_eq!(view.sort, sort);
        match sort {
            Some(sort) => prop_assert_eq!(&catalog.sorted(view.clone(), sort), view),
            None if album_descending => {
                prop_assert_eq!(&catalog.newest_first(view.clone(), grouping), view);
            }
            None => {
                let position: HashMap<usize, usize> =
                    catalog.order(grouping).enumerate().map(|(at, i)| (i, at)).collect();
                prop_assert!(view.tracks.is_sorted_by_key(|i| position[i]));
            }
        }

        let corrected = BrowseState { selected: shown.value.clone(), ..state.clone() };
        prop_assert_eq!(&catalog.browsed_view(&corrected, &[], &search), view);
        if listed.is_none() {
            prop_assert_eq!(&catalog.browse(&corrected, &[], &search), &browsed);
        }
    }

    #[test]
    fn a_playlist_shows_its_tracks_in_catalog_that_pass_the_search_in_order_with_repeats(
        tracks in arb_tracks(),
        track_ids in prop::collection::vec(-2..22i64, 0..30),
        id in arb_list_id(),
        listed in any::<bool>(),
        shown in any::<bool>(),
        selected in arb_filter_value(),
        sort in prop::option::of(arb_sort()),
        album_descending in any::<bool>(),
        search in arb_search(),
    ) {
        let catalog = Catalog::new(tracks);
        let playlist = Playlist { id, name: "Mix".to_owned(), track_ids: track_ids.clone() };
        let playlists = if listed { vec![playlist] } else { Vec::new() };
        let state = BrowseState { source: Source::Playlist { id: shown.then_some(id) }, selected, sort, album_descending };
        let browsed = catalog.browse(&state, &playlists, &search);

        let expected: Vec<i64> = if listed && shown { track_ids.clone() } else { Vec::new() }
            .into_iter()
            .filter(|id| catalog.by_id.get(id).is_some_and(|&i| search.passes(&catalog, i)))
            .collect();
        let ids = |view: &View| -> Vec<i64> {
            view.tracks.iter().map(|&i| catalog.track(i).unwrap().id).collect()
        };
        prop_assert_eq!(browsed.filters.len(), 1);
        prop_assert_eq!(&browsed.filters[0].value, &FilterValue::All);
        prop_assert_eq!(browsed.filters[0].tracks, expected.len());
        prop_assert_eq!(browsed.selected_row, 0);
        prop_assert_eq!(browsed.view.sort, sort);
        prop_assert_eq!(&catalog.browsed_view(&state, &playlists, &search), &browsed.view);
        match sort {
            Some(sort) => {
                let mut shown = ids(&browsed.view);
                let mut expected = expected.clone();
                shown.sort_unstable();
                expected.sort_unstable();
                prop_assert_eq!(shown, expected);
                let unsorted = BrowseState { sort: None, ..state.clone() };
                let unsorted = catalog.browsed_view(&unsorted, &playlists, &search);
                prop_assert_eq!(&catalog.sorted(unsorted, sort), &browsed.view);
            }
            None => prop_assert_eq!(ids(&browsed.view), expected),
        }
        prop_assert!(browsed.view.flat);
        let runs: Vec<Range<usize>> = (0..browsed.view.len()).map(|i| i..i + 1).collect();
        prop_assert_eq!(&browsed.view.albums, &runs);

        // Each row shows its own position of the playlist; unsorted, in order.
        let shown_ids = if listed && shown { &track_ids[..] } else { &[] };
        let positions: Vec<usize> = catalog
            .playlist_positions(&browsed.view, shown_ids)
            .into_iter()
            .map(Option::unwrap)
            .collect();
        prop_assert_eq!(
            positions.iter().map(|&p| shown_ids[p]).collect::<Vec<_>>(),
            ids(&browsed.view)
        );
        let mut distinct = positions.clone();
        distinct.sort_unstable();
        distinct.dedup();
        prop_assert_eq!(distinct.len(), positions.len());
        if sort.is_none() {
            prop_assert!(positions.is_sorted());
        }
    }
}
