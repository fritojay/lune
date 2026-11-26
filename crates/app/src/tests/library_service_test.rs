use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use lune_core::Settings;

use super::*;
use crate::preferences::Store;
use crate::test_util::{TempDir, wav_bytes};

/// Longer than `WATCH_DELAY` plus a scan, with room for a slow CI machine.
const WAIT: Duration = Duration::from_secs(20);

struct Harness {
    dir: TempDir,
    service: LibraryService,
    updates: Receiver<Update>,
}

impl Harness {
    fn db(dir: &TempDir) -> PathBuf {
        dir.path().join("data/library.sqlite3")
    }

    fn config(dir: &TempDir) -> PathBuf {
        dir.path().join("config/config.toml")
    }

    fn thumbnails(dir: &TempDir) -> PathBuf {
        dir.path().join("cache/thumbnails")
    }

    fn start(dir: TempDir) -> Self {
        Self::start_with_clock(dir, &FakeClock::default())
    }

    fn start_with_clock(dir: TempDir, clock: &FakeClock) -> Self {
        let (tx, updates) = mpsc::channel();
        let clock = clock.clone();
        let service = LibraryService::start(
            Self::db(&dir),
            Store::load(Self::config(&dir)).startup_folders(),
            Some(Self::thumbnails(&dir)),
            RESCAN_EVERY,
            move || clock.now(),
            move |u| {
                let _ = tx.send(u);
            },
        );
        Self {
            dir,
            service,
            updates,
        }
    }

    /// The first update `pick` accepts, skipping the others.
    fn wait_for<T>(&self, mut pick: impl FnMut(Update) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.updates.recv_timeout(left) {
                Ok(update) => {
                    if let Some(found) = pick(update) {
                        return found;
                    }
                }
                Err(RecvTimeoutError::Timeout) => panic!("no matching update within {WAIT:?}"),
                Err(RecvTimeoutError::Disconnected) => panic!("the library thread stopped"),
            }
        }
    }

    fn idle(&self) {
        self.wait_for(|u| matches!(u, Update::Status(None)).then_some(()));
    }

    fn library_size(&self) -> usize {
        self.wait_for(|u| match u {
            Update::Library(catalog) => Some(catalog.len()),
            _ => None,
        })
    }

    fn loaded_size(&self) -> usize {
        self.wait_for(|u| match u {
            Update::Loaded(catalog, _, _) => Some(catalog.len()),
            _ => None,
        })
    }
}

/// A wall clock that only moves when told to, so periodic rescans happen
/// exactly when a test wants them.
#[derive(Clone)]
struct FakeClock(Arc<Mutex<SystemTime>>);

impl Default for FakeClock {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(SystemTime::UNIX_EPOCH)))
    }
}

impl FakeClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }

    fn advance(&self, by: Duration) {
        *self.0.lock().unwrap() += by;
    }
}

/// A library DB holding the tracks of `folders` (scanned directly, as
/// lune-cli would).
fn prescanned(dir: &TempDir, folders: &[&str]) {
    let mut library = Library::open(&Harness::db(dir)).unwrap();
    for folder in folders {
        library
            .scan(&dir.path().join(folder), ScanMode::Thorough, |_| {})
            .unwrap();
    }
}

fn two_folders() -> TempDir {
    let dir = TempDir::new();
    dir.wav("a/Artist/1.wav");
    dir.wav("a/Artist/2.wav");
    dir.wav("b/1.wav");
    dir
}

#[test]
fn the_saved_playlists_arrive_with_the_library() {
    let dir = two_folders();
    prescanned(&dir, &["a"]);
    let saved = Library::open(&Harness::db(&dir))
        .unwrap()
        .playlists()
        .unwrap();
    let h = Harness::start(dir);
    let loaded = h.wait_for(|u| match u {
        Update::Loaded(_, _, playlists) => Some(playlists),
        _ => None,
    });
    assert_eq!(loaded.map(|p| p.playlists), Some(saved));
}

#[test]
fn without_a_settings_file_a_library_built_elsewhere_is_kept() {
    let dir = two_folders();
    prescanned(&dir, &["a", "b"]);
    let h = Harness::start(dir);
    assert_eq!(h.loaded_size(), 3);
    assert!(h.folders().is_empty());
    h.wait_for(|u| match u {
        Update::Library(_) => panic!("nothing should have been dropped"),
        _ => matches!(u, Update::Status(None)).then_some(()),
    });
}

