use std::sync::mpsc;

use super::*;

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn ticks_go_on_until_they_can_no_longer_be_queued() {
    let mut queued = 0;
    run(
        || Duration::ZERO,
        |tick| {
            tick();
            queued += 1;
            queued < 3
        },
    );
    assert_eq!(queued, 3);
}

#[test]
fn no_tick_is_queued_while_the_last_one_waits_to_run() {
    let (ticks_tx, ticks) = mpsc::channel();
    let done = Arc::new(AtomicBool::new(false));
    let stop = Arc::clone(&done);
    let ticker = std::thread::spawn(move || {
        run(
            || Duration::from_millis(1),
            |tick| !stop.load(Ordering::Acquire) && ticks_tx.send(tick).is_ok(),
        );
    });
    let first = ticks.recv_timeout(WAIT).unwrap();
    // Many intervals pass while the first tick waits.
    std::thread::sleep(Duration::from_millis(50));
    assert!(ticks.try_recv().is_err());
    first();
    let second = ticks.recv_timeout(WAIT).unwrap();
    done.store(true, Ordering::Release);
    second();
    ticker.join().unwrap();
}
