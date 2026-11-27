//! State shared by the QML-facing objects. QML creates those objects without
//! constructor arguments, so what they share lives here: the catalog and
//! current view (UI thread only), and the queue (shared with the decoder
//! thread through [`EngineQueue`]). Objects that show it subscribe to its
//! [`Change`]s with [`on_change`].

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::SystemTime;

use cxx_qt::{CxxQtThread, Threading};
use lune_core::{
    BrowseState, Catalog, Library, ListId, MissingTrack, Playlist, Queue, QueueItem, SlotId,
    SortColumn, View, WindowState,
};
use lune_engine::{EntryId, PlayQueue, QueueEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// The library loaded or changed.
    Catalog,
    /// [`view`] shows something else, from its start.
    View,
    /// [`view`] is the same selection with the library changed underneath
    /// (a rescan), so the list keeps its position.
    ViewRefreshed,
    Missing,
    Playlists,
    /// Its contents, order, or shuffle. The current item is `PlayerController.queueIndex`.
    Queue,
    /// Playback of a track started.
    Played,
    /// [`browse`] changed.
    Browse,
    /// [`hidden_columns`] changed.
    Columns,
    /// The user chose another audio output.
    OutputDevice,
    /// [`has_folders`] changed.
    Folders,
    /// [`now_playing`] changed.
    NowPlaying,
}

#[derive(Default)]
struct UiState {
    catalog: Option<Rc<Catalog>>,
    view: Rc<View>,
    missing: Rc<Vec<MissingTrack>>,
    playlists: Rc<Vec<Playlist>>,
    /// For the next new playlist; `None` until the playlists have loaded.
    unused_playlist_id: Option<ListId>,
    browse: Option<BrowseState>,
    hidden_columns: Option<BTreeSet<SortColumn>>,
    /// The level (0.0 to 1.0) and whether it is muted.
    volume: Option<(f64, bool)>,
    has_folders: bool,
    now_playing: Option<i64>,
    listeners: Vec<(Change, Rc<dyn Fn()>)>,
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

pub fn playlists() -> Rc<Vec<Playlist>> {
    UI.with_borrow(|ui| Rc::clone(&ui.playlists))
}

/// The playlists as stored, and an id no stored list has. Until this is
/// called, the playlists can't be edited: a save would replace the stored
/// ones with none.
pub fn load_playlists(playlists: Vec<Playlist>, unused_id: ListId) {
    UI.with_borrow_mut(|ui| ui.unused_playlist_id = Some(unused_id));
    set_playlists(playlists);
}

pub fn playlists_loaded() -> bool {
    UI.with_borrow(|ui| ui.unused_playlist_id.is_some())
}

fn set_playlists(playlists: Vec<Playlist>) {
    UI.with_borrow_mut(|ui| ui.playlists = Rc::new(playlists));
    notify(Change::Playlists);
}

/// Changes the playlists: the change shows now, and the DB is updated in
/// the background. Does nothing before they have loaded.
pub fn edit_playlists(edit: impl FnOnce(&mut Vec<Playlist>)) {
    if !playlists_loaded() {
        tracing::warn!("a playlist edit before the playlists loaded is dropped");
        return;
    }
    let mut playlists = Vec::clone(&playlists());
    edit(&mut playlists);
    save(Save::Playlists(playlists.clone()));
    set_playlists(playlists);
}

/// Makes a playlist and returns its id, or `None` before the playlists have
/// loaded.
pub fn create_playlist(name: String, track_ids: Vec<i64>) -> Option<ListId> {
    let id = UI.with_borrow_mut(|ui| {
        let id = ui.unused_playlist_id?;
        ui.unused_playlist_id = Some(id.next());
        Some(id)
    })?;
    edit_playlists(|playlists| {
        playlists.push(Playlist {
            id,
            name,
            track_ids,
        });
    });
    Some(id)
}

/// The main pane's track list: what a double-click puts in the queue.
pub fn view() -> Rc<View> {
    UI.with_borrow(|ui| Rc::clone(&ui.view))
}

pub fn set_view(view: View, reset: bool) {
    UI.with_borrow_mut(|ui| ui.view = Rc::new(view));
    notify(if reset {
        Change::View
    } else {
        Change::ViewRefreshed
    });
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
    let changed = UI.with_borrow_mut(|ui| ui.browse.replace(browse.clone()) != Some(browse));
    if changed {
        notify(Change::Browse);
    }
}

/// The main pane's hidden columns; until changed, as the last run left them.
pub fn hidden_columns() -> BTreeSet<SortColumn> {
    UI.with_borrow(|ui| ui.hidden_columns.clone())
        .unwrap_or_else(|| saved_window().hidden_columns.clone())
}

pub fn set_hidden_columns(hidden: BTreeSet<SortColumn>) {
    let changed =
        UI.with_borrow_mut(|ui| ui.hidden_columns.replace(hidden.clone()) != Some(hidden));
    if changed {
        notify(Change::Columns);
    }
}

/// The player's volume and mute; until changed, as the last run left them.
pub fn volume() -> (f64, bool) {
    UI.with_borrow(|ui| ui.volume).unwrap_or_else(|| {
        let saved = saved_window();
        let level = f64::from(saved.volume_percent) / f64::from(lune_core::MAX_VOLUME_PERCENT);
        (level, saved.muted)
    })
}

pub fn set_volume(level: f64, muted: bool) {
    UI.with_borrow_mut(|ui| ui.volume = Some((level, muted)));
}

/// Whether the library has any folders, as the library thread last said.
pub fn has_folders() -> bool {
    UI.with_borrow(|ui| ui.has_folders)
}

pub fn set_has_folders(has_folders: bool) {
    let changed =
        UI.with_borrow_mut(|ui| std::mem::replace(&mut ui.has_folders, has_folders) != has_folders);
    if changed {
        notify(Change::Folders);
    }
}

/// The track id of the queue's current track, which the views mark.
pub fn now_playing() -> Option<i64> {
    UI.with_borrow(|ui| ui.now_playing)
}

pub fn set_now_playing(track_id: Option<i64>) {
    let changed =
        UI.with_borrow_mut(|ui| std::mem::replace(&mut ui.now_playing, track_id) != track_id);
    if changed {
        notify(Change::NowPlaying);
    }
}

/// Playback of track `track_id` started: Recently Played changes now, and
/// the DB is updated in the background.
pub fn record_play(track_id: i64) {
    let at = SystemTime::now();
    if let Some(catalog) = catalog() {
        catalog.record_play(track_id, at);
    }
    save(Save::Play(track_id, at));
    notify(Change::Played);
}

pub fn queue() -> &'static Arc<Mutex<Queue>> {
    static QUEUE: OnceLock<Arc<Mutex<Queue>>> = OnceLock::new();
    QUEUE.get_or_init(Arc::default)
}

