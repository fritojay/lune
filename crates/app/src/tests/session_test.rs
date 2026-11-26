use super::*;
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
    set_playlists(Vec::new());
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
fn nothing_is_sent_without_a_change() {
    let changes = recorded();
    let _ = (catalog(), view(), missing());
    assert!(changes.borrow().is_empty());
}
