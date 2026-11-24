use std::path::PathBuf;

use super::*;

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
        let filters = catalog.filters(grouping, None);
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].value, FilterValue::All);
        assert_eq!(filters[0].tracks, 0);
        assert!(catalog.view(grouping, &FilterValue::All, None).is_empty());
    }
}

#[test]
fn album_artist_filters_list_each_album_artist_once_in_key_order() {
    let catalog = library();
    let filters = catalog.filters(Grouping::AlbumArtist, None);
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
        &FilterValue::Text("radiohead".to_owned()),
        None,
    );
    assert_eq!(ids(&catalog, &view), [3, 4, 5]);
    assert_eq!(view.albums, [0..2, 2..3]);
}

#[test]
fn artist_filters_use_the_track_artist_and_merge_case_variants() {
    let catalog = library();
    let filters = catalog.filters(Grouping::Artist, None);
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
        &FilterValue::Text("portishead".to_owned()),
        None,
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
    let filters = catalog.filters(Grouping::Genre, None);
    // Track 4 has no genre tag: it is listed under an empty (unknown) label.
    assert_eq!(
        labels(&filters),
        [("", 5), ("", 1), ("Rock", 2), ("Trip Hop", 2)]
    );
    assert_eq!(filters[1].value, FilterValue::Text(String::new()));

    let view = catalog.view(
        Grouping::Genre,
        &FilterValue::Text("trip hop".to_owned()),
        None,
    );
    // Only the matching tracks of each album are selected.
    assert_eq!(ids(&catalog, &view), [1, 5]);
    assert_eq!(view.albums, [0..1, 1..2]);
}

#[test]
fn album_filters_are_in_album_artist_then_year_order_with_the_album_artist_as_detail() {
    let catalog = library();
    let filters = catalog.filters(Grouping::Album, None);
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
    let view = catalog.view(Grouping::Album, &FilterValue::Album(21), None);
    assert_eq!(ids(&catalog, &view), [5]);
}

#[test]
fn year_filters_ascend_with_unknown_last() {
    let catalog = library();
    let values: Vec<FilterValue> = catalog
        .filters(Grouping::Year, None)
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
    let view = catalog.view(Grouping::Year, &FilterValue::All, None);
    // 1990 (B, then D by album artist), 2010, then Unknown.
    assert_eq!(ids(&catalog, &view), [3, 5, 1, 2, 4]);
    assert_eq!(view.albums, [0..1, 1..2, 2..4, 4..5]);

    let view = catalog.view(Grouping::Year, &FilterValue::Year(1990), None);
    assert_eq!(ids(&catalog, &view), [3, 5]);
    let view = catalog.view(Grouping::Year, &FilterValue::UnknownYear, None);
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
    for grouping in [Grouping::AlbumArtist, Grouping::Album, Grouping::Genre] {
        let view = catalog.view(grouping, &FilterValue::All, None);
        let view = catalog.newest_first(view, grouping);
        // Artists stay A to Z, and same-year albums keep title order.
        assert_eq!(ids(&catalog, &view), [2, 3, 1, 4, 7, 5, 6], "{grouping:?}");
        assert_eq!(view.albums, [0..1, 1..2, 2..3, 3..4, 4..5, 5..7]);
        assert_eq!(view.sort, None);
    }
    let b = FilterValue::Text(normalize::key("B"));
    let view = catalog.view(Grouping::AlbumArtist, &b, None);
    let view = catalog.newest_first(view, Grouping::AlbumArtist);
    assert_eq!(ids(&catalog, &view), [7, 5, 6]);
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
    let view = catalog.view(Grouping::Year, &FilterValue::All, None);
    let view = catalog.newest_first(view, Grouping::Year);
    // 2010, 1990 (B, then D by album artist), then Unknown.
    assert_eq!(ids(&catalog, &view), [1, 2, 3, 5, 4]);
    assert_eq!(view.albums, [0..2, 2..3, 3..4, 4..5]);
}