pub fn lock_queue() -> MutexGuard<'static, Queue> {
    lock(queue())
}

fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    // Every edit leaves the queue consistent before it can panic.
    queue.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn output_device_changed() {
    notify(Change::OutputDevice);
}

/// Call after editing the queue through [`lock_queue`].
pub fn queue_edited() {
    notify(Change::Queue);
}

pub fn subscribe(change: Change, listener: impl Fn() + 'static) {
    UI.with_borrow_mut(|ui| ui.listeners.push((change, Rc::new(listener))));
}

/// Calls `changed` on `qt_thread`'s object with each `change`. It is
/// queued, so it never runs inside another object's update.
pub fn on_change<T: Threading + 'static>(
    qt_thread: CxxQtThread<T>,
    change: Change,
    changed: fn(Pin<&mut T>),
) {
    subscribe(change, move || {
        // An error means the object is gone (the app is quitting).
        let _ = qt_thread.queue(changed);
    });
}

fn notify(change: Change) {
    // Cloned so a listener can use the state, or subscribe, while it runs.
    let listeners: Vec<_> = UI.with_borrow(|ui| {
        ui.listeners
            .iter()
            .filter(|(heard, _)| *heard == change)
            .map(|(_, listener)| Rc::clone(listener))
            .collect()
    });
    for listener in listeners {
        listener();
    }
}

type ErrorListener = Box<dyn Fn(&str) + Send>;

fn error_listeners() -> &'static Mutex<Vec<ErrorListener>> {
    static LISTENERS: OnceLock<Mutex<Vec<ErrorListener>>> = OnceLock::new();
    LISTENERS.get_or_init(Mutex::default)
}

/// Logs `message` and shows it to the user; call from any thread.
pub fn report_error(message: &str) {
    tracing::error!("{message}");
    let listeners = error_listeners()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    for listener in listeners.iter() {
        listener(message);
    }
}

/// `listener` is called, on the thread that reports, with each error.
pub fn subscribe_errors(listener: impl Fn(&str) + Send + 'static) {
    error_listeners()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Box::new(listener));
}

