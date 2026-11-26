use proptest::prelude::*;

use super::*;
use crate::db;
use crate::test_util::library_with_tracks;

const LIBRARY: [i64; 5] = [1, 2, 3, 4, 5];

fn save(conn: &mut Connection, id: ListId, track_ids: &[i64]) {
    let tx = conn.transaction().unwrap();
    save_items(&tx, id, track_ids).unwrap();
    tx.commit().unwrap();
}

#[test]
fn a_new_database_has_an_empty_queue_list() {
    let conn = db::open_in_memory().unwrap();
    assert!(load_items(&conn, ListId::QUEUE).unwrap().is_empty());
}

proptest! {
    #[test]
    fn items_load_as_last_saved_and_keep_their_place_when_tracks_are_forgotten(
        earlier in prop::collection::vec(0..8i64, 0..10),
        track_ids in prop::collection::vec(0..8i64, 0..20),
        forgotten in prop::sample::subsequence(LIBRARY.to_vec(), 0..=LIBRARY.len()),
    ) {
        let mut conn = db::open_in_memory().unwrap();
        library_with_tracks(&conn, &LIBRARY);
        save(&mut conn, ListId::QUEUE, &earlier);
        save(&mut conn, ListId::QUEUE, &track_ids);
        for id in &forgotten {
            conn.execute("UPDATE tracks SET missing_ns = 1 WHERE id = ?1", [id]).unwrap();
        }
        crate::scan::forget_missing(&mut conn, None).unwrap();

        let expected: Vec<(i64, Option<PathBuf>)> = track_ids
            .iter()
            .map(|&id| {
                let in_library = LIBRARY.contains(&id) && !forgotten.contains(&id);
                (id, in_library.then(|| PathBuf::from(format!("{id}.mp3"))))
            })
            .collect();
        prop_assert_eq!(load_items(&conn, ListId::QUEUE).unwrap(), expected);
    }
}

proptest! {
    #[test]
    fn playlists_load_oldest_first_with_their_items_and_without_the_queue(
        playlists in prop::collection::vec(
            (prop::option::of("[a-z ]{0,6}"), 0..3i64, prop::collection::vec(0..8i64, 0..10)),
            0..5,
        ),
        queued in prop::collection::vec(0..8i64, 0..10),
    ) {
        let mut conn = db::open_in_memory().unwrap();
        save(&mut conn, ListId::QUEUE, &queued);
        let mut expected = Vec::new();
        for (id, (name, created_ns, track_ids)) in (2..).zip(&playlists) {
            conn.execute(
                "INSERT INTO lists (id, kind, name, created_ns)
                 VALUES (:id, 'playlist', :name, :created_ns)",
                named_params! { ":id": id, ":name": name, ":created_ns": created_ns },
            )
            .unwrap();
            save(&mut conn, ListId(id), track_ids);
            expected.push((*created_ns, Playlist {
                id: ListId(id),
                name: name.clone().unwrap_or_default(),
                track_ids: track_ids.clone(),
            }));
        }
        expected.sort_by_key(|(created_ns, playlist)| (*created_ns, playlist.id.0));
        let expected: Vec<Playlist> = expected.into_iter().map(|(_, p)| p).collect();
        prop_assert_eq!(load_playlists(&conn).unwrap(), expected);
    }
}

/// Ids 1 to 6, so some snapshots claim the queue's id.
fn arb_playlists() -> impl Strategy<Value = Vec<Playlist>> {
    prop::collection::btree_map(
        1..7i64,
        ("[a-zA-Z ]{0,5}", prop::collection::vec(0..8i64, 0..5)),
        0..5,
    )
    .prop_map(|playlists| {
        playlists
            .into_iter()
            .map(|(id, (name, track_ids))| Playlist {
                id: ListId(id),
                name,
                track_ids,
            })
            .collect()
    })
}