#[test]
fn newest_first_leaves_groupings_ordered_by_date_alone() {
    let catalog = Catalog::new(vec![
        added(track(1, "A", 10, "Old", Some(1990)), 2),
        added(track(2, "B", 20, "New", Some(2020)), 1),
    ]);
    let view = catalog.view(Grouping::RecentlyAdded, &FilterValue::All, None);
    assert_eq!(
        catalog.newest_first(view.clone(), Grouping::RecentlyAdded),
        view
    );
}

#[test]
fn newest_first_of_an_empty_view_is_empty() {
    let catalog = library();
    let view = catalog.newest_first(View::default(), Grouping::AlbumArtist);
    assert_eq!(view, View::default());
}

#[test]
fn year_filters_newest_first_keep_all_first_and_unknown_last() {
    let filters = library().filters_newest_first(Grouping::Year, None);
    let values: Vec<FilterValue> = filters.into_iter().map(|f| f.value).collect();
    assert_eq!(
        values,
        [
            FilterValue::All,
            FilterValue::Year(2000),
            FilterValue::Year(1997),
            FilterValue::Year(1994),
            FilterValue::UnknownYear,
        ]
    );

    let filters = Catalog::new(Vec::new()).filters_newest_first(Grouping::Year, None);
    assert_eq!(filters.len(), 1);
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
    let labels = |filters: Vec<Filter>| -> Vec<String> {
        filters.into_iter().skip(1).map(|f| f.label).collect()
    };
    assert_eq!(
        labels(catalog.filters_newest_first(Grouping::Album, None)),
        ["Beta", "Alpha", "Undated", "New", "Old"]
    );
    let matches = HashSet::from([1, 2, 4]);
    let filters = catalog.filters_newest_first(Grouping::Album, Some(&matches));
    assert_eq!(filters[0].tracks, 3);
    assert_eq!(labels(filters), ["Beta", "Alpha", "Old"]);
}

#[test]
fn other_filters_newest_first_are_unchanged() {
    let catalog = library();
    for grouping in [
        Grouping::AlbumArtist,
        Grouping::Artist,
        Grouping::Genre,
        Grouping::RecentlyAdded,
        Grouping::RecentlyPlayed,
    ] {
        assert_eq!(
            catalog.filters_newest_first(grouping, None),
            catalog.filters(grouping, None),
            "{grouping:?}"
        );
    }
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
    let filters = catalog.filters(Grouping::RecentlyAdded, None);
    assert_eq!(
        album_values(&filters),
        [FilterValue::Album(10), FilterValue::Album(20)]
    );
    assert_eq!(filters[0].tracks, 5);
    assert_eq!(labels(&filters[1..]), [("Album", 4), ("Older", 1)]);
    assert_eq!(filters[1].detail.as_deref(), Some("A"));

    let view = catalog.view(Grouping::RecentlyAdded, &FilterValue::All, None);
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
    let filters = catalog.filters(Grouping::RecentlyAdded, None);
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
    let view = catalog.view(Grouping::RecentlyAdded, &FilterValue::All, None);
    assert_eq!(ids(&catalog, &view), [1, 2, 3, 4]);
    assert_eq!(view.albums, [0..2, 2..4]);
}

