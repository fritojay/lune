use super::*;
use lune_core::{Grouping, Repeat, SortColumn, TrackFilter};

use crate::test_util::list_track;

const ALL: [Change; 13] = [
    Change::Catalog,
    Change::View,
    Change::ViewRefreshed,
    Change::Missing,
    Change::LeftOut,
    Change::Playlists,
    Change::Queue,
    Change::Played,
    Change::Browse,
    Change::Columns,
    Change::OutputDevice,
    Change::Folders,
    Change::NowPlaying,
];

/// The changes sent from now on, in order.
fn recorded() -> Rc<RefCell<Vec<Change>>> {
    let changes = Rc::new(RefCell::new(Vec::new()));
    for change in ALL {
        let sink = Rc::clone(&changes);
        subscribe(change, move || sink.borrow_mut().push(change));
    }
    changes
}

#[test]
fn each_setter_sends_its_change() {
    let changes = recorded();
    set_catalog(Catalog::new(Vec::new()));
    set_view(View::default(), false);
    set_view(View::default(), true);
    set_missing(Vec::new());
    set_left_out(Arc::default());
    load_playlists(Vec::new(), id("2"));
    queue_edited();
    set_browse(BrowseState::default());
    set_browse(BrowseState::default());
    set_hidden_columns(BTreeSet::from([SortColumn::Artist]));
    set_hidden_columns(BTreeSet::from([SortColumn::Artist]));
    set_has_folders(true);
    set_has_folders(true);
    set_has_folders(false);
    set_now_playing(Some(4));
    set_now_playing(Some(4));
    set_now_playing(None);
    assert_eq!(
        *changes.borrow(),
        [
            Change::Catalog,
            Change::ViewRefreshed,
            Change::View,
            Change::Missing,
            Change::LeftOut,
            Change::Playlists,
            Change::Queue,
            Change::Browse,
            Change::Columns,
            Change::Folders,
            Change::Folders,
            Change::NowPlaying,
            Change::NowPlaying,
        ]
    );
}

#[test]
fn only_the_listeners_of_a_change_hear_it_in_the_order_they_subscribed() {
    let order = Rc::new(RefCell::new(Vec::new()));
    for (listener, change) in [(0, Change::Queue), (1, Change::Missing), (2, Change::Queue)] {
        let order = Rc::clone(&order);
        subscribe(change, move || order.borrow_mut().push(listener));
    }
    queue_edited();
    queue_edited();
    assert_eq!(*order.borrow(), [0, 2, 0, 2]);
}

#[test]
fn a_listener_can_read_the_new_state_and_subscribe() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    subscribe(Change::Catalog, move || {
        sink.borrow_mut().push(catalog().map_or(0, |c| c.len()));
        subscribe(Change::Catalog, || {});
    });
    set_catalog(Catalog::new(vec![list_track(1, 1, None)]));
    assert_eq!(*seen.borrow(), [1]);
}

#[test]
fn a_play_updates_the_catalog_the_list_models_hold_rather_than_a_copy() {
    set_catalog(Catalog::new(vec![
        list_track(1, 1, None),
        list_track(2, 2, None),
    ]));
    let held = catalog().unwrap();
    let changes = recorded();
    record_play(2);
    assert!(Rc::ptr_eq(&held, &catalog().unwrap()));
    assert_eq!(*changes.borrow(), [Change::Played]);
    let played = held.view(Grouping::RecentlyPlayed, &TrackFilter::All);
    assert_eq!(
        played
            .tracks
            .iter()
            .map(|&i| held.track(i).unwrap().id)
            .collect::<Vec<_>>(),
        [2]
    );
}

#[test]
fn nothing_is_sent_without_a_change() {
    let changes = recorded();
    let _ = (catalog(), view(), missing(), left_out());
    assert!(changes.borrow().is_empty());
}

fn id(id: &str) -> ListId {
    id.parse().unwrap()
}

fn playlist(id: &str, track_ids: Vec<i64>) -> Playlist {
    Playlist {
        id: self::id(id),
        name: format!("list {id}"),
        track_ids,
    }
}

