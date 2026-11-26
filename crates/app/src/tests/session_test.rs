use super::*;
use lune_core::{Grouping, TrackFilter};

use crate::test_util::list_track;

/// The changes sent from now on, in order.
fn recorded() -> Rc<RefCell<Vec<Change>>> {
    let changes = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&changes);
    subscribe(move |change| sink.borrow_mut().push(change));
    changes
}

#[test]
fn each_setter_sends_its_change() {
    let changes = recorded();
    set_catalog(Catalog::new(Vec::new()));
    set_view(View::default(), false);
    set_view(View::default(), true);
    set_missing(Vec::new());
    load_playlists(Vec::new(), id("2"));
    queue_edited();
    set_browse(BrowseState::default());
    set_browse(BrowseState::default());
    assert_eq!(
        *changes.borrow(),
        [
            Change::Catalog,
            Change::View { reset: false },
            Change::View { reset: true },
            Change::Missing,
            Change::Playlists,
            Change::Queue,
            Change::Browse,
        ]
    );
}

#[test]
fn every_listener_hears_every_change_in_the_order_they_subscribed() {
    let order = Rc::new(RefCell::new(Vec::new()));
    for listener in 0..3 {
        let order = Rc::clone(&order);
        subscribe(move |_| order.borrow_mut().push(listener));
    }
    queue_edited();
    queue_edited();
    assert_eq!(*order.borrow(), [0, 1, 2, 0, 1, 2]);
}

#[test]
fn a_listener_can_read_the_new_state_and_subscribe() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    subscribe(move |change| {
        if change == Change::Catalog {
            sink.borrow_mut().push(catalog().map_or(0, |c| c.len()));
            subscribe(|_| {});
        }
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
    let _ = (catalog(), view(), missing());
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
