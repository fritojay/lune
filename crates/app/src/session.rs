//! State shared by the QML-facing objects. QML creates those objects without
//! constructor arguments, so what they share lives here: the catalog and
//! current view (UI thread only), and the queue (shared with the decoder
//! thread through [`EngineQueue`]). Objects that show it subscribe to its
//! [`Change`]s with [`on_change`].

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::SystemTime;

use cxx_qt::{CxxQtThread, Threading};
use lune_core::{
    BrowseState, Catalog, Library, MissingTrack, Queue, QueueItem, SlotId, View, WindowState,
};
use lune_engine::{EntryId, PlayQueue, QueueEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The library loaded or changed.
    Catalog,
    /// `reset` is false when only the library changed underneath (a
    /// rescan), so the list keeps its position.
    View {
        reset: bool,
    },
    Missing,
    /// Its contents, order, or shuffle. The current item is `PlayerController.queueIndex`.
    Queue,
    /// Playback of a track started.
    Played,
}

#[derive(Default)]
struct UiState {
    catalog: Option<Rc<Catalog>>,
    view: Rc<View>,
    missing: Rc<Vec<MissingTrack>>,
    browse: Option<BrowseState>,
    listeners: Vec<Rc<dyn Fn(Change)>>,
}

thread_local! {
    // Only the getters and setters below touch it, and none of them calls
    // into Qt while it is borrowed (Qt can call back into a model).
    static UI: RefCell<UiState> = RefCell::default();
}

pub fn catalog() -> Option<Rc<Catalog>> {
    UI.with_borrow(|ui| ui.catalog.clone())
}

pub fn set_catalog(catalog: Catalog) {
    UI.with_borrow_mut(|ui| ui.catalog = Some(Rc::new(catalog)));
    notify(Change::Catalog);
}

/// The tracks whose files are missing.
pub fn missing() -> Rc<Vec<MissingTrack>> {
    UI.with_borrow(|ui| Rc::clone(&ui.missing))
}

pub fn set_missing(tracks: Vec<MissingTrack>) {
    UI.with_borrow_mut(|ui| ui.missing = Rc::new(tracks));
    notify(Change::Missing);
}

/// The main pane's track list: what a double-click puts in the queue.
pub fn view() -> Rc<View> {
    UI.with_borrow(|ui| Rc::clone(&ui.view))
}

pub fn set_view(view: View, reset: bool) {
    UI.with_borrow_mut(|ui| ui.view = Rc::new(view));
    notify(Change::View { reset });
}

/// The window as the last run left it. `--bench` always starts from the
/// defaults, so its timings are comparable.
pub fn saved_window() -> &'static WindowState {
    static SAVED: OnceLock<WindowState> = OnceLock::new();
    SAVED.get_or_init(|| {
        let config = crate::config();
        if config.bench {
            return WindowState::default();
        }
        WindowState::load(&config.window_state_path).unwrap_or_else(|err| {
            tracing::warn!(%err, "the window opens with its default layout");
            WindowState::default()
        })
    })
}

/// What the main pane shows now; until the library loads, what it showed
/// in the last run.
pub fn browse() -> BrowseState {
    UI.with_borrow(|ui| ui.browse.clone())
        .unwrap_or_else(|| saved_window().browse.clone())
}

pub fn set_browse(browse: BrowseState) {
    UI.with_borrow_mut(|ui| ui.browse = Some(browse));
}

/// Playback of track `track_id` started: Recently Played changes now, and
/// the DB is updated in the background.
pub fn record_play(track_id: i64) {
    let at = SystemTime::now();
    UI.with_borrow_mut(|ui| {
        if let Some(catalog) = &mut ui.catalog {
            Rc::make_mut(catalog).record_play(track_id, at);
        }
    });
    save(Save::Play(track_id, at));
    notify(Change::Played);
}

pub fn queue() -> &'static Arc<Mutex<Queue>> {
    static QUEUE: OnceLock<Arc<Mutex<Queue>>> = OnceLock::new();
    QUEUE.get_or_init(Arc::default)
}