#[test]
fn selecting_a_recently_added_album_lists_only_its_tracks() {
    let catalog = release_day();
    let view = catalog.view(Grouping::RecentlyAdded, &FilterValue::Album(20), None);
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
    let view = catalog.view(Grouping::RecentlyPlayed, &FilterValue::All, None);
    assert_eq!(ids(&catalog, &view), [4, 5, 1, 2]);
    assert_eq!(
        album_values(&catalog.filters(Grouping::RecentlyPlayed, None)),
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
    let view = catalog.view(Grouping::RecentlyPlayed, &FilterValue::All, None);
    assert_eq!(ids(&catalog, &view), [2, 1]);
    // Recently Added is unaffected.
    assert!(
        catalog
            .view(Grouping::RecentlyAdded, &FilterValue::All, None)
            .is_empty()
    );
}

#[test]
fn a_play_of_an_unknown_track_changes_nothing() {
    let mut catalog = Catalog::new(vec![played(track(1, "A", 10, "One", None), 100)]);
    catalog.record_play(99, SystemTime::now());
    let view = catalog.view(Grouping::RecentlyPlayed, &FilterValue::All, None);
    assert_eq!(ids(&catalog, &view), [1]);
}

#[test]
fn search_matches_narrow_the_recency_groupings() {
    let catalog = release_day();
    let matches = HashSet::from([2, 5]);
    let filters = catalog.filters(Grouping::RecentlyAdded, Some(&matches));
    assert_eq!(labels(&filters[1..]), [("Album", 1), ("Older", 1)]);
    let view = catalog.view(Grouping::RecentlyAdded, &FilterValue::All, Some(&matches));
    assert_eq!(ids(&catalog, &view), [2, 5]);
}

#[test]
fn search_matches_narrow_both_the_filters_and_the_view() {
    let catalog = library();
    let matches: HashSet<i64> = [3, 5, 7].into();
    let filters = catalog.filters(Grouping::AlbumArtist, Some(&matches));
    assert_eq!(
        labels(&filters),
        [("", 3), ("Radiohead", 2), ("Various", 1)]
    );

    let view = catalog.view(Grouping::AlbumArtist, &FilterValue::All, Some(&matches));
    assert_eq!(ids(&catalog, &view), [3, 5, 7]);
    assert_eq!(view.albums, [0..1, 1..2, 2..3]);

    let none = HashSet::new();
    assert_eq!(catalog.filters(Grouping::Artist, Some(&none)).len(), 1);
    assert!(
        catalog
            .view(Grouping::Artist, &FilterValue::All, Some(&none))
            .is_empty()
    );
}

#[test]
fn a_value_that_no_longer_exists_selects_nothing() {
    let catalog = library();
    let view = catalog.view(
        Grouping::AlbumArtist,
        &FilterValue::Text("gone".to_owned()),
        None,
    );
    assert!(view.is_empty());
    assert!(view.albums.is_empty());
}

#[test]
fn tracks_are_found_by_id() {
    let catalog = library();
    assert_eq!(catalog.by_id(5).unwrap().album, "Kid A");
    assert!(catalog.by_id(99).is_none());
}

fn sort(column: SortColumn, descending: bool) -> Sort {
    Sort { column, descending }
}

fn titled(mut t: ListTrack, title: &str) -> ListTrack {
    t.title = title.to_owned();
    t
}

#[test]
fn clicking_a_heading_sorts_ascending_then_descending_then_restores_album_order() {
    let title = Sort::after_click(None, SortColumn::Title);
    assert_eq!(title, Some(sort(SortColumn::Title, false)));
    let title = Sort::after_click(title, SortColumn::Title);
    assert_eq!(title, Some(sort(SortColumn::Title, true)));
    assert_eq!(Sort::after_click(title, SortColumn::Title), None);
}

#[test]
fn clicking_another_heading_sorts_by_it_ascending() {
    for current in [
        sort(SortColumn::Title, false),
        sort(SortColumn::Title, true),
    ] {
        assert_eq!(
            Sort::after_click(Some(current), SortColumn::Duration),
            Some(sort(SortColumn::Duration, false))
        );
    }
}

#[test]
fn sorting_by_title_ignores_case_and_gives_each_track_its_own_row() {
    let catalog = Catalog::new(vec![
        titled(track(1, "A", 10, "One", None), "banana"),
        titled(track(2, "A", 10, "One", None), "Cherry"),
        titled(track(3, "B", 20, "Two", None), "Apple"),
    ]);
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);

    let view = catalog.sorted(all.clone(), sort(SortColumn::Title, false));
    assert_eq!(ids(&catalog, &view), [3, 1, 2]);
    assert_eq!(view.albums, [0..1, 1..2, 2..3]);
    assert_eq!(view.sort, Some(sort(SortColumn::Title, false)));
    assert_eq!(view.sort, Some(sort(SortColumn::Title, false)));

    let view = catalog.sorted(all, sort(SortColumn::Title, true));
    assert_eq!(ids(&catalog, &view), [2, 1, 3]);
}