proptest! {
    #[test]
    fn saved_playlists_load_as_the_last_snapshot_and_the_queue_is_never_touched(
        snapshots in prop::collection::vec(arb_playlists(), 1..5),
        queued in prop::collection::vec(0..8i64, 0..5),
    ) {
        let mut conn = db::open_in_memory().unwrap();
        save(&mut conn, ListId::QUEUE, &queued);
        for snapshot in &snapshots {
            let tx = conn.transaction().unwrap();
            save_playlists(&tx, snapshot).unwrap();
            tx.commit().unwrap();

            let mut loaded = load_playlists(&conn).unwrap();
            loaded.sort_by_key(|p| p.id.0);
            let expected: Vec<&Playlist> =
                snapshot.iter().filter(|p| p.id != ListId::QUEUE).collect();
            prop_assert_eq!(loaded.iter().collect::<Vec<_>>(), expected);
            let queue: Vec<i64> =
                load_items(&conn, ListId::QUEUE).unwrap().into_iter().map(|(id, _)| id).collect();
            prop_assert_eq!(&queue, &queued);
        }
    }

    #[test]
    fn by_name_is_every_playlist_ordered_by_name_ignoring_case_then_id(
        playlists in arb_playlists(),
    ) {
        let sorted = Playlist::by_name(&playlists);
        let mut ids: Vec<i64> = sorted.iter().map(|p| p.id.0).collect();
        let keys: Vec<(String, i64)> =
            sorted.iter().map(|p| (crate::normalize::key(&p.name), p.id.0)).collect();
        prop_assert!(keys.is_sorted());
        ids.sort_unstable();
        prop_assert_eq!(ids, playlists.iter().map(|p| p.id.0).collect::<Vec<_>>());
    }

    #[test]
    fn a_new_id_is_neither_a_playlists_nor_the_queues(playlists in arb_playlists()) {
        let id = Playlist::new_id(&playlists);
        prop_assert_ne!(id, ListId::QUEUE);
        prop_assert!(playlists.iter().all(|p| p.id != id));
    }

    #[test]
    fn a_shown_playlist_stays_while_it_exists_and_else_is_the_first_by_name(
        playlists in arb_playlists(),
        id in prop::option::of(1..9i64),
    ) {
        let id = id.map(ListId);
        let expected = match id {
            Some(id) if playlists.iter().any(|p| p.id == id) => Some(id),
            _ => Playlist::by_name(&playlists).first().map(|p| p.id),
        };
        prop_assert_eq!(Playlist::existing_or_first(id, &playlists), expected);
    }
}

fn playlist(track_ids: Vec<i64>) -> Playlist {
    Playlist {
        id: ListId(2),
        name: String::new(),
        track_ids,
    }
}

proptest! {
    #[test]
    fn removing_keeps_the_other_tracks_in_order(
        track_ids in prop::collection::vec(0..5i64, 0..10),
        positions in prop::collection::vec(0..12usize, 0..6),
    ) {
        let mut edited = playlist(track_ids.clone());
        edited.remove(&positions);
        let kept: Vec<i64> = track_ids
            .iter()
            .enumerate()
            .filter(|(position, _)| !positions.contains(position))
            .map(|(_, &id)| id)
            .collect();
        prop_assert_eq!(edited.track_ids, kept);
    }

    #[test]
    fn moving_puts_the_tracks_together_in_order_before_the_target_and_keeps_the_rest(
        track_ids in prop::collection::vec(0..5i64, 0..10),
        positions in prop::collection::vec(0..12usize, 0..6),
        to in 0..12usize,
    ) {
        let mut edited = playlist(track_ids.clone());
        edited.move_to(&positions, to);
        let picked = |moving: bool| -> Vec<i64> {
            track_ids
                .iter()
                .enumerate()
                .filter(|(position, _)| positions.contains(position) == moving)
                .map(|(_, &id)| id)
                .collect()
        };
        let (moved, rest) = (picked(true), picked(false));
        let to = to.min(track_ids.len());
        let at = (0..to).filter(|position| !positions.contains(position)).count();
        let mut expected = rest;
        expected.splice(at..at, moved);
        prop_assert_eq!(edited.track_ids, expected);
    }
}

proptest! {
    #[test]
    fn a_list_id_parses_back_from_how_it_is_shown(id in any::<i64>()) {
        prop_assert_eq!(ListId(id).to_string().parse::<ListId>().unwrap(), ListId(id));
    }
}