#[test]
fn a_library_that_cannot_be_opened_names_its_file() {
    let dir = TempDir::new();
    let db = Harness::db(&dir);
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    dir.write("data/library.sqlite3", b"not a database, just some text");
    let harness = Harness::start(dir);
    let message = harness.wait_for(|u| match u {
        Update::Error(message) => Some(message),
        _ => None,
    });
    assert!(message.contains(&db.display().to_string()), "{message}");
}

#[test]
fn an_empty_new_library_loads_empty() {
    let h = Harness::start(TempDir::new());
    assert_eq!(h.loaded_size(), 0);
    h.idle();
}

#[test]
fn adding_a_folder_lists_it_and_scans_it() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let h = Harness::start(dir);
    h.idle();
    h.service.send(Job::AddFolder(a.clone()));
    assert_eq!(h.folders(), [(a, None)]);
    assert_eq!(h.library_size(), 2);
}

#[test]
fn adding_a_folder_twice_changes_nothing_the_second_time() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let h = Harness::start(dir);
    h.idle();
    h.service.send(Job::AddFolder(a.clone()));
    assert_eq!(h.library_size(), 2);
    h.idle();
    h.service.send(Job::AddFolder(a.join("Artist")));
    h.wait_for(|u| match u {
        Update::Library(_) => panic!("nothing should have changed"),
        Update::Status(None) => Some(()),
        _ => None,
    });
}

#[test]
fn removing_a_folder_drops_its_tracks() {
    let dir = two_folders();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    let h = Harness::start(dir);
    h.idle();
    // One at a time: sent together, they may or may not land in one batch.
    h.service.send(Job::AddFolder(a.clone()));
    assert_eq!(h.library_size(), 2);
    h.idle();
    h.service.send(Job::AddFolder(b.clone()));
    assert_eq!(h.library_size(), 3);
    h.idle();
    h.service.send(Job::RemoveFolder(a));
    let folders: Vec<PathBuf> = h.folders().into_iter().map(|(path, _)| path).collect();
    assert_eq!(folders, [b]);
    assert_eq!(h.library_size(), 1);
}

#[test]
fn startup_drops_tracks_outside_the_configured_folders() {
    let dir = two_folders();
    prescanned(&dir, &["a", "b"]);
    Settings {
        library_folders: vec![dir.path().join("a")],
        ..Settings::default()
    }
    .save(&Harness::config(&dir))
    .unwrap();
    let h = Harness::start(dir);
    assert_eq!(h.loaded_size(), 3, "the stored library shows first");
    assert_eq!(h.library_size(), 2);
}

#[test]
fn startup_picks_up_changes_made_while_the_app_was_closed() {
    let dir = two_folders();
    prescanned(&dir, &["a"]);
    Settings {
        library_folders: vec![dir.path().join("a")],
        ..Settings::default()
    }
    .save(&Harness::config(&dir))
    .unwrap();
    dir.wav("a/Artist/3.wav");
    let h = Harness::start(dir);
    assert_eq!(h.loaded_size(), 2);
    assert_eq!(h.library_size(), 3);
}

#[test]
fn a_broken_settings_file_drops_nothing() {
    let dir = two_folders();
    prescanned(&dir, &["a", "b"]);
    dir.write("config/config.toml", b"library_folders = [");
    let h = Harness::start(dir);
    assert_eq!(h.loaded_size(), 3);
    h.wait_for(|u| match u {
        Update::Library(_) => panic!("nothing should have been dropped"),
        _ => matches!(u, Update::Status(None)).then_some(()),
    });
}

#[test]
fn files_added_to_and_removed_from_a_watched_folder_show_up() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let h = Harness::start(dir);
    h.idle();
    h.service.send(Job::AddFolder(a));
    assert_eq!(h.library_size(), 2);
    h.idle();

    h.dir.wav("a/Other/new.wav");
    assert_eq!(h.library_size(), 3);
    h.idle();

    h.dir.remove_dir("a/Artist");
    assert_eq!(h.library_size(), 1);
    let missing = h.wait_for(|u| match u {
        Update::Missing(tracks) => Some(tracks),
        _ => None,
    });
    assert_eq!(missing.len(), 2);
}