#[test]
fn equal_values_keep_album_order_in_both_directions() {
    let catalog = library();
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    for descending in [false, true] {
        let view = catalog.sorted(all.clone(), sort(SortColumn::Duration, descending));
        assert_eq!(ids(&catalog, &view), ids(&catalog, &all));
    }
}

#[test]
fn sorting_by_duration_orders_by_length() {
    let catalog = Catalog::new(
        [(1, 300_000), (2, 90_000), (3, 3_600_000)]
            .into_iter()
            .map(|(id, ms)| ListTrack {
                duration_ms: ms,
                ..track(id, "A", 10, "One", None)
            })
            .collect(),
    );
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let view = catalog.sorted(all.clone(), sort(SortColumn::Duration, false));
    assert_eq!(ids(&catalog, &view), [2, 1, 3]);
    let view = catalog.sorted(all, sort(SortColumn::Duration, true));
    assert_eq!(ids(&catalog, &view), [3, 1, 2]);
}

#[test]
fn tracks_without_a_number_sort_last_in_both_directions() {
    let catalog = Catalog::new(
        [(1, Some(2)), (2, None), (3, Some(1)), (4, Some(2))]
            .into_iter()
            .map(|(id, no)| ListTrack {
                track_no: no,
                ..track(id, "A", 10, "One", None)
            })
            .collect(),
    );
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let view = catalog.sorted(all.clone(), sort(SortColumn::TrackNumber, false));
    assert_eq!(ids(&catalog, &view), [3, 1, 4, 2]);
    let view = catalog.sorted(all, sort(SortColumn::TrackNumber, true));
    assert_eq!(ids(&catalog, &view), [1, 4, 3, 2]);
}

#[test]
fn track_numbers_sort_by_disc_then_track_with_no_disc_as_the_first() {
    let catalog = Catalog::new(
        [
            (1, Some(2), 1),
            (2, Some(1), 2),
            (3, None, 1),
            (4, Some(1), 1),
        ]
        .into_iter()
        .map(|(id, disc, no)| ListTrack {
            disc_no: disc,
            track_no: Some(no),
            ..track(id, "A", 10, "One", None)
        })
        .collect(),
    );
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let view = catalog.sorted(all.clone(), sort(SortColumn::TrackNumber, false));
    assert_eq!(ids(&catalog, &view), [3, 4, 2, 1]);
    let view = catalog.sorted(all, sort(SortColumn::TrackNumber, true));
    assert_eq!(ids(&catalog, &view), [1, 2, 3, 4]);
}

#[test]
fn sorting_by_artist_ignores_case_and_puts_tracks_without_one_last() {
    let catalog = Catalog::new(vec![
        with_artist(track(1, "A", 10, "One", None), Some("beck")),
        with_artist(track(2, "A", 10, "One", None), None),
        with_artist(track(3, "A", 10, "One", None), Some("Air")),
        with_artist(track(4, "A", 10, "One", None), Some("Cake")),
        with_artist(track(5, "A", 10, "One", None), Some("BECK")),
        with_artist(track(6, "A", 10, "One", None), Some("  ")),
    ]);
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let view = catalog.sorted(all.clone(), sort(SortColumn::Artist, false));
    assert_eq!(ids(&catalog, &view), [3, 1, 5, 4, 2, 6]);
    let view = catalog.sorted(all, sort(SortColumn::Artist, true));
    assert_eq!(ids(&catalog, &view), [4, 1, 5, 3, 2, 6]);
}

