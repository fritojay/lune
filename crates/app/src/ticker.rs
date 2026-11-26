//! Calls an object on the Qt thread at an interval, from Rust rather than a
//! QML Timer, so what it keeps going doesn't depend on the QML.

use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use cxx_qt::{CxxQtThread, Threading};

/// Calls `tick` on `qt_thread`'s object every `interval()`, until the
/// object is gone.
pub fn every<T: Threading + 'static>(
    qt_thread: CxxQtThread<T>,
    interval: impl Fn() -> Duration + Send + 'static,
    tick: fn(Pin<&mut T>),
) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("lune-ticker".into())
        .spawn(move || {
            run(interval, |run_tick| {
                qt_thread
                    .queue(move |object| {
                        run_tick();
                        tick(object);
                    })
                    .is_ok()
            });
        })
        .map(drop)
}

/// Sleeps `interval()`, then asks `queue` to run a tick, which it does by
/// calling the function it is given. A tick isn't queued while the last one
/// hasn't run, so a busy UI thread doesn't build up a backlog. Stops once
/// `queue` returns false.
fn run(interval: impl Fn() -> Duration, mut queue: impl FnMut(Box<dyn FnOnce() + Send>) -> bool) {
    let waiting = Arc::new(AtomicBool::new(false));
    loop {
        std::thread::sleep(interval());
        if waiting.swap(true, Ordering::AcqRel) {
            continue;
        }
        let ran = Arc::clone(&waiting);
        if !queue(Box::new(move || ran.store(false, Ordering::Release))) {
            return;
        }
    }
}

#[cfg(test)]
#[path = "tests/ticker_test.rs"]
mod tests;
