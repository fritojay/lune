use super::*;
use crate::db;

fn tracks(ids: impl IntoIterator<Item = i64>) -> Vec<(i64, PathBuf)> {
    ids.into_iter()
        .map(|id| (id, PathBuf::from(format!("{id}.mp3"))))
        .collect()
}

fn ids(queue: &Queue) -> Vec<i64> {
    queue.items().iter().map(|item| item.track_id).collect()
}

fn slot_of(queue: &Queue, track_id: i64) -> SlotId {
    queue
        .items()
        .iter()
        .find(|item| item.track_id == track_id)
        .unwrap()
        .slot
}

fn queue_of(ids: impl IntoIterator<Item = i64>, start: usize) -> Queue {
    let mut queue = Queue::default();
    queue.replace(tracks(ids), start);
    queue
}

#[test]
fn replace_snapshots_the_list_and_starts_at_the_clicked_track() {
    let mut queue = queue_of([1, 2, 3], 0);
    let started = queue.replace(tracks([10, 11, 12, 13]), 2).unwrap();
    assert_eq!(started.track_id, 12);
    assert_eq!(ids(&queue), [10, 11, 12, 13]);
    assert_eq!(queue.current_index(), Some(2));
    assert_eq!(queue.next_after(slot_of(&queue, 12)).unwrap().track_id, 13);
    assert_eq!(
        queue.previous_before(slot_of(&queue, 12)).unwrap().track_id,
        11
    );
}

#[test]
fn replace_with_nothing_or_a_bad_start_leaves_nothing_current() {
    let mut queue = Queue::default();
    assert!(queue.replace(Vec::new(), 0).is_none());
    assert!(queue.is_empty());
    assert!(queue.replace(tracks([1, 2]), 5).is_none());
    assert_eq!(queue.current_index(), None);
}

#[test]
fn the_ends_of_the_queue_have_no_next_or_previous() {
    let queue = queue_of([1], 0);
    let only = slot_of(&queue, 1);
    assert!(queue.next_after(only).is_none());
    assert!(queue.previous_before(only).is_none());
    assert!(queue.next_after(SlotId(999)).is_none());
}

#[test]
fn the_same_track_can_be_queued_twice_in_separate_slots() {
    let mut queue = queue_of([1, 2], 0);
    queue.add(tracks([1]));
    assert_eq!(ids(&queue), [1, 2, 1]);
    let slots: HashSet<SlotId> = queue.items().iter().map(|i| i.slot).collect();
    assert_eq!(slots.len(), 3);
    queue.set_current(queue.items()[2].slot);
    assert_eq!(queue.current_index(), Some(2));
}

#[test]
fn play_next_inserts_after_the_current_track_and_changes_what_plays_next() {
    let mut queue = queue_of([1, 2, 3], 1);
    queue.play_next(tracks([7, 8]));
    assert_eq!(ids(&queue), [1, 2, 7, 8, 3]);
    assert_eq!(queue.next_after(slot_of(&queue, 2)).unwrap().track_id, 7);
    assert_eq!(queue.current().unwrap().track_id, 2);
}

#[test]
fn play_next_with_nothing_current_inserts_at_the_front() {
    let mut queue = Queue::default();
    queue.add(tracks([1, 2]));
    queue.play_next(tracks([9]));
    assert_eq!(ids(&queue), [9, 1, 2]);
}

#[test]
fn add_to_queue_appends() {
    let mut queue = queue_of([1, 2], 0);
    queue.add(tracks([3]));
    assert_eq!(ids(&queue), [1, 2, 3]);
    queue.add(Vec::new());
    assert_eq!(ids(&queue), [1, 2, 3]);
}

#[test]
fn removing_tracks_before_the_current_one_keeps_it_current() {
    let mut queue = queue_of([1, 2, 3, 4], 2);
    queue.remove(&[slot_of(&queue, 1), slot_of(&queue, 4)]);
    assert_eq!(ids(&queue), [2, 3]);
    assert_eq!(queue.current().unwrap().track_id, 3);
}

#[test]
fn removing_the_playing_track_continues_with_the_one_that_took_its_place() {
    let mut queue = queue_of([1, 2, 3], 1);
    let playing = slot_of(&queue, 2);
    queue.remove(&[playing]);
    assert_eq!(ids(&queue), [1, 3]);
    assert!(queue.current().is_none());
    assert_eq!(queue.next_after(playing).unwrap().track_id, 3);
    assert_eq!(queue.previous_before(playing).unwrap().track_id, 1);
    // Play Next still lands right after where the removed track was.
    queue.play_next(tracks([9]));
    assert_eq!(ids(&queue), [1, 9, 3]);
}