#[test]
fn tracks_of_unknown_length_sort_last_in_both_directions() {
    let catalog = Catalog::new(
        [(1, 300_000), (2, 0), (3, 90_000)]
            .into_iter()
            .map(|(id, ms)| ListTrack {
                duration_ms: ms,
                ..track(id, "A", 10, "One", None)
            })
            .collect(),
    );
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let view = catalog.sorted(all.clone(), sort(SortColumn::Duration, false));
    assert_eq!(ids(&catalog, &view), [3, 1, 2]);
    let view = catalog.sorted(all, sort(SortColumn::Duration, true));
    assert_eq!(ids(&catalog, &view), [1, 3, 2]);
}

#[test]
fn sorting_by_bit_rate_puts_tracks_of_unknown_rate_last_in_both_directions() {
    let catalog = Catalog::new(
        [(1, Some(256)), (2, None), (3, Some(1411)), (4, Some(128))]
            .into_iter()
            .map(|(id, kbps)| ListTrack {
                bitrate_kbps: kbps,
                ..track(id, "A", 10, "One", None)
            })
            .collect(),
    );
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let view = catalog.sorted(all.clone(), sort(SortColumn::BitRate, false));
    assert_eq!(ids(&catalog, &view), [4, 1, 3, 2]);
    let view = catalog.sorted(all, sort(SortColumn::BitRate, true));
    assert_eq!(ids(&catalog, &view), [3, 1, 4, 2]);
}

#[test]
fn sorting_by_file_type_groups_each_codec_keeping_album_order_within_it() {
    let catalog = Catalog::new(
        [(1, "mp3"), (2, "flac"), (3, "mp3"), (4, "aac")]
            .into_iter()
            .map(|(id, codec)| ListTrack {
                codec: codec.to_owned(),
                ..track(id, "A", 10, "One", None)
            })
            .collect(),
    );
    let all = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
    let view = catalog.sorted(all.clone(), sort(SortColumn::FileType, false));
    assert_eq!(ids(&catalog, &view), [4, 2, 1, 3]);
    let view = catalog.sorted(all, sort(SortColumn::FileType, true));
    assert_eq!(ids(&catalog, &view), [1, 3, 2, 4]);
}

#[test]
fn an_album_view_has_no_sort() {
    let catalog = library();
    let view = catalog.view(Grouping::AlbumArtist, &FilterValue::All, None);
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
fn sorting_keeps_only_the_filtered_and_searched_tracks() {
    let catalog = library();
    let matches = HashSet::from([4, 5, 6]);
    let radiohead = FilterValue::Text(normalize::key("Radiohead"));
    let view = catalog.view(Grouping::AlbumArtist, &radiohead, Some(&matches));
    let view = catalog.sorted(view, sort(SortColumn::TrackNumber, true));
    assert_eq!(ids(&catalog, &view), [5, 4]);
}

#[test]
fn sorting_an_empty_view_gives_an_empty_view() {
    let catalog = library();
    let view = catalog.sorted(View::default(), sort(SortColumn::Title, false));
    assert!(view.is_empty());
    assert!(view.albums.is_empty());
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
        | Grouping::RecentlyAdded
        | Grouping::RecentlyPlayed => 7,
    }
}

#[test]
fn all_has_every_grouping_once_in_declaration_order() {
    assert_eq!(Grouping::ALL.len(), grouping_count());
    for (i, grouping) in Grouping::ALL.into_iter().enumerate() {
        assert_eq!(grouping as usize, i, "{grouping:?}");
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

#[test]
fn every_filters_count_is_the_size_of_its_view() {
    let catalog = Catalog::new(vec![
        with_genre(added(track(1, "A", 10, "One", Some(1999)), 5), Some("Rock")),
        played(with_artist(track(2, "A", 10, "One", Some(1999)), None), 7),
        with_genre(added(track(3, "B", 20, "Two", None), 6), None),
        track(4, "B", 30, "Three", Some(2005)),
    ]);
    let matches = HashSet::from([1, 3, 4]);
    for grouping in Grouping::ALL {
        for matches in [None, Some(&matches)] {
            for filter in catalog.filters(grouping, matches) {
                let view = catalog.view(grouping, &filter.value, matches);
                assert_eq!(view.len(), filter.tracks, "{grouping:?} {:?}", filter.value);
            }
        }
    }
}
