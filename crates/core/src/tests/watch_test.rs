use std::path::Path;
use std::sync::mpsc;

use super::*;
use crate::test_util::TempDir;

fn rescan(roots: Vec<PathBuf>, paths: &[PathBuf]) -> Vec<PathBuf> {
    dirs_to_rescan(&roots, paths.iter())
}

#[test]
fn a_changed_track_rescans_its_directory() {
    let root = PathBuf::from("/music");
    assert_eq!(
        rescan(vec![root.clone()], &[root.join("A/Album/1.flac")]),
        [root.join("A/Album")]
    );
}

#[test]
fn a_change_at_the_top_of_a_root_rescans_the_root() {
    let root = PathBuf::from("/music");
    assert_eq!(
        rescan(vec![root.clone()], &[root.join("1.mp3")]),
        vec![root.clone()]
    );
    // The root itself was removed or renamed.
    assert_eq!(
        rescan(vec![root.clone()], std::slice::from_ref(&root)),
        [root]
    );
}

#[test]
fn nested_and_repeated_directories_collapse() {
    let root = PathBuf::from("/music");
    let dirs = rescan(
        vec![root.clone()],
        &[
            root.join("A/Album/CD1/1.mp3"),
            root.join("A/Album/2.mp3"),
            root.join("A/Album/3.mp3"),
            root.join("A/Album (Deluxe)/1.mp3"),
            root.join("B/1.mp3"),
        ],
    );
    assert_eq!(
        dirs,
        [
            root.join("A/Album"),
            root.join("A/Album (Deluxe)"),
            root.join("B")
        ]
    );
}

#[test]
fn paths_outside_every_root_are_ignored() {
    let dirs = rescan(
        vec![PathBuf::from("/music")],
        &[PathBuf::from("/musical/1.mp3")],
    );
    assert!(dirs.is_empty());
    assert!(rescan(vec![], &[PathBuf::from("/music/1.mp3")]).is_empty());
}

#[test]
fn other_existing_files_are_ignored_but_folders_and_deletions_count() {
    let dir = TempDir::new();
    let root = dir.path().to_path_buf();
    let cover = dir.touch("Album/cover.jpg");
    // A folder whose name looks like it has an extension.
    std::fs::create_dir_all(root.join("Vol. 2")).unwrap();
    assert!(rescan(vec![root.clone()], &[cover]).is_empty());
    assert_eq!(
        rescan(vec![root.clone()], &[root.join("Vol. 2")]),
        vec![root.clone()]
    );
    // Deleted: it can't be told apart from a folder, so it counts.
    assert_eq!(
        rescan(vec![root.clone()], &[root.join("Album/gone.jpg")]),
        [root.join("Album")]
    );
}

#[test]
fn reads_and_metadata_changes_do_not_count_as_changes() {
    use notify::event::{
        AccessKind, AccessMode, CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode,
    };
    assert!(!changes_files(&EventKind::Access(AccessKind::Open(
        AccessMode::Read
    ))));
    assert!(!changes_files(&EventKind::Access(AccessKind::Close(
        AccessMode::Write
    ))));
    assert!(!changes_files(&EventKind::Modify(ModifyKind::Metadata(
        MetadataKind::Permissions
    ))));
    assert!(changes_files(&EventKind::Create(CreateKind::File)));
    assert!(changes_files(&EventKind::Remove(RemoveKind::Folder)));
    assert!(changes_files(&EventKind::Modify(ModifyKind::Data(
        DataChange::Content
    ))));
    assert!(changes_files(&EventKind::Modify(ModifyKind::Name(
        RenameMode::Both
    ))));
    assert!(changes_files(&EventKind::Any));
}

