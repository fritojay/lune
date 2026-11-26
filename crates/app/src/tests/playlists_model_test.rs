use super::*;

#[test]
fn a_suggested_name_is_the_first_one_not_taken() {
    let playlists = |names: &[&str]| -> Vec<Playlist> {
        (2..)
            .zip(names)
            .map(|(id, name)| Playlist {
                id: id.to_string().parse().unwrap(),
                name: (*name).to_owned(),
                track_ids: Vec::new(),
            })
            .collect()
    };
    for (names, expected) in [
        (&[][..], "Mix"),
        (&["Mix 2"][..], "Mix"),
        (&["Mix"][..], "Mix 2"),
        (&["Mix", "Mix 2", "Mix 4"][..], "Mix 3"),
        (&["mix"][..], "Mix"),
    ] {
        assert_eq!(
            suggested_name("Mix", &playlists(names)),
            expected,
            "{names:?}"
        );
    }
}

#[test]
fn a_menu_shows_a_name_as_written() {
    for (name, shown) in [
        ("Mix", "Mix"),
        ("Rock & Roll", "Rock && Roll"),
        ("<b>Loud</b>", "<\u{2060}b>Loud<\u{2060}/b>"),
    ] {
        assert_eq!(menu_text(name), shown);
    }
}

mod rows {
    use lune_core::{Catalog, Sort, SortColumn, TrackFilter, View};
    use proptest::prelude::*;

    use super::super::*;
    use crate::test_util::list_track;

    /// Tracks 1 to 5 by title "1".."5"; ids 6 and up aren't in the library.
    fn catalog() -> Catalog {
        Catalog::new(
            (1..=5)
                .map(|id| {
                    let mut track = list_track(id, id % 2, None);
                    track.title = id.to_string();
                    track
                })
                .collect(),
        )
    }

    fn ids(catalog: &Catalog, view: &View) -> Vec<i64> {
        view.tracks
            .iter()
            .map(|&i| catalog.track(i).unwrap().id)
            .collect()
    }

    fn view(catalog: &Catalog, track_ids: &[i64], sorted: bool, search: &TrackFilter) -> View {
        let view = catalog.playlist_view(track_ids, search);
        if sorted {
            let sort = Sort {
                column: SortColumn::Title,
                descending: true,
            };
            catalog.sorted(view, sort)
        } else {
            view
        }
    }

    fn playlist(track_ids: Vec<i64>) -> Playlist {
        Playlist {
            id: "2".parse().unwrap(),
            name: String::new(),
            track_ids,
        }
    }

    fn arb_search() -> impl Strategy<Value = TrackFilter> {
        prop_oneof![
            Just(TrackFilter::All),
            prop::collection::hash_set(1..=5i64, 0..5)
                .prop_map(|ids| TrackFilter::Ids(std::sync::Arc::new(ids))),
        ]
    }

    proptest! {
        #[test]
        fn removing_rows_takes_out_just_those_rows_and_keeps_tracks_not_shown(
            track_ids in prop::collection::vec(1..8i64, 0..10),
            sorted in any::<bool>(),
            search in arb_search(),
            rows in prop::collection::vec(-1..12i32, 0..5),
        ) {
            let catalog = catalog();
            let before = view(&catalog, &track_ids, sorted, &search);
            let mut edited = playlist(track_ids.clone());
            let positions = catalog.playlist_positions(&before, &track_ids);
            remove_rows(&mut edited, &positions, rows.iter().copied());

            let after = view(&catalog, &edited.track_ids, sorted, &search);
            let kept: Vec<i64> = (0..)
                .zip(ids(&catalog, &before))
                .filter(|(row, _)| !rows.contains(row))
                .map(|(_, id)| id)
                .collect();
            prop_assert_eq!(ids(&catalog, &after), kept);
            let hidden = |track_ids: &[i64]| track_ids.len() - catalog.playlist_view(track_ids, &search).len();
            prop_assert_eq!(hidden(&edited.track_ids), hidden(&track_ids));
        }

        #[test]
        fn moving_rows_moves_them_as_shown_in_playlist_order(
            track_ids in prop::collection::vec(1..8i64, 0..10),
            rows in prop::collection::vec(0..12i32, 0..5),
            to in 0..12i32,
        ) {
            let catalog = catalog();
            let before = view(&catalog, &track_ids, false, &TrackFilter::All);
            let mut edited = playlist(track_ids.clone());
            let positions = catalog.playlist_positions(&before, &track_ids);
            move_rows(&mut edited, &positions, rows.iter().copied(), to);

            let shown = ids(&catalog, &before);
            let picked = |moving: bool| -> Vec<i64> {
                (0..)
                    .zip(&shown)
                    .filter(|(row, _)| rows.contains(row) == moving)
                    .map(|(_, &id)| id)
                    .collect()
            };
            let (moved, mut expected) = (picked(true), picked(false));
            let at = (0..to.min(shown.len() as i32)).filter(|row| !rows.contains(row)).count();
            expected.splice(at..at, moved);
            let after = view(&catalog, &edited.track_ids, false, &TrackFilter::All);
            prop_assert_eq!(ids(&catalog, &after), expected);
            let mut sorted_ids = edited.track_ids.clone();
            let mut original = track_ids.clone();
            sorted_ids.sort_unstable();
            original.sort_unstable();
            prop_assert_eq!(sorted_ids, original);
        }
    }
}