#[test]
fn removing_the_last_playing_track_leaves_nothing_next() {
    let mut queue = queue_of([1, 2], 1);
    let playing = slot_of(&queue, 2);
    queue.remove(&[playing]);
    assert!(queue.next_after(playing).is_none());
}

#[test]
fn move_to_reorders_and_keeps_the_current_track() {
    let mut queue = queue_of([1, 2, 3, 4, 5], 2);
    // Drag 4 and 1 (in queue order: 1, 4) to before 3.
    queue.move_to(&[slot_of(&queue, 4), slot_of(&queue, 1)], 2);
    assert_eq!(ids(&queue), [2, 1, 4, 3, 5]);
    assert_eq!(queue.current().unwrap().track_id, 3);
    assert_eq!(queue.next_after(slot_of(&queue, 3)).unwrap().track_id, 5);

    queue.move_to(&[slot_of(&queue, 2)], 99);
    assert_eq!(ids(&queue), [1, 4, 3, 5, 2]);
    queue.move_to(&[slot_of(&queue, 2)], 0);
    assert_eq!(ids(&queue), [2, 1, 4, 3, 5]);
    assert_eq!(queue.current().unwrap().track_id, 3);
}

#[test]
fn clear_empties_the_queue() {
    let mut queue = queue_of([1, 2], 0);
    queue.clear();
    assert!(queue.is_empty());
    assert!(queue.current().is_none());
}

#[test]
fn shuffle_is_a_permutation_with_the_current_track_first() {
    let list: Vec<i64> = (1..=50).collect();
    let mut queue = queue_of(list.clone(), 20);
    queue.set_shuffle(true);
    assert!(queue.shuffle());
    assert_eq!(queue.items()[0].track_id, 21);
    assert_eq!(queue.current().unwrap().track_id, 21);
    let mut shuffled = ids(&queue);
    assert_ne!(shuffled, list, "50 tracks came back in order");
    shuffled.sort_unstable();
    assert_eq!(shuffled, list);
}

#[test]
fn turning_shuffle_off_restores_the_original_order_and_keeps_the_current_track() {
    let mut queue = queue_of(1..=10, 3);
    queue.set_shuffle(true);
    let other = queue.items()[5].slot;
    queue.set_current(other);
    let current = queue.current().unwrap().track_id;
    queue.set_shuffle(false);
    assert_eq!(ids(&queue), (1..=10).collect::<Vec<_>>());
    assert_eq!(queue.current().unwrap().track_id, current);
}

#[test]
fn edits_while_shuffled_are_kept_when_shuffle_is_turned_off() {
    let mut queue = queue_of(1..=5, 1);
    queue.set_shuffle(true);
    queue.play_next(tracks([20]));
    assert_eq!(queue.items()[1].track_id, 20, "plays next even shuffled");
    queue.add(tracks([30]));
    queue.remove(&[slot_of(&queue, 4)]);
    queue.set_shuffle(false);
    assert_eq!(ids(&queue), [1, 2, 20, 3, 5, 30]);
}

#[test]
fn replacing_while_shuffled_shuffles_the_new_list_from_the_clicked_track() {
    let mut queue = Queue::default();
    queue.set_shuffle(true);
    let started = queue.replace(tracks(1..=20), 7).unwrap().track_id;
    assert_eq!(started, 8);
    assert_eq!(queue.items()[0].track_id, 8);
    queue.set_shuffle(false);
    assert_eq!(ids(&queue), (1..=20).collect::<Vec<_>>());
    assert_eq!(queue.current().unwrap().track_id, 8);
}

#[test]
fn shuffling_an_empty_or_single_queue_works() {
    let mut queue = Queue::default();
    queue.set_shuffle(true);
    queue.set_shuffle(false);
    let mut queue = queue_of([1], 0);
    queue.set_shuffle(true);
    assert_eq!(ids(&queue), [1]);
}

fn library_with_tracks(conn: &Connection, ids: &[i64]) {
    conn.execute(
        "INSERT INTO albums (id, album_artist, album_artist_key, title, title_key)
         VALUES (1, 'A', 'a', 'T', 't')",
        [],
    )
    .unwrap();
    for id in ids {
        conn.execute(
            "INSERT INTO tracks (id, path, mtime_ns, size, album_id, duration_ms, codec)
             VALUES (?1, ?2, 0, 0, 1, 1000, 'mp3')",
            params![id, format!("{id}.mp3")],
        )
        .unwrap();
    }
}

