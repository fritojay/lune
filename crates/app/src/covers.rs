//! Cover thumbnails for the UI. [`Covers::path`] never blocks: an unknown
//! cover is queued for background workers, and subscribers are told when it
//! is ready. The most recent requests are served first, so the covers in
//! view after a fast scroll appear before ones scrolled past.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};

use cxx_qt::casting::Upcast;
use cxx_qt::{CxxQtThread, Threading};
use cxx_qt_lib::{QString, QUrl, QVariant};
use lune_core::Library;

use crate::list_model::{self, QAbstractListModel};

/// Longest side, in pixels: album covers are 160 logical pixels, and this
/// keeps them sharp on 2x displays. The now-playing cover can be larger, so
/// it is slightly soft there.
pub const THUMBNAIL_SIZE: u32 = 320;
const WORKERS: usize = 2;
/// Requests kept waiting. Older ones are dropped: after a fast scroll they
/// are covers no longer in view, and are asked for again if they come back.
const MAX_PENDING: usize = 256;

type Listener = Arc<dyn Fn(i64) + Send + Sync>;

pub struct Covers {
    state: Mutex<State>,
    wake: Condvar,
}

#[derive(Default)]
struct State {
    /// `None`: the cover couldn't be made, so it isn't retried this session.
    done: HashMap<i64, Option<PathBuf>>,
    /// Made before the library last changed, so made again on the next
    /// request; meanwhile it is still shown, so covers don't blink.
    stale: HashSet<i64>,
    queued: HashSet<i64>,
    pending: VecDeque<i64>,
    listeners: Vec<Listener>,
}

