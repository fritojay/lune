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
