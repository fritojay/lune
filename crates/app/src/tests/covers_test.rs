use std::sync::mpsc;
use std::time::Duration;

use super::*;

const WAIT: Duration = Duration::from_secs(5);

/// Covers whose workers report each request on `made`, and whose listener
/// reports each finished id on the returned receiver.
fn covers_with(
    make: impl Fn(i64) -> Option<PathBuf> + Send + Sync + Clone + 'static,
) -> (Arc<Covers>, mpsc::Receiver<i64>) {
    let covers = Covers::start(1, move || make.clone());
    let (tx, rx) = mpsc::channel();
    covers.subscribe(move |id| {
        let _ = tx.send(id);
    });
    (covers, rx)
}

#[test]
fn a_cover_is_queued_then_ready_after_the_listener_hears_of_it() {
    let (covers, ready) = covers_with(|id| Some(PathBuf::from(format!("{id}.jpg"))));
    assert_eq!(covers.path(7), None);
    assert_eq!(ready.recv_timeout(WAIT).unwrap(), 7);
    assert_eq!(covers.path(7), Some(PathBuf::from("7.jpg")));
}

#[test]
fn a_failed_cover_is_not_retried() {
    let (made_tx, made) = mpsc::channel();
    let made_tx = Arc::new(Mutex::new(made_tx));
    let (covers, ready) = covers_with(move |id| {
        let _ = made_tx.lock().unwrap().send(id);
        None
    });
    assert_eq!(covers.path(1), None);
    ready.recv_timeout(WAIT).unwrap();
    assert_eq!(covers.path(1), None);
    assert_eq!(covers.path(1), None);
    assert_eq!(made.try_iter().count(), 1);
}

#[test]
fn repeated_requests_while_queued_make_the_cover_once() {
    let (gate_tx, gate) = mpsc::channel::<()>();
    let gate = Arc::new(Mutex::new(gate));
    let (made_tx, made) = mpsc::channel();
    let made_tx = Arc::new(Mutex::new(made_tx));
    let (covers, ready) = covers_with(move |id| {
        gate.lock().unwrap().recv_timeout(WAIT).unwrap();
        let _ = made_tx.lock().unwrap().send(id);
        Some(PathBuf::new())
    });
    for _ in 0..10 {
        covers.path(3);
    }
    gate_tx.send(()).unwrap();
    assert_eq!(ready.recv_timeout(WAIT).unwrap(), 3);
    assert_eq!(made.try_iter().collect::<Vec<_>>(), [3]);
}

#[test]
fn the_latest_request_is_served_first() {
    let (gate_tx, gate) = mpsc::channel::<()>();
    let gate = Arc::new(Mutex::new(gate));
    let (started_tx, started) = mpsc::channel();
    let started_tx = Arc::new(Mutex::new(started_tx));
    let (covers, ready) = covers_with(move |id| {
        let _ = started_tx.lock().unwrap().send(id);
        gate.lock().unwrap().recv_timeout(WAIT).unwrap();
        Some(PathBuf::new())
    });
    covers.path(1);
    // The only worker is busy with 1 while 2, 3, and 4 queue up.
    assert_eq!(started.recv_timeout(WAIT).unwrap(), 1);
    covers.path(2);
    covers.path(3);
    covers.path(4);
    for _ in 0..4 {
        gate_tx.send(()).unwrap();
    }
    let order: Vec<i64> = (0..4).map(|_| ready.recv_timeout(WAIT).unwrap()).collect();
    assert_eq!(order, [1, 4, 3, 2]);
}

#[test]
fn forgotten_covers_are_made_again() {
    let (covers, ready) = covers_with(|id| Some(PathBuf::from(format!("{id}.jpg"))));
    covers.path(5);
    ready.recv_timeout(WAIT).unwrap();
    covers.forget();
    assert_eq!(covers.path(5), None, "queued again");
    assert_eq!(ready.recv_timeout(WAIT).unwrap(), 5);
    assert!(covers.path(5).is_some());
}

#[test]
fn a_listener_can_ask_for_the_cover_it_heard_of() {
    let covers = Covers::start(1, || |id| Some(PathBuf::from(format!("{id}.jpg"))));
    let (tx, rx) = mpsc::channel();
    let weak = Arc::downgrade(&covers);
    covers.subscribe(move |id| {
        let _ = tx.send(weak.upgrade().and_then(|covers| covers.path(id)));
    });
    assert_eq!(covers.path(3), None);
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), Some(PathBuf::from("3.jpg")));
}

#[test]
fn rows_by_cover_lists_the_rows_of_each_cover_and_skips_rows_without_one() {
    let rows = rows_by_cover([Some(7), None, Some(7), Some(8)]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[&7], [0, 2]);
    assert_eq!(rows[&8], [3]);
}

#[test]
fn rows_by_cover_of_no_rows_is_empty() {
    assert!(rows_by_cover([]).is_empty());
}

#[test]
fn a_cover_url_names_the_local_file() {
    let path = std::env::temp_dir().join("thumbnails").join("a b#1.jpg");
    let url = file_url(&path).unwrap();
    assert_eq!(url.scheme().map(|s| s.to_string()).as_deref(), Some("file"));
    let local = url.to_local_file().unwrap().to_string();
    assert_eq!(PathBuf::from(local), path);
}