/// Sends a job that does nothing, so the thread checks the clock.
fn wake(h: &Harness) {
    h.service.send(Job::Rescan(Vec::new()));
    h.idle();
}

fn is_scanning(u: &Update) -> bool {
    matches!(u, Update::Status(Some(s)) if s.starts_with("Scanning"))
}

#[test]
fn the_folders_are_rescanned_once_the_rescan_is_due() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let clock = FakeClock::default();
    let h = Harness::start_with_clock(dir, &clock);
    h.idle();
    h.service.send(Job::AddFolder(a));
    h.idle();

    clock.advance(RESCAN_EVERY - Duration::from_secs(1));
    wake(&h);
    h.service.send(Job::ForgetAllMissing);
    h.wait_for(|u| match u {
        u if is_scanning(&u) => panic!("rescanned early"),
        Update::Status(None) => Some(()),
        _ => None,
    });

    clock.advance(Duration::from_secs(1));
    // The thread now rechecks the clock every second, so the rescan may start
    // before this job arrives; waiting for idle here could skip it.
    h.service.send(Job::Rescan(Vec::new()));
    h.wait_for(|u| is_scanning(&u).then_some(()));
    h.idle();
}

#[test]
fn a_rescan_all_postpones_the_periodic_rescan() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let clock = FakeClock::default();
    let h = Harness::start_with_clock(dir, &clock);
    h.idle();
    h.service.send(Job::AddFolder(a));
    h.idle();

    clock.advance(RESCAN_EVERY / 2);
    h.service.send(Job::RescanAll);
    h.idle();
    clock.advance(RESCAN_EVERY / 2);
    wake(&h);
    h.service.send(Job::ForgetAllMissing);
    h.wait_for(|u| match u {
        u if is_scanning(&u) => panic!("the rescan all should have reset the timer"),
        Update::Status(None) => Some(()),
        _ => None,
    });
}

#[test]
fn the_rescan_wait_is_capped_and_ends_at_the_due_time() {
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let second = Duration::from_secs(1);
    assert_eq!(rescan_wait(t + second, t), Some(second));
    assert_eq!(rescan_wait(t + 3 * MAX_WAIT, t), Some(MAX_WAIT));
    assert_eq!(rescan_wait(t, t), None);
    assert_eq!(
        rescan_wait(t, t + second),
        None,
        "overdue, e.g. after sleep"
    );
}

#[test]
fn startup_rereads_a_file_rewritten_in_place_while_the_app_was_closed() {
    let dir = two_folders();
    dir.age("a");
    dir.age("a/Artist");
    prescanned(&dir, &["a"]);
    Settings {
        library_folders: vec![dir.path().join("a")],
        ..Settings::default()
    }
    .save(&Harness::config(&dir))
    .unwrap();
    // Rewriting an existing file leaves its directory's mtime alone.
    dir.write("a/Artist/1.wav", &wav_bytes(44_100));
    let h = Harness::start(dir);
    h.loaded_size();
    h.wait_for(|u| match u {
        Update::Library(_) => Some(()),
        Update::Status(None) => panic!("the rewritten file was not read again"),
        _ => None,
    });
    let durations: Vec<u64> = Library::open(&Harness::db(&h.dir))
        .unwrap()
        .track_list()
        .unwrap()
        .iter()
        .map(|t| t.duration_ms)
        .collect();
    assert!(durations.contains(&1000), "{durations:?}");
}

fn missing_ids(h: &Harness) -> Vec<i64> {
    h.wait_for(|u| match u {
        Update::Missing(tracks) => Some(tracks.iter().map(|t| t.id).collect()),
        _ => None,
    })
}

#[test]
fn unavailable_tracks_can_be_forgotten_one_at_a_time_or_all_at_once() {
    let dir = two_folders();
    dir.wav("a/3.wav");
    let a = dir.path().join("a");
    let h = Harness::start(dir);
    h.idle();
    h.service.send(Job::AddFolder(a));
    h.idle();
    h.dir.remove_dir("a/Artist");
    h.service.send(Job::Rescan(vec![h.dir.path().join("a")]));
    let missing = missing_ids(&h);
    assert_eq!(missing.len(), 2);
    h.idle();

    let present = Library::open(&Harness::db(&h.dir))
        .unwrap()
        .track_list()
        .unwrap()[0]
        .id;
    h.service
        .send(Job::ForgetMissing(vec![missing[0], present]));
    assert_eq!(missing_ids(&h), missing[1..]);
    h.idle();

    h.service.send(Job::ForgetAllMissing);
    assert!(missing_ids(&h).is_empty());
    h.idle();
    assert_eq!(
        Library::open(&Harness::db(&h.dir))
            .unwrap()
            .track_list()
            .unwrap()
            .len(),
        1,
        "the present track stays"
    );
}