/// The engine's view of the queue.
pub struct EngineQueue;

pub fn entry(item: &QueueItem) -> QueueEntry {
    QueueEntry::new(EntryId(item.slot.0), item.path.clone())
}

impl PlayQueue for EngineQueue {
    fn next_after(&self, id: EntryId) -> Option<QueueEntry> {
        lock_queue().follow_after(SlotId(id.0)).map(entry)
    }

    fn previous_before(&self, id: EntryId) -> Option<QueueEntry> {
        lock_queue().skip_before(SlotId(id.0)).map(entry)
    }

    fn skip_after(&self, id: EntryId) -> Option<QueueEntry> {
        lock_queue().skip_after(SlotId(id.0)).map(entry)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Save {
    /// The queue's contents changed.
    Queue,
    /// Only the current item, shuffle, or repeat changed.
    Cursor,
    /// Playback moved to another item: a cursor save that the next track
    /// change repeats.
    TrackChanged,
    /// Playback of a track (by id) started.
    Play(i64, SystemTime),
    /// All of them, as they are now.
    Playlists(Vec<Playlist>),
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
                        report_error(&format!("The queue and playlists can't be saved: {err}"));
                        return;
                    }
                };
                // What is stored, so a save rewrites only what changed.
                let mut saved_playlists = library.playlists().unwrap_or_else(|err| {
                    tracing::warn!(%err, "the first playlist save rewrites them all");
                    Vec::new()
                });
                let mut failures = QueueSaveFailures::default();
                while let Ok(first) = rx.recv() {
                    let batch: Vec<Save> = std::iter::once(first).chain(rx.try_iter()).collect();
                    for save in &batch {
                        if let Save::Play(track_id, at) = *save
                            && let Err(err) = library.record_play(track_id, at)
                        {
                            tracing::error!(%err, "could not record a play");
                        }
                    }
                    let playlists = batch.iter().rev().find_map(|save| match save {
                        Save::Playlists(playlists) => Some(playlists),
                        _ => None,
                    });
                    if let Some(playlists) = playlists {
                        match library.save_playlists(&saved_playlists, playlists) {
                            Ok(()) => saved_playlists = playlists.clone(),
                            Err(err) => {
                                report_error(&format!("The playlists could not be saved: {err}"));
                            }
                        }
                    }
                    // The most a save in the batch asks for covers the others.
                    let Some(save) = [Save::Queue, Save::Cursor, Save::TrackChanged]
                        .into_iter()
                        .find(|save| batch.contains(save))
                    else {
                        continue;
                    };
                    let result = if save == Save::Queue {
                        save_from_queue(queue(), Queue::clone, |snapshot| {
                            library.save_queue(snapshot)
                        })
                    } else {
                        save_from_queue(queue(), Queue::cursor_state, |cursor| {
                            library.save_queue_cursor(cursor)
                        })
                    };
                    match result {
                        Ok(()) => failures.saved(),
                        Err(err) if failures.is_quiet(&save, err.is_busy()) => {
                            tracing::warn!(%err, "could not save the queue position");
                        }
                        Err(err) => report_error(&format!("The queue could not be saved: {err}")),
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

/// Which failed queue saves are only logged: a busy [`Save::TrackChanged`]
/// is repeated at the next track change, so only a second one in a row is
/// shown. Any other failure is shown, such as the save on quitting, which
/// nothing repeats.
#[derive(Default)]
struct QueueSaveFailures {
    failed_before: bool,
}

impl QueueSaveFailures {
    fn saved(&mut self) {
        self.failed_before = false;
    }

    fn is_quiet(&mut self, save: &Save, busy: bool) -> bool {
        let quiet = *save == Save::TrackChanged && busy && !self.failed_before;
        self.failed_before = true;
        quiet
    }
}

/// Copies what `write` needs out of the queue, then writes it with the lock
/// released: the database can be busy for seconds (a scan), and the decoder
/// and the UI need the lock meanwhile.
fn save_from_queue<T>(
    queue: &Mutex<Queue>,
    copy: impl FnOnce(&Queue) -> T,
    write: impl FnOnce(&T) -> lune_core::Result<()>,
) -> lune_core::Result<()> {
    // A statement of its own: a temporary guard lives to the end of its statement.
    let copied = copy(&lock(queue));
    write(&copied)
}

/// Saves the queue, a play, or the playlists on a background thread; bursts
/// of queue or playlist saves are coalesced into one.
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