impl Covers {
    /// Starts `workers` threads; each calls `make` to produce a thumbnail.
    pub fn start<M>(workers: usize, make: impl Fn() -> M + Send + Sync + 'static) -> Arc<Self>
    where
        M: FnMut(i64) -> Option<PathBuf>,
    {
        let covers = Arc::new(Self {
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
        });
        let make = Arc::new(make);
        for n in 0..workers {
            let covers = Arc::clone(&covers);
            let make = Arc::clone(&make);
            let spawned = std::thread::Builder::new()
                .name(format!("lune-covers-{n}"))
                .spawn(move || covers.work(make()));
            if let Err(err) = spawned {
                tracing::error!(%err, "could not start a cover worker");
            }
        }
        covers
    }

    /// The thumbnail of `art_id` if it is ready; otherwise queues it.
    pub fn path(&self, art_id: i64) -> Option<PathBuf> {
        let mut state = self.lock();
        let done = state.done.get(&art_id).cloned();
        if done.is_none() || state.stale.remove(&art_id) {
            self.queue(&mut state, art_id);
        }
        done.flatten()
    }

    fn queue(&self, state: &mut State, art_id: i64) {
        if state.queued.insert(art_id) {
            state.pending.push_back(art_id);
            if state.pending.len() > MAX_PENDING
                && let Some(oldest) = state.pending.pop_front()
            {
                state.queued.remove(&oldest);
                // A stale cover dropped unmade is still stale.
                if state.done.contains_key(&oldest) {
                    state.stale.insert(oldest);
                }
            }
            self.wake.notify_one();
        }
    }

    /// After a rescan a cover id may name a different image, so each cover
    /// is made again when next asked for. Failures are retried at once.
    pub fn forget(&self) {
        let mut state = self.lock();
        state.done.retain(|_, path| path.is_some());
        state.stale = state.done.keys().copied().collect();
    }

    /// `listener` is called from a worker thread with each finished cover id.
    pub fn subscribe(&self, listener: impl Fn(i64) + Send + Sync + 'static) {
        self.lock().listeners.push(Arc::new(listener));
    }

    fn work(&self, mut make: impl FnMut(i64) -> Option<PathBuf>) {
        loop {
            let art_id = {
                let mut state = self.lock();
                loop {
                    if let Some(id) = state.pending.pop_back() {
                        break id;
                    }
                    state = self
                        .wake
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            };
            let path = make(art_id);
            // Called without the lock held, so a listener may call `path`.
            let listeners = {
                let mut state = self.lock();
                state.queued.remove(&art_id);
                state.done.insert(art_id, path);
                state.listeners.clone()
            };
            for listener in &listeners {
                listener(art_id);
            }
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // Plain maps; a panic elsewhere can't leave them inconsistent.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The rows of a model showing each cover, so a finished cover updates only
/// those.
pub type CoverRows = HashMap<i64, Vec<i32>>;

/// For each cover in `covers` (one entry per model row), the rows showing it.
pub fn rows_by_cover(covers: impl IntoIterator<Item = Option<i64>>) -> CoverRows {
    let mut rows: HashMap<i64, Vec<i32>> = HashMap::new();
    for (row, art_id) in covers.into_iter().enumerate() {
        if let (Some(art_id), Ok(row)) = (art_id, i32::try_from(row)) {
            rows.entry(art_id).or_default().push(row);
        }
    }
    rows
}

/// Calls `ready` on `qt_thread`'s object with each finished cover id.
pub fn on_ready<T: Threading + 'static>(qt_thread: CxxQtThread<T>, ready: fn(Pin<&mut T>, i64)) {
    covers().subscribe(move |art_id| {
        // An error means the object is gone (the app is quitting).
        let _ = qt_thread.queue(move |object| ready(object, art_id));
    });
}

/// Updates `role` of the rows `rows` gives for each finished cover.
pub fn update_rows_on_ready<T>(qt_thread: CxxQtThread<T>, rows: fn(&T) -> &CoverRows, role: i32)
where
    T: Threading + Upcast<QAbstractListModel> + 'static,
{
    covers().subscribe(move |art_id| {
        // An error means the object is gone (the app is quitting).
        let _ = qt_thread.queue(move |model| {
            if let Some(changed) = rows(&model).get(&art_id).cloned() {
                list_model::role_changed(model.upcast_pin(), &changed, role);
            }
        });
    });
}

/// A model's cover role: the thumbnail's URL, or empty until it is ready.
pub fn url(art_id: Option<i64>) -> QVariant {
    art_id
        .and_then(|id| covers().path(id))
        .and_then(|path| file_url(&path))
        .map_or_else(QVariant::default, |url| QVariant::from(&url))
}

/// `None` for a path that isn't valid UTF-8 (only possible in an unusual
/// cache directory): the cover is then shown as missing.
pub fn file_url(path: &Path) -> Option<QUrl> {
    // Not a plain `file` scheme and path: a Windows path needs the leading
    // `/` and forward slashes that `fromLocalFile` adds.
    Some(QUrl::from_local_file(&QString::from(path.to_str()?)))
}

/// The app's cover cache, reading the library at the configured path.
pub fn covers() -> &'static Arc<Covers> {
    static COVERS: OnceLock<Arc<Covers>> = OnceLock::new();
    COVERS.get_or_init(|| {
        let db_path = crate::config().db_path.clone();
        let dir = lune_core::default_thumbnail_dir();
        Covers::start(WORKERS, move || {
            let library = Library::open(&db_path)
                .map_err(|err| tracing::error!(%err, "cover worker could not open the library"))
                .ok();
            let dir = dir
                .as_ref()
                .map_err(|err| tracing::error!(%err, "no thumbnail cache directory"))
                .ok()
                .cloned();
            move |art_id| {
                let (library, dir) = (library.as_ref()?, dir.as_ref()?);
                library
                    .thumbnail(art_id, dir, THUMBNAIL_SIZE)
                    .map_err(|err| tracing::warn!(%err, art_id, "no thumbnail"))
                    .ok()
                    .flatten()
            }
        })
    })
}

#[cfg(test)]
#[path = "tests/covers_test.rs"]
mod tests;
