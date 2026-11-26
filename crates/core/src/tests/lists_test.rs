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

proptest! {
    #[test]
    fn appending_adds_to_the_end_of_a_playlist_and_never_changes_the_queue_or_other_lists(
        appends in prop::collection::vec(
            (0..4i64, prop::collection::vec(0..8i64, 0..5)),
            0..8,
        ),
        queued in prop::collection::vec(0..8i64, 0..5),
    ) {
        // Lists 2 and 3 are playlists; 1 is the queue and 0 doesn't exist.
        let mut conn = db::open_in_memory().unwrap();
        save(&mut conn, ListId::QUEUE, &queued);
        for id in [2, 3] {
            conn.execute(
                "INSERT INTO lists (id, kind, created_ns) VALUES (:id, 'playlist', :id)",
                named_params! { ":id": id },
            )
            .unwrap();
        }
        let mut expected = [Vec::new(), Vec::new()];
        for (id, track_ids) in &appends {
            let tx = conn.transaction().unwrap();
            append_to_playlist(&tx, ListId(*id), track_ids).unwrap();
            tx.commit().unwrap();
            if let Some(playlist) = id.checked_sub(2).and_then(|i| expected.get_mut(i as usize)) {
                playlist.extend(track_ids);
            }
        }
        let loaded: Vec<Vec<i64>> =
            load_playlists(&conn).unwrap().into_iter().map(|p| p.track_ids).collect();
        prop_assert_eq!(loaded, expected);
        let queue: Vec<i64> =
            load_items(&conn, ListId::QUEUE).unwrap().into_iter().map(|(id, _)| id).collect();
        prop_assert_eq!(queue, queued);
    }
}

proptest! {
    #[test]
    fn a_list_id_parses_back_from_how_it_is_shown(id in any::<i64>()) {
        prop_assert_eq!(ListId(id).to_string().parse::<ListId>().unwrap(), ListId(id));
    }
}