fn watch_settled(dir: &TempDir, delay: Duration) -> (FolderWatcher, mpsc::Receiver<Vec<PathBuf>>) {
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let marker = root.join("settle");
    std::fs::create_dir_all(&marker).unwrap();
    let (tx, rx) = mpsc::channel();
    let (watcher, failed) = FolderWatcher::start(std::slice::from_ref(&root), delay, move |dirs| {
        let _ = tx.send(dirs);
    })
    .unwrap();
    assert!(failed.is_empty());
    dir.touch("settle/marker.flac");
    loop {
        let dirs = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        if dirs.iter().any(|d| marker.starts_with(d)) {
            break;
        }
    }
    // Windows can report the marker again, e.g. as its folder's mtime updates.
    while rx.recv_timeout(Duration::from_millis(500)).is_ok() {}
    (watcher, rx)
}

fn event(kind: EventKind, path: &Path) -> notify::Result<Event> {
    Ok(Event::new(kind).add_path(path.to_path_buf()))
}

#[test]
fn a_remove_that_repeats_a_create_is_still_reported() {
    use notify::event::{CreateKind, RemoveKind};
    let dir = TempDir::new();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut pending = Pending::default();
    pending.add(event(
        EventKind::Create(CreateKind::Folder),
        &root.join("Artist"),
    ));
    pending.add(event(
        EventKind::Remove(RemoveKind::Folder),
        &root.join("Artist"),
    ));
    assert_eq!(pending.take(std::slice::from_ref(&root)), [root]);
}

#[test]
fn only_reads_leave_nothing_pending() {
    use notify::event::{AccessKind, AccessMode};
    let mut pending = Pending::default();
    pending.add(event(
        EventKind::Access(AccessKind::Open(AccessMode::Read)),
        Path::new("/music/A/1.flac"),
    ));
    assert!(pending.is_empty());
}

#[test]
fn dropped_events_or_errors_rescan_every_root() {
    let roots = [PathBuf::from("/music"), PathBuf::from("/more")];
    let mut pending = Pending::default();
    pending.add(Ok(
        Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan)
    ));
    assert_eq!(pending.take(&roots), roots);
    assert!(pending.is_empty());

    pending.add(Err(notify::Error::generic("watch failed")));
    assert_eq!(pending.take(&roots), roots);
}

#[test]
fn changes_are_reported_once_and_then_cleared() {
    use notify::event::{CreateKind, ModifyKind};
    let dir = TempDir::new();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let mut pending = Pending::default();
    pending.add(event(
        EventKind::Create(CreateKind::File),
        &root.join("A/1.flac"),
    ));
    pending.add(event(
        EventKind::Modify(ModifyKind::Any),
        &root.join("A/1.flac"),
    ));
    pending.add(event(
        EventKind::Create(CreateKind::File),
        &root.join("A/2.flac"),
    ));
    assert_eq!(pending.take(std::slice::from_ref(&root)), [root.join("A")]);
    assert!(pending.is_empty());
    assert!(pending.take(std::slice::from_ref(&root)).is_empty());
}

#[test]
fn reading_files_in_a_watched_folder_is_not_reported() {
    let dir = TempDir::new();
    let track = dir.write("Artist/1.flac", b"audio");
    let (_watcher, rx) = watch_settled(&dir, Duration::from_millis(50));
    // What a scan does: list folders and read files.
    for _ in 0..3 {
        std::fs::read_dir(dir.path().join("Artist"))
            .unwrap()
            .count();
        std::fs::read(&track).unwrap();
    }
    assert_eq!(rx.recv_timeout(Duration::from_millis(500)).ok(), None);
}

#[test]
fn a_new_file_in_a_watched_folder_is_reported() {
    let dir = TempDir::new();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    std::fs::create_dir_all(root.join("Artist")).unwrap();
    let (_watcher, rx) = watch_settled(&dir, Duration::from_millis(100));
    dir.touch("Artist/new.flac");
    let dirs = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(dirs, [root.join("Artist")]);
}

#[test]
fn a_missing_folder_is_reported_and_the_rest_are_watched() {
    let dir = TempDir::new();
    let missing = dir.path().join("unmounted");
    let (_watcher, failed) = FolderWatcher::start(
        &[dir.path().to_path_buf(), missing.clone()],
        Duration::from_millis(100),
        |_| {},
    )
    .unwrap();
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, missing);
}