pub fn lock_queue() -> MutexGuard<'static, Queue> {
    // Every edit leaves the queue consistent before it can panic.
    queue().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Call after editing the queue through [`lock_queue`].
pub fn queue_edited() {
    notify(Change::Queue);
}

pub fn subscribe(listener: impl Fn(Change) + 'static) {
    UI.with_borrow_mut(|ui| ui.listeners.push(Rc::new(listener)));
}

/// Calls `changed` on `qt_thread`'s object with each change. It is queued, so
/// it never runs inside another object's update.
pub fn on_change<T: Threading + 'static>(
    qt_thread: CxxQtThread<T>,
    changed: fn(Pin<&mut T>, Change),
) {
    subscribe(move |change| {
        // An error means the object is gone (the app is quitting).
        let _ = qt_thread.queue(move |object| changed(object, change));
    });
}

fn notify(change: Change) {
    // Cloned so a listener can use the state, or subscribe, while it runs.
    let listeners = UI.with_borrow(|ui| ui.listeners.clone());
    for listener in listeners {
        listener(change);
    }
}

/// The engine's view of the queue.
pub struct EngineQueue;

pub fn entry(item: &QueueItem) -> QueueEntry {
    QueueEntry::new(EntryId(item.slot.0), item.path.clone())
}

impl PlayQueue for EngineQueue {
    fn next_after(&self, id: EntryId) -> Option<QueueEntry> {
        lock_queue().next_after(SlotId(id.0)).map(entry)
    }

    fn previous_before(&self, id: EntryId) -> Option<QueueEntry> {
        lock_queue().previous_before(SlotId(id.0)).map(entry)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Save {
    /// The queue's contents changed.
    Queue,
    /// Only the current item or shuffle changed.
    Cursor,
    /// Playback of a track (by id) started.
    Play(i64, SystemTime),
}

struct Saver {
    requests: Sender<Save>,
    thread: JoinHandle<()>,
}

fn saver() -> &'static Mutex<Option<Saver>> {
    static SAVER: OnceLock<Mutex<Option<Saver>>> = OnceLock::new();
    SAVER.get_or_init(|| {
        let db_path = crate::config().db_path.clone();
        let (requests, rx) = mpsc::channel::<Save>();
        let spawned = std::thread::Builder::new()
            .name("lune-queue-saver".into())
            .spawn(move || {
                let mut library = match Library::open(&db_path) {
                    Ok(library) => library,
                    Err(err) => {
                        tracing::error!(%err, "the queue will not be saved");
                        return;
                    }
                };
                while let Ok(first) = rx.recv() {
                    let batch: Vec<Save> = std::iter::once(first).chain(rx.try_iter()).collect();
                    for save in &batch {
                        if let Save::Play(track_id, at) = *save
                            && let Err(err) = library.record_play(track_id, at)
                        {
                            tracing::error!(%err, "could not record a play");
                        }
                    }
                    let result = if batch.contains(&Save::Queue) {
                        // Saving 100k rows takes a while; don't hold the
                        // lock the decoder needs meanwhile.
                        let snapshot = lock_queue().clone();
                        library.save_queue(&snapshot)
                    } else if batch.contains(&Save::Cursor) {
                        library.save_queue_cursor(&lock_queue())
                    } else {
                        Ok(())
                    };
                    if let Err(err) = result {
                        tracing::error!(%err, "could not save the queue");
                    }
                }
            });
        let saver = spawned
            .map_err(|err| tracing::error!(%err, "could not start the queue saver"))
            .ok()
            .map(|thread| Saver { requests, thread });
        Mutex::new(saver)
    })
}

/// Saves the queue or a play on a background thread; bursts of queue saves
/// are coalesced into one.
pub fn save(what: Save) {
    let saver = saver().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(saver) = saver.as_ref() {
        let _ = saver.requests.send(what);
    }
}

/// Waits for pending saves; call before exiting.
pub fn finish_saving() {
    let saver = saver()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    if let Some(Saver { requests, thread }) = saver {
        drop(requests);
        let _ = thread.join();
    }
}

#[cfg(test)]
#[path = "tests/session_test.rs"]
mod tests;