#[test]
fn an_empty_database_loads_an_empty_queue() {
    let conn = db::open_in_memory().unwrap();
    let queue = load(&conn).unwrap();
    assert!(queue.is_empty());
    assert!(!queue.shuffle());
}

#[test]
fn the_queue_and_cursor_survive_a_save_and_load() {
    let mut conn = db::open_in_memory().unwrap();
    library_with_tracks(&conn, &[1, 2, 3]);
    let mut queue = queue_of([3, 1, 2, 1], 2);
    save(&mut conn, &queue).unwrap();
    let loaded = load(&conn).unwrap();
    assert_eq!(ids(&loaded), [3, 1, 2, 1]);
    assert_eq!(loaded.current_index(), Some(2));
    assert_eq!(loaded.items()[0].path, PathBuf::from("3.mp3"));

    queue.set_current(queue.items()[3].slot);
    save_cursor(&conn, &queue).unwrap();
    assert_eq!(load(&conn).unwrap().current_index(), Some(3));

    queue.clear();
    save(&mut conn, &queue).unwrap();
    assert!(load(&conn).unwrap().is_empty());
}

#[test]
fn a_shuffled_queue_restores_both_orders() {
    let mut conn = db::open_in_memory().unwrap();
    library_with_tracks(&conn, &(1..=10).collect::<Vec<_>>());
    let mut queue = queue_of(1..=10, 4);
    queue.set_shuffle(true);
    save(&mut conn, &queue).unwrap();
    let mut loaded = load(&conn).unwrap();
    assert!(loaded.shuffle());
    assert_eq!(ids(&loaded), ids(&queue));
    assert_eq!(loaded.current().unwrap().track_id, 5);
    loaded.set_shuffle(false);
    assert_eq!(ids(&loaded), (1..=10).collect::<Vec<_>>());
    assert_eq!(loaded.current().unwrap().track_id, 5);
}

#[test]
fn tracks_deleted_from_the_library_are_dropped_on_load() {
    let mut conn = db::open_in_memory().unwrap();
    library_with_tracks(&conn, &[1, 2, 3, 4]);
    let mut queue = queue_of([1, 2, 3, 4], 3);
    queue.set_shuffle(true);
    queue.set_shuffle(false);
    queue.set_shuffle(true);
    save(&mut conn, &queue).unwrap();
    conn.execute("DELETE FROM tracks WHERE id IN (1, 3)", [])
        .unwrap();

    let mut loaded = load(&conn).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded.current().unwrap().track_id, 4);
    loaded.set_shuffle(false);
    assert_eq!(ids(&loaded), [2, 4]);
}

#[test]
fn a_deleted_current_track_loads_with_nothing_current() {
    let mut conn = db::open_in_memory().unwrap();
    library_with_tracks(&conn, &[1, 2]);
    save(&mut conn, &queue_of([1, 2], 1)).unwrap();
    conn.execute("DELETE FROM tracks WHERE id = 2", []).unwrap();
    let loaded = load(&conn).unwrap();
    assert_eq!(ids(&loaded), [1]);
    assert!(loaded.current().is_none());
}

#[test]
fn rescans_never_alter_the_saved_queue() {
    // The queue table has no foreign key; only loading drops missing tracks.
    let mut conn = db::open_in_memory().unwrap();
    library_with_tracks(&conn, &[1, 2]);
    save(&mut conn, &queue_of([1, 2], 0)).unwrap();
    conn.execute("DELETE FROM tracks WHERE id = 2", []).unwrap();
    let saved: i64 = conn
        .query_row("SELECT COUNT(*) FROM queue", [], |r| r.get(0))
        .unwrap();
    assert_eq!(saved, 2);
}

#[test]
fn missing_tracks_stay_in_the_loaded_queue() {
    let mut conn = db::open_in_memory().unwrap();
    library_with_tracks(&conn, &[1, 2, 3]);
    save(&mut conn, &queue_of([1, 2, 3], 1)).unwrap();
    conn.execute("UPDATE tracks SET missing_ns = 1 WHERE id = 2", [])
        .unwrap();
    let queue = load(&conn).unwrap();
    assert_eq!(ids(&queue), [1, 2, 3]);
    assert_eq!(queue.current_index(), Some(1), "the cursor stays on it");
}