#[test]
fn the_missing_tracks_are_sent_after_loading() {
    let h = Harness::start(TempDir::new());
    h.loaded_size();
    let missing = h.wait_for(|u| match u {
        Update::Missing(tracks) => Some(tracks),
        _ => None,
    });
    assert!(missing.is_empty());
}

#[test]
fn the_nearest_existing_directory_inside_a_folder_is_rescanned() {
    let dir = TempDir::new();
    dir.wav("music/Artist/1.wav");
    let music = dir.path().join("music");
    let folders = [music.clone()];
    assert_eq!(
        existing_ancestor(&music.join("Artist"), &folders),
        Some(music.join("Artist"))
    );
    assert_eq!(
        existing_ancestor(&music.join("Artist/Gone/Deeper"), &folders),
        Some(music.join("Artist"))
    );
    assert_eq!(
        existing_ancestor(&dir.path().join("elsewhere"), &folders),
        None
    );
    // The folder itself is gone: nothing to scan, so its tracks stay.
    let gone = [dir.path().join("unmounted")];
    assert_eq!(existing_ancestor(&gone[0].join("x"), &gone), None);
}

#[test]
fn startup_deletes_thumbnails_of_covers_no_longer_in_the_library() {
    let dir = TempDir::new();
    let stale = format!("cache/thumbnails/{}-320.jpg", "a".repeat(64));
    let stale = dir.write(&stale, b"");
    let other = dir.write("cache/thumbnails/notes.txt", b"");
    let h = Harness::start(dir);
    h.loaded_size();
    h.idle();
    assert!(!stale.exists());
    assert!(other.exists());
}

#[test]
fn progress_names_the_folder_and_the_phase() {
    let at = |p| progress_status("/music", p);
    assert_eq!(
        at(ScanProgress::Listing { files: 0 }),
        "Scanning /music: listing files, 0 found"
    );
    assert_eq!(
        at(ScanProgress::Listing { files: 1 }),
        "Scanning /music: listing files, 1 found"
    );
    assert_eq!(
        at(ScanProgress::Reading {
            done: 1240,
            total: 5000
        }),
        "Scanning /music: reading tags, 1240 of 5000"
    );
}

#[test]
fn adding_a_folder_reports_setting_up_watching_before_scanning() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let h = Harness::start(dir);
    h.idle();
    h.service.send(Job::AddFolder(a));
    let first = h.wait_for(|u| match u {
        Update::Status(Some(s)) => Some(s),
        _ => None,
    });
    assert_eq!(first, "Setting up folder watching");
    h.wait_for(|u| is_scanning(&u).then_some(()));
    h.idle();
}

#[test]
fn removing_the_last_folder_reports_no_status() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let h = Harness::start(dir);
    h.idle();
    h.service.send(Job::AddFolder(a.clone()));
    h.idle();
    h.service.send(Job::RemoveFolder(a));
    h.wait_for(|u| match u {
        Update::Status(Some(s)) => panic!("unexpected status {s:?}"),
        Update::Status(None) => Some(()),
        _ => None,
    });
}

#[test]
fn adding_a_folder_reports_how_many_tags_are_left_to_read() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let h = Harness::start(dir);
    h.idle();
    h.service.send(Job::AddFolder(a));
    h.wait_for(|u| match u {
        Update::Status(Some(s)) if s.contains("reading tags, 0 of 2") => Some(()),
        _ => None,
    });
    h.idle();
}

impl Harness {
    fn folders(&self) -> Vec<(PathBuf, Option<i64>)> {
        self.wait_for(|u| match u {
            Update::Folders(folders) => Some(
                folders
                    .into_iter()
                    .map(|f| (f.path, f.last_scanned_ns))
                    .collect(),
            ),
            _ => None,
        })
    }
}

fn at(secs: i64) -> i64 {
    secs * 1_000_000_000
}

