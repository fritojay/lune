use proptest::prelude::*;
use proptest::sample::Index;

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

fn queue_of(ids: impl IntoIterator<Item = i64>, start: usize) -> Queue {
    let mut queue = Queue::default();
    queue.replace(tracks(ids), start);
    queue
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

#[derive(Debug, Clone)]
enum Op {
    Replace(Vec<i64>, usize),
    PlayNext(Vec<i64>),
    Add(Vec<i64>),
    Remove(Vec<Index>),
    MoveTo(Vec<Index>, usize),
    SetCurrent(Index),
    SetResume(Index, Duration),
    Clear,
    Shuffle(bool),
}

fn arb_track_ids() -> impl Strategy<Value = Vec<i64>> {
    prop::collection::vec(1..=5i64, 0..5)
}

fn arb_op() -> impl Strategy<Value = Op> {
    let picks = || prop::collection::vec(any::<Index>(), 0..3);
    prop_oneof![
        (arb_track_ids(), 0..6usize).prop_map(|(ids, start)| Op::Replace(ids, start)),
        arb_track_ids().prop_map(Op::PlayNext),
        arb_track_ids().prop_map(Op::Add),
        picks().prop_map(Op::Remove),
        (picks(), 0..8usize).prop_map(|(picks, to)| Op::MoveTo(picks, to)),
        any::<Index>().prop_map(Op::SetCurrent),
        (any::<Index>(), any::<u32>())
            .prop_map(|(pick, ms)| Op::SetResume(pick, Duration::from_millis(ms.into()))),
        Just(Op::Clear),
        any::<bool>().prop_map(Op::Shuffle),
    ]
}

/// The queue as plain lists: what plays in order, the order to restore when
/// shuffle goes off, and where the current item is (or was, once removed).
#[derive(Default)]
struct Model {
    items: Vec<(SlotId, i64)>,
    original: Option<Vec<SlotId>>,
    current: Option<SlotId>,
    place: usize,
    resume: Option<(SlotId, Duration)>,
}

impl Model {
    fn position(&self, slot: SlotId) -> Option<usize> {
        self.items.iter().position(|&(s, _)| s == slot)
    }

    fn current_index(&self) -> Option<usize> {
        self.current.and_then(|c| self.position(c))
    }

    fn slots(&self, picks: &[Index]) -> Vec<SlotId> {
        if self.items.is_empty() {
            return Vec::new();
        }
        picks
            .iter()
            .map(|pick| self.items[pick.index(self.items.len())].0)
            .collect()
    }

    /// Takes the order `queue` shuffled into, after checking it is a
    /// permutation with the current item first.
    fn adopt_shuffle(&mut self, queue: &Queue) {
        let shuffled: Vec<(SlotId, i64)> =
            queue.items().iter().map(|i| (i.slot, i.track_id)).collect();
        let (mut a, mut b) = (shuffled.clone(), self.items.clone());
        a.sort_by_key(|&(s, _)| s.0);
        b.sort_by_key(|&(s, _)| s.0);
        assert_eq!(a, b, "shuffling keeps every item");
        if self.current_index().is_some() {
            assert_eq!(
                Some(shuffled[0].0),
                self.current,
                "the current item goes first"
            );
            self.place = 0;
        }
        self.items = shuffled;
    }

    fn apply(&mut self, op: &Op, queue: &mut Queue) {
        let new = |queue: &Queue, from: usize| -> Vec<(SlotId, i64)> {
            queue.items()[from..]
                .iter()
                .map(|i| (i.slot, i.track_id))
                .collect()
        };
        match op {
            Op::Replace(ids, start) => {
                let started = queue
                    .replace(tracks(ids.clone()), *start)
                    .map(|i| i.track_id);
                assert_eq!(started, ids.get(*start).copied());
                let mut items = queue.items().to_vec();
                // Shuffle may have reordered them; slots count up as items are added.
                items.sort_by_key(|i| i.slot.0);
                self.items = items.iter().map(|i| (i.slot, i.track_id)).collect();
                assert_eq!(self.items.iter().map(|&(_, t)| t).collect::<Vec<_>>(), *ids);
                self.current = self.items.get(*start).map(|&(s, _)| s);
                self.place = if self.current.is_some() { *start } else { 0 };
                if self.original.is_some() {
                    self.original = Some(self.items.iter().map(|&(s, _)| s).collect());
                    self.adopt_shuffle(queue);
                }
            }
            Op::PlayNext(ids) => {
                let at = match self.current_index() {
                    Some(i) => i + 1,
                    None if self.current.is_some() => self.place.min(self.items.len()),
                    None => 0,
                };
                queue.play_next(tracks(ids.clone()));
                let added: Vec<(SlotId, i64)> =
                    new(queue, at).into_iter().take(ids.len()).collect();
                assert_eq!(added.iter().map(|&(_, t)| t).collect::<Vec<_>>(), *ids);
                if let Some(original) = &mut self.original {
                    let after = self
                        .current
                        .and_then(|c| original.iter().position(|&s| s == c));
                    let o = after.map_or(0, |i| i + 1);
                    original.splice(o..o, added.iter().map(|&(s, _)| s));
                }
                self.items.splice(at..at, added);
            }
            Op::Add(ids) => {
                let from = self.items.len();
                queue.add(tracks(ids.clone()));
                let added = new(queue, from);
                assert_eq!(added.iter().map(|&(_, t)| t).collect::<Vec<_>>(), *ids);
                if let Some(original) = &mut self.original {
                    original.extend(added.iter().map(|&(s, _)| s));
                }
                self.items.extend(added);
            }
            Op::Remove(picks) => {
                let slots = self.slots(picks);
                queue.remove(&slots);
                let gone = |s: &SlotId| slots.contains(s);
                self.place -= self.items[..self.place.min(self.items.len())]
                    .iter()
                    .filter(|(s, _)| gone(s))
                    .count();
                self.items.retain(|(s, _)| !gone(s));
                if let Some(original) = &mut self.original {
                    original.retain(|s| !gone(s));
                }
            }
            Op::MoveTo(picks, to) => {
                let slots = self.slots(picks);
                queue.move_to(&slots, *to);
                let to = (*to).min(self.items.len());
                let (moved, mut rest): (Vec<_>, Vec<_>) =
                    self.items.iter().partition(|(s, _)| slots.contains(s));
                let at = to
                    - self.items[..to]
                        .iter()
                        .filter(|(s, _)| slots.contains(s))
                        .count();
                rest.splice(at..at, moved);
                self.items = rest;
                if let Some(i) = self.current_index() {
                    self.place = i;
                }
            }
            Op::SetCurrent(pick) => {
                if let Some(&slot) = self.slots(&[*pick]).first() {
                    queue.set_current(slot);
                    self.current = Some(slot);
                    self.place = self.position(slot).unwrap();
                    self.resume = None;
                }
            }
            Op::SetResume(pick, position) => {
                if let Some(&slot) = self.slots(&[*pick]).first() {
                    queue.set_resume_position(slot, *position);
                    self.resume = Some((slot, *position));
                }
            }
            Op::Clear => {
                queue.clear();
                *self = Model {
                    original: self.original.as_ref().map(|_| Vec::new()),
                    ..Model::default()
                };
            }
            Op::Shuffle(on) => {
                queue.set_shuffle(*on);
                match (*on, self.original.take()) {
                    (true, None) => {
                        self.original = Some(self.items.iter().map(|&(s, _)| s).collect());
                        self.adopt_shuffle(queue);
                    }
                    (false, Some(original)) => {
                        let track = |slot| self.items.iter().find(|&&(s, _)| s == slot).unwrap().1;
                        self.items = original.iter().map(|&s| (s, track(s))).collect();
                        if let Some(i) = self.current_index() {
                            self.place = i;
                        }
                    }
                    (_, original) => self.original = original,
                }
            }
        }
    }

    fn check(&self, queue: &Queue) {
        let items: Vec<(SlotId, i64)> =
            queue.items().iter().map(|i| (i.slot, i.track_id)).collect();
        assert_eq!(items, self.items);
        for item in queue.items() {
            assert_eq!(item.path, PathBuf::from(format!("{}.mp3", item.track_id)));
        }
        let slots: HashSet<SlotId> = items.iter().map(|&(s, _)| s).collect();
        assert_eq!(slots.len(), items.len(), "every item has its own slot");
        assert_eq!(queue.shuffle(), self.original.is_some());
        assert_eq!(queue.current_index(), self.current_index());
        let resume = match self.resume {
            Some((slot, position))
                if self.current_index().is_some() && self.current == Some(slot) =>
            {
                position
            }
            _ => Duration::ZERO,
        };
        assert_eq!(queue.resume_position(), resume);
        let never_handed_out = SlotId(u64::MAX);
        assert!(queue.next_after(never_handed_out).is_none());
        assert!(queue.previous_before(never_handed_out).is_none());
        let Some(current) = self.current else { return };
        let (next, previous) = match self.current_index() {
            Some(i) => (i + 1, i.checked_sub(1)),
            None => (self.place, self.place.checked_sub(1)),
        };
        let track = |i: Option<usize>| i.and_then(|i| self.items.get(i)).map(|&(_, t)| t);
        assert_eq!(
            queue.next_after(current).map(|i| i.track_id),
            track(Some(next))
        );
        assert_eq!(
            queue.previous_before(current).map(|i| i.track_id),
            track(previous)
        );
    }
}

proptest! {
    #[test]
    fn every_edit_matches_the_list_model(ops in prop::collection::vec(arb_op(), 0..30)) {
        let mut queue = Queue::default();
        let mut model = Model::default();
        for op in &ops {
            model.apply(op, &mut queue);
            model.check(&queue);
        }
        model.apply(&Op::Shuffle(false), &mut queue);
        model.check(&queue);
    }

    #[test]
    fn the_queue_and_its_cursor_survive_a_save_and_load(
        ops in prop::collection::vec(arb_op(), 0..30),
        cursor: Index,
        resume_ms: u32,
    ) {
        let mut conn = db::open_in_memory().unwrap();
        library_with_tracks(&conn, &[1, 2, 3, 4, 5]);
        let mut queue = Queue::default();
        let mut model = Model::default();
        for op in &ops {
            model.apply(op, &mut queue);
        }
        save(&mut conn, &queue).unwrap();
        model.apply(&Op::SetCurrent(cursor), &mut queue);
        model.apply(&Op::SetResume(cursor, Duration::from_millis(resume_ms.into())), &mut queue);
        save_cursor(&conn, &queue).unwrap();
        let mut loaded = load(&conn).unwrap();
        let entries = |queue: &Queue| -> Vec<(i64, PathBuf)> {
            queue.items().iter().map(|i| (i.track_id, i.path.clone())).collect()
        };
        prop_assert_eq!(entries(&loaded), entries(&queue));
        prop_assert_eq!(loaded.current_index(), queue.current_index());
        prop_assert_eq!(loaded.shuffle(), queue.shuffle());
        prop_assert_eq!(loaded.resume_position(), queue.resume_position());
        queue.set_shuffle(false);
        loaded.set_shuffle(false);
        prop_assert_eq!(ids(&loaded), ids(&queue));
        prop_assert_eq!(loaded.current_index(), queue.current_index());
    }
}