#[test]
fn a_playlist_edit_shows_at_once_and_is_announced() {
    load_playlists(
        vec![playlist("2", vec![5]), playlist("3", vec![1])],
        id("4"),
    );
    let changes = recorded();
    edit_playlists(|playlists| {
        playlists[0].track_ids.push(1);
        playlists.remove(1);
    });
    assert_eq!(*playlists(), [playlist("2", vec![5, 1])]);
    assert_eq!(*changes.borrow(), [Change::Playlists]);
}

#[test]
fn before_the_playlists_load_nothing_can_change_them() {
    let changes = recorded();
    edit_playlists(|playlists| playlists.push(playlist("2", Vec::new())));
    assert_eq!(create_playlist("Mix".to_owned(), vec![1]), None);
    assert!(playlists().is_empty());
    assert!(changes.borrow().is_empty());
}

#[test]
fn new_playlists_take_the_unused_id_then_the_ones_after_it() {
    load_playlists(vec![playlist("2", Vec::new())], id("9"));
    assert_eq!(create_playlist("A".to_owned(), vec![1]), Some(id("9")));
    assert_eq!(create_playlist("B".to_owned(), Vec::new()), Some(id("10")));
    let all = playlists();
    let made: Vec<(ListId, &str, &[i64])> = all
        .iter()
        .map(|p| (p.id, p.name.as_str(), &p.track_ids[..]))
        .collect();
    assert_eq!(
        made,
        [
            (id("2"), "list 2", &[][..]),
            (id("9"), "A", &[1][..]),
            (id("10"), "B", &[][..])
        ]
    );
}

#[test]
fn an_error_reported_on_any_thread_reaches_every_error_listener() {
    let (tx, rx) = std::sync::mpsc::channel();
    for listener in 0..2 {
        let tx = tx.clone();
        // Other tests may report errors of their own meanwhile.
        subscribe_errors(move |message| {
            if message == "the disk is on fire" {
                let _ = tx.send(listener);
            }
        });
    }
    std::thread::spawn(|| report_error("the disk is on fire"))
        .join()
        .unwrap();
    assert_eq!(rx.try_iter().collect::<Vec<_>>(), [0, 1]);
}

#[test]
fn the_queue_is_unlocked_while_a_save_writes_what_it_copied() {
    let queue = Mutex::new(Queue::default());
    lock(&queue).set_repeat(Repeat::All);
    let expected = lock(&queue).cursor_state();
    let mut written = None;
    save_from_queue(&queue, Queue::cursor_state, |cursor| {
        assert!(
            queue.try_lock().is_ok(),
            "the queue is locked during the write"
        );
        written = Some(*cursor);
        Ok(())
    })
    .unwrap();
    assert_eq!(written, Some(expected));
    save_from_queue(&queue, Queue::clone, |_| {
        assert!(
            queue.try_lock().is_ok(),
            "the queue is locked during the write"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn only_a_first_busy_save_at_a_track_change_fails_quietly() {
    // (save, busy, quiet): each row follows the ones before it.
    let rows = [
        (Save::TrackChanged, true, true),
        (Save::TrackChanged, true, false),
        (Save::TrackChanged, true, false),
        (Save::TrackChanged, false, false),
    ];
    let mut failures = QueueSaveFailures::default();
    for (i, (save, busy, quiet)) in rows.into_iter().enumerate() {
        assert_eq!(failures.is_quiet(&save, busy), quiet, "row {i}");
    }
    failures.saved();
    assert!(failures.is_quiet(&Save::TrackChanged, true));

    for save in [Save::Cursor, Save::Queue] {
        let mut failures = QueueSaveFailures::default();
        assert!(!failures.is_quiet(&save, true));
    }
    let mut failures = QueueSaveFailures::default();
    assert!(!failures.is_quiet(&Save::Cursor, true));
    assert!(
        !failures.is_quiet(&Save::TrackChanged, true),
        "after another failure"
    );
}