#[test]
fn adding_a_folder_lists_it_unscanned_then_scanned() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let clock = FakeClock::default();
    clock.advance(Duration::from_secs(100));
    let h = Harness::start_with_clock(dir, &clock);
    h.idle();
    h.service.send(Job::AddFolder(a.clone()));
    assert_eq!(h.folders(), [(a.clone(), None)]);
    assert_eq!(h.folders(), [(a, Some(at(100)))]);
}

#[test]
fn a_rescan_all_updates_every_folder_and_survives_a_restart() {
    let dir = two_folders();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    Settings {
        library_folders: vec![a.clone(), b.clone()],
        ..Settings::default()
    }
    .save(&Harness::config(&dir))
    .unwrap();
    let clock = FakeClock::default();
    clock.advance(Duration::from_secs(100));
    let h = Harness::start_with_clock(dir, &clock);
    assert_eq!(h.folders(), [(a.clone(), None), (b.clone(), None)]);
    h.idle();

    clock.advance(Duration::from_secs(50));
    h.service.send(Job::RescanAll);
    let scanned = [(a.clone(), Some(at(150))), (b.clone(), Some(at(150)))];
    assert_eq!(h.folders(), scanned);
    h.idle();

    let Harness { dir, service, .. } = h;
    drop(service);
    let h = Harness::start_with_clock(dir, &clock);
    assert_eq!(h.folders(), scanned, "listed before the startup scan");
    h.idle();
}

#[test]
fn rescanning_a_directory_inside_a_folder_leaves_its_time_alone() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let clock = FakeClock::default();
    let h = Harness::start_with_clock(dir, &clock);
    h.idle();
    h.service.send(Job::AddFolder(a.clone()));
    h.idle();

    let inside = a.join("Artist");
    // A missing directory would be rescanned through its folder instead.
    assert!(inside.is_dir());
    clock.advance(Duration::from_secs(10));
    h.service.send(Job::Rescan(vec![inside]));
    h.wait_for(|u| match u {
        Update::Folders(_) => panic!("only a scan of the whole folder counts"),
        Update::Status(None) => Some(()),
        _ => None,
    });
}

#[test]
fn an_unavailable_folder_stays_unscanned() {
    let dir = two_folders();
    let (a, gone) = (dir.path().join("a"), dir.path().join("unmounted"));
    Settings {
        library_folders: vec![a.clone(), gone.clone()],
        ..Settings::default()
    }
    .save(&Harness::config(&dir))
    .unwrap();
    let clock = FakeClock::default();
    clock.advance(Duration::from_secs(100));
    let h = Harness::start_with_clock(dir, &clock);
    assert_eq!(h.folders(), [(a.clone(), None), (gone.clone(), None)]);
    assert_eq!(h.folders(), [(a, Some(at(100))), (gone, None)]);
}

#[test]
fn a_removed_folder_added_again_starts_unscanned() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let clock = FakeClock::default();
    clock.advance(Duration::from_secs(100));
    let h = Harness::start_with_clock(dir, &clock);
    h.idle();
    h.service.send(Job::AddFolder(a.clone()));
    assert_eq!(h.folders(), [(a.clone(), None)]);
    assert_eq!(h.folders(), [(a.clone(), Some(at(100)))]);
    h.idle();

    h.service.send(Job::RemoveFolder(a.clone()));
    assert_eq!(h.folders(), []);
    h.idle();
    clock.advance(Duration::from_secs(50));
    h.service.send(Job::AddFolder(a.clone()));
    assert_eq!(h.folders(), [(a.clone(), None)]);
    assert_eq!(h.folders(), [(a, Some(at(150)))]);
}

#[test]
fn the_periodic_rescan_updates_the_folder_times() {
    let dir = two_folders();
    let a = dir.path().join("a");
    let clock = FakeClock::default();
    let h = Harness::start_with_clock(dir, &clock);
    h.idle();
    h.service.send(Job::AddFolder(a.clone()));
    h.idle();

    clock.advance(RESCAN_EVERY);
    let due = at(i64::try_from(RESCAN_EVERY.as_secs()).unwrap());
    // Only wakes the thread, which may already have started the rescan.
    h.service.send(Job::Rescan(Vec::new()));
    h.wait_for(|u| match u {
        Update::Folders(folders) => (folders[0].last_scanned_ns == Some(due)).then_some(()),
        _ => None,
    });
}
