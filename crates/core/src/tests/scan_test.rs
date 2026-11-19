use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::query::ListTrack;
use crate::test_util::{TempDir, meta, wav_bytes};
use crate::{db, query};

fn scan_with(
    conn: &mut Connection,
    root: &Path,
    mode: ScanMode,
    reader: impl Fn(&Path) -> Result<TrackMeta> + Sync,
) -> Result<ScanReport> {
    scan_using(conn, root, mode, reader, &mut |_| {})
}

/// Scans `dir` with fake tags: each file's name maps to its metadata.
fn scan_tags(conn: &mut Connection, dir: &TempDir, tags: &[(&str, TrackMeta)]) -> ScanReport {
    let tags: HashMap<String, TrackMeta> = tags
        .iter()
        .map(|(rel, m)| (dir.path().join(rel).to_str().unwrap().to_owned(), m.clone()))
        .collect();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let canonical_dir = dir.path().to_str().unwrap().to_owned();
    let root = root.to_str().unwrap().to_owned();
    scan_with(conn, dir.path(), ScanMode::Thorough, move |path: &Path| {
        // Paths arrive canonicalized; map them back to the fixture names.
        let key = path.to_str().unwrap().replacen(&root, &canonical_dir, 1);
        tags.get(&key)
            .cloned()
            .ok_or(CoreError::NotADirectory(path.to_path_buf()))
    })
    .unwrap()
}

fn with_year(mut m: TrackMeta, year: Option<i32>) -> TrackMeta {
    m.year = year;
    m
}

fn with_disc(mut m: TrackMeta, disc: u32) -> TrackMeta {
    m.disc_no = Some(disc);
    m
}

fn titles(conn: &Connection, album_id: i64) -> Vec<String> {
    query::album_tracks(conn, album_id)
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect()
}

#[test]
fn a_panicking_tag_reader_fails_one_file_not_the_scan() {
    let dir = TempDir::new();
    dir.touch("bad.mp3");
    dir.touch("good.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let report = scan_with(&mut conn, dir.path(), ScanMode::Thorough, |path: &Path| {
        if path.ends_with("bad.mp3") {
            panic!("simulated parser bug");
        }
        Ok(meta("Artist", "Album", 1))
    })
    .unwrap();
    assert_eq!(report.failed, 1);
    assert_eq!(report.added, 1);
}

// Creating the links needs `std::os::unix`; the code under test is portable.
#[cfg(unix)]
#[test]
fn symlinks_inside_a_folder_are_not_followed() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new();
    let outside = TempDir::new();
    let real = dir.touch("music/real/01.mp3");
    outside.touch("elsewhere/02.mp3");
    symlink(&real, dir.path().join("music/link.mp3")).unwrap();
    symlink(
        outside.path().join("elsewhere"),
        dir.path().join("music/linked"),
    )
    .unwrap();
    symlink(dir.path(), dir.path().join("music/real/loop")).unwrap();
    let mut conn = db::open_in_memory().unwrap();
    let report = scan_with(
        &mut conn,
        &dir.path().join("music"),
        ScanMode::Thorough,
        |_: &Path| Ok(meta("A", "B", 1)),
    )
    .unwrap();
    assert_eq!(report.added, 1, "{report:?}");
    assert_eq!(report.walk_errors, 0, "{report:?}");
    let paths: Vec<String> = conn
        .prepare("SELECT path FROM tracks")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert!(paths[0].ends_with("real/01.mp3"), "{paths:?}");
}

// Creating the links needs `std::os::unix`; the code under test is portable.
#[cfg(unix)]
#[test]
fn a_folder_that_is_itself_a_link_is_scanned() {
    let dir = TempDir::new();
    dir.touch("real/01.mp3");
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(dir.path().join("real"), &link).unwrap();
    let mut conn = db::open_in_memory().unwrap();
    let report = scan_with(&mut conn, &link, ScanMode::Thorough, |_: &Path| {
        Ok(meta("A", "B", 1))
    })
    .unwrap();
    assert_eq!(report.added, 1, "{report:?}");
}

#[test]
fn same_title_by_different_album_artists_is_two_albums() {
    let dir = TempDir::new();
    dir.touch("a.mp3");
    dir.touch("b.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(
        &mut conn,
        &dir,
        &[
            ("a.mp3", meta("Queen", "Greatest Hits", 1)),
            ("b.mp3", meta("ABBA", "Greatest Hits", 1)),
        ],
    );
    assert_eq!(query::albums(&conn).unwrap().len(), 2);
}

#[test]
fn a_multi_disc_album_across_folders_is_one_album_in_disc_order() {
    let dir = TempDir::new();
    for rel in ["CD2/01.mp3", "CD1/02.mp3", "CD1/01.mp3"] {
        dir.touch(rel);
    }
    let mut conn = db::open_in_memory().unwrap();
    let mut d2 = with_disc(meta("Pink Floyd", "The Wall", 1), 2);
    d2.title = Some("disc 2".into());
    scan_tags(
        &mut conn,
        &dir,
        &[
            ("CD2/01.mp3", d2),
            (
                "CD1/02.mp3",
                with_disc(meta("Pink Floyd", "The Wall", 2), 1),
            ),
            (
                "CD1/01.mp3",
                with_disc(meta("Pink Floyd", "the  wall", 1), 1),
            ),
        ],
    );
    let albums = query::albums(&conn).unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(
        titles(&conn, albums[0].id),
        ["Track 1", "Track 2", "disc 2"]
    );
}

#[test]
fn keys_ignore_case_whitespace_and_unicode_composition() {
    let dir = TempDir::new();
    dir.touch("a.mp3");
    dir.touch("b.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(
        &mut conn,
        &dir,
        &[
            ("a.mp3", meta("Beyonc\u{e9}", "Lemonade", 1)),
            ("b.mp3", meta("  BEYONCE\u{301} ", "lemonade", 2)),
        ],
    );
    assert_eq!(query::albums(&conn).unwrap().len(), 1);
}

#[test]
fn album_year_is_the_max_and_never_reorders_tracks() {
    let dir = TempDir::new();
    for rel in ["1.mp3", "2.mp3", "3.mp3"] {
        dir.touch(rel);
    }
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(
        &mut conn,
        &dir,
        &[
            ("1.mp3", with_year(meta("A", "Album", 1), Some(2001))),
            // A single released earlier, and a track with no year.
            ("2.mp3", with_year(meta("A", "Album", 2), Some(1995))),
            ("3.mp3", with_year(meta("A", "Album", 3), None)),
        ],
    );
    let album = &query::albums(&conn).unwrap()[0];
    assert_eq!(album.year_max, Some(2001));
    assert_eq!(titles(&conn, album.id), ["Track 1", "Track 2", "Track 3"]);
}

#[test]
fn an_album_with_no_years_is_unknown() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(&mut conn, &dir, &[("1.mp3", meta("A", "Album", 1))]);
    assert_eq!(query::albums(&conn).unwrap()[0].year_max, None);
}

#[test]
fn rescans_update_years_and_hide_deleted_tracks_and_empty_albums() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    dir.touch("2.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let m1 = with_year(meta("A", "Album", 1), Some(2000));
    let m2 = with_year(meta("A", "Album", 2), Some(2010));
    scan_tags(
        &mut conn,
        &dir,
        &[("1.mp3", m1.clone()), ("2.mp3", m2.clone())],
    );
    assert_eq!(query::albums(&conn).unwrap()[0].year_max, Some(2010));

    let report = scan_tags(&mut conn, &dir, &[("1.mp3", m1.clone()), ("2.mp3", m2)]);
    assert_eq!(report.unchanged, 2);

    dir.write("2.mp3", b"changed");
    let m2 = with_year(meta("A", "Album", 2), Some(2005));
    let report = scan_tags(&mut conn, &dir, &[("1.mp3", m1.clone()), ("2.mp3", m2)]);
    assert_eq!(report.updated, 1);
    assert_eq!(query::albums(&conn).unwrap()[0].year_max, Some(2005));

    dir.remove("2.mp3");
    let report = scan_tags(&mut conn, &dir, &[("1.mp3", m1)]);
    assert_eq!(report.missing, 1);
    assert_eq!(query::albums(&conn).unwrap()[0].year_max, Some(2000));

    dir.remove("1.mp3");
    scan_tags(&mut conn, &dir, &[]);
    assert!(query::albums(&conn).unwrap().is_empty());
}

#[test]
fn unused_art_is_deleted() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let mut m = meta("A", "Album", 1);
    m.art = Some(crate::EmbeddedArt {
        hash: "abc".into(),
        mime: None,
    });
    scan_tags(&mut conn, &dir, &[("1.mp3", m)]);
    assert_eq!(query::stats(&conn).unwrap().art, 1);
    dir.write("1.mp3", b"no art now");
    scan_tags(&mut conn, &dir, &[("1.mp3", meta("A", "Album", 1))]);
    assert_eq!(query::stats(&conn).unwrap().art, 0);
}

#[test]
fn album_display_strings_follow_the_tags_on_rescan() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(
        &mut conn,
        &dir,
        &[("1.mp3", meta("the beatles", "abbey road", 1))],
    );
    dir.write("1.mp3", b"retagged");
    scan_tags(
        &mut conn,
        &dir,
        &[("1.mp3", meta("The Beatles", "Abbey Road", 1))],
    );
    let albums = query::albums(&conn).unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(
        (albums[0].album_artist.as_str(), albums[0].title.as_str()),
        ("The Beatles", "Abbey Road")
    );
    let hits = query::search(&conn, "abbey", 10).unwrap();
    assert_eq!(hits[0].album, "Abbey Road");
}

#[test]
fn an_empty_library_has_an_empty_track_list() {
    let conn = db::open_in_memory().unwrap();
    assert!(query::track_list(&conn).unwrap().is_empty());
}

#[test]
fn every_tag_comes_back_in_its_own_field() {
    let dir = TempDir::new();
    dir.touch("1.flac");
    let tags = TrackMeta {
        title: Some("Title".to_owned()),
        artist: Some("Artist".to_owned()),
        album_artist: Some("Album Artist".to_owned()),
        album: Some("Album".to_owned()),
        track_no: Some(3),
        disc_no: Some(2),
        year: Some(1987),
        duration_ms: 234_567,
        codec: "flac".to_owned(),
        sample_rate: Some(48_000),
        bitrate_kbps: Some(912),
        art: Some(crate::EmbeddedArt {
            hash: "abc".into(),
            mime: None,
        }),
    };
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(&mut conn, &dir, &[("1.flac", tags)]);
    let id = query::track_list(&conn).unwrap()[0].id;
    let played = SystemTime::UNIX_EPOCH + Duration::from_secs(5);
    query::record_play(&conn, id, played).unwrap();

    let list = query::track_list(&conn).unwrap();
    let [track] = list.as_slice() else {
        panic!("{list:?}");
    };
    assert_eq!(
        *track,
        ListTrack {
            id,
            path: track.path.clone(),
            title: "Title".to_owned(),
            artist: Some("Artist".to_owned()),
            album_id: track.album_id,
            album: "Album".to_owned(),
            album_key: "album".to_owned(),
            album_artist: "Album Artist".to_owned(),
            album_artist_key: "album artist".to_owned(),
            art_id: track.art_id,
            album_year: Some(1987),
            disc_no: Some(2),
            track_no: Some(3),
            duration_ms: 234_567,
            codec: "flac".to_owned(),
            sample_rate: Some(48_000),
            bitrate_kbps: Some(912),
            added_ns: track.added_ns,
            played_ns: Some(unix_ns(played)),
        }
    );
    assert!(track.path.ends_with("1.flac"));
    assert!(track.art_id.is_some());
    assert!(track.added_ns.is_some_and(|added| added != unix_ns(played)));
    assert_eq!(query::album_tracks(&conn, track.album_id).unwrap(), list);
    assert_eq!(query::search(&conn, "title", 10).unwrap(), list);
}

#[test]
fn the_track_list_groups_albums_by_key_in_album_order() {
    let dir = TempDir::new();
    let files = ["1.mp3", "2.mp3", "3.mp3", "4.mp3", "5.mp3", "6.mp3"];
    for f in files {
        dir.touch(f);
    }
    let mut no_title = meta("beta", "Greatest Hits", 1);
    no_title.title = None;
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(
        &mut conn,
        &dir,
        &[
            (
                "1.mp3",
                with_year(meta("Gamma", "Greatest Hits", 1), Some(1990)),
            ),
            ("2.mp3", with_disc(meta("alpha", "Live", 1), 2)),
            ("3.mp3", meta("alpha", "Live", 2)),
            (
                "4.mp3",
                with_year(meta("Gamma", "Greatest Hits", 2), Some(2001)),
            ),
            ("5.mp3", no_title),
            ("6.mp3", meta("alpha", "Live", 1)),
        ],
    );

    let list = query::track_list(&conn).unwrap();
    let rows: Vec<(&str, &str, &str, Option<i32>)> = list
        .iter()
        .map(|t| {
            (
                t.album_artist.as_str(),
                t.album.as_str(),
                t.title.as_str(),
                t.album_year,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("alpha", "Live", "Track 1", None),
            ("alpha", "Live", "Track 2", None),
            ("alpha", "Live", "Track 1", None),
            ("beta", "Greatest Hits", "5.mp3", None),
            ("Gamma", "Greatest Hits", "Track 1", Some(2001)),
            ("Gamma", "Greatest Hits", "Track 2", Some(2001)),
        ]
    );
    assert_eq!(list[2].disc_no, Some(2), "disc 2 comes after disc 1");
    assert_eq!(list[0].duration_ms, 1000);
    assert_eq!(list[0].artist.as_deref(), Some("alpha"));
}

#[test]
fn scanning_never_modifies_music_files() {
    let dir = TempDir::new();
    let paths: Vec<PathBuf> = (0..4)
        .map(|i| dir.write(&format!("{i}.wav"), &wav_bytes(1000 + i)))
        .collect();
    let fingerprint = |p: &PathBuf| {
        let meta = std::fs::metadata(p).unwrap();
        let bytes = std::fs::read(p).unwrap();
        (blake3::hash(&bytes), meta.len(), meta.modified().unwrap())
    };
    let before: Vec<_> = paths.iter().map(fingerprint).collect();
    let mut conn = db::open_in_memory().unwrap();
    let report = scan(&mut conn, dir.path(), ScanMode::Thorough, &mut |_| {}).unwrap();
    assert_eq!(report.added, 4);
    scan(&mut conn, dir.path(), ScanMode::Quick, &mut |_| {}).unwrap();
    let after: Vec<_> = paths.iter().map(fingerprint).collect();
    assert_eq!(before, after);
}

#[test]
fn the_track_list_orders_an_artists_albums_by_year_with_unknown_last() {
    let dir = TempDir::new();
    for f in ["1.mp3", "2.mp3", "3.mp3"] {
        dir.touch(f);
    }
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(
        &mut conn,
        &dir,
        &[
            ("1.mp3", meta("Artist", "Apple", 1)),
            ("2.mp3", with_year(meta("Artist", "Zebra", 1), Some(1990))),
            ("3.mp3", with_year(meta("Artist", "Mango", 1), Some(2000))),
        ],
    );
    let albums: Vec<String> = query::track_list(&conn)
        .unwrap()
        .into_iter()
        .map(|t| t.album)
        .collect();
    assert_eq!(albums, ["Zebra", "Mango", "Apple"]);
}

#[test]
fn search_ids_returns_every_match_and_nothing_for_empty_input() {
    let dir = TempDir::new();
    for f in ["1.mp3", "2.mp3", "3.mp3"] {
        dir.touch(f);
    }
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(
        &mut conn,
        &dir,
        &[
            ("1.mp3", meta("Radiohead", "Kid A", 1)),
            ("2.mp3", meta("Radiohead", "Kid A", 2)),
            ("3.mp3", meta("Portishead", "Dummy", 1)),
        ],
    );
    assert_eq!(query::search_ids(&conn, "radio").unwrap().len(), 2);
    assert_eq!(query::search_ids(&conn, "dummy portis").unwrap().len(), 1);
    assert!(query::search_ids(&conn, "   ").unwrap().is_empty());
    assert!(query::search_ids(&conn, "nothing").unwrap().is_empty());
}

fn scan_two_folders(conn: &mut Connection, dir: &TempDir) {
    for f in ["keep/1.mp3", "keep/2.mp3", "drop/1.mp3", "keeper/1.mp3"] {
        dir.touch(f);
    }
    scan_tags(
        conn,
        dir,
        &[
            ("keep/1.mp3", meta("A", "Kept", 1)),
            ("keep/2.mp3", meta("A", "Kept", 2)),
            ("drop/1.mp3", meta("B", "Dropped", 1)),
            ("keeper/1.mp3", meta("C", "Sibling", 1)),
        ],
    );
}

#[test]
fn prune_removes_tracks_outside_the_folders_and_their_empty_albums() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    let removed = prune(&mut conn, &[dir.path().join("keep")]).unwrap();
    // "keeper" shares a prefix with "keep" but is not inside it.
    assert_eq!(removed, Some(2));
    let albums: Vec<String> = query::albums(&conn)
        .unwrap()
        .into_iter()
        .map(|a| a.title)
        .collect();
    assert_eq!(albums, ["Kept"]);
    assert!(
        query::search_ids(&conn, "dropped").unwrap().is_empty(),
        "FTS rows go too"
    );
}

#[test]
fn prune_with_every_folder_present_removes_nothing() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    assert_eq!(
        prune(&mut conn, &[dir.path().to_path_buf()]).unwrap(),
        Some(0)
    );
    assert_eq!(query::stats(&conn).unwrap().tracks, 4);
}

#[test]
fn prune_with_no_folders_empties_the_library() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    assert_eq!(prune(&mut conn, &[]).unwrap(), Some(4));
    assert_eq!(query::stats(&conn).unwrap(), crate::LibraryStats::default());
}

#[test]
fn prune_removes_nothing_while_a_folder_is_unavailable() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    let unmounted = dir.path().join("unmounted-drive");
    assert_eq!(
        prune(&mut conn, &[dir.path().join("keep"), unmounted]).unwrap(),
        None
    );
    assert_eq!(query::stats(&conn).unwrap().tracks, 4);
}

// Creating the link needs `std::os::unix`; the code under test is portable.
#[cfg(unix)]
#[test]
fn prune_matches_a_folder_given_through_a_symlink() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    let link = dir.path().join("link-to-keep");
    std::os::unix::fs::symlink(dir.path().join("keep"), &link).unwrap();
    assert_eq!(prune(&mut conn, &[link]).unwrap(), Some(2));
}

#[test]
fn folder_scans_are_none_until_recorded_and_keep_the_latest() {
    let conn = db::open_in_memory().unwrap();
    let (a, b) = (PathBuf::from("/music/a"), PathBuf::from("/music/b"));
    assert_eq!(folder_scans(&conn, &[]).unwrap(), []);
    assert_eq!(
        folder_scans(&conn, std::slice::from_ref(&a)).unwrap(),
        [None]
    );

    let first = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let second = first + Duration::from_nanos(1);
    record_folder_scan(&conn, &a, first).unwrap();
    assert_eq!(
        folder_scans(&conn, &[b.clone(), a.clone()]).unwrap(),
        [None, Some(1_700_000_000_000_000_000)]
    );
    record_folder_scan(&conn, &a, second).unwrap();
    assert_eq!(
        folder_scans(&conn, &[a]).unwrap(),
        [Some(1_700_000_000_000_000_001)]
    );
}

#[test]
fn a_folder_scan_before_the_epoch_round_trips() {
    let conn = db::open_in_memory().unwrap();
    let folder = PathBuf::from("/music");
    let at = UNIX_EPOCH - Duration::from_secs(5);
    record_folder_scan(&conn, &folder, at).unwrap();
    assert_eq!(
        folder_scans(&conn, &[folder]).unwrap(),
        [Some(-5_000_000_000)]
    );
}

// Making a non-UTF-8 path needs `std::os::unix`; the code under test is portable.
#[cfg(unix)]
#[test]
fn a_non_utf8_folder_scan_is_not_recorded() {
    use std::os::unix::ffi::OsStrExt;
    let conn = db::open_in_memory().unwrap();
    let folder = PathBuf::from(std::ffi::OsStr::from_bytes(b"/music/\xff"));
    record_folder_scan(&conn, &folder, UNIX_EPOCH).unwrap();
    assert_eq!(folder_scans(&conn, &[folder]).unwrap(), [None]);
}

#[test]
fn prune_forgets_the_scans_of_removed_folders_only() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    let (keep, drop) = (dir.path().join("keep"), dir.path().join("drop"));
    let keeper = dir.path().join("keeper");
    for folder in [&keep, &drop, &keeper] {
        record_folder_scan(&conn, folder, UNIX_EPOCH).unwrap();
    }
    prune(&mut conn, std::slice::from_ref(&keep)).unwrap();
    assert_eq!(
        folder_scans(&conn, &[keep, drop, keeper]).unwrap(),
        [Some(0), None, None]
    );
}

#[test]
fn prune_forgets_the_scan_of_a_removed_folder_while_another_is_unavailable() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    let (keep, drop) = (dir.path().join("keep"), dir.path().join("drop"));
    let unmounted = dir.path().join("unmounted-drive");
    for folder in [&keep, &drop] {
        record_folder_scan(&conn, folder, UNIX_EPOCH).unwrap();
    }
    assert_eq!(prune(&mut conn, &[keep.clone(), unmounted]).unwrap(), None);
    assert_eq!(folder_scans(&conn, &[keep, drop]).unwrap(), [Some(0), None]);
}

#[test]
fn prune_forgets_the_scan_of_a_folder_covered_by_a_new_parent() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    scan_two_folders(&mut conn, &dir);
    let keep = dir.path().join("keep");
    record_folder_scan(&conn, &keep, UNIX_EPOCH).unwrap();
    prune(&mut conn, &[dir.path().to_path_buf()]).unwrap();
    assert_eq!(folder_scans(&conn, &[keep]).unwrap(), [None]);
}

fn art(hash: &str) -> Option<crate::EmbeddedArt> {
    Some(crate::EmbeddedArt {
        hash: hash.into(),
        mime: None,
    })
}

fn art_hashes(conn: &Connection) -> Vec<String> {
    conn.prepare("SELECT hash FROM art ORDER BY hash")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

#[test]
fn a_changed_cover_drops_the_old_one_unless_another_track_still_has_it() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    dir.touch("2.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let with = |n, hash| TrackMeta {
        art: art(hash),
        ..meta("A", "Album", n)
    };
    scan_tags(
        &mut conn,
        &dir,
        &[("1.mp3", with(1, "old")), ("2.mp3", with(2, "old"))],
    );
    assert_eq!(art_hashes(&conn), ["old"]);

    dir.write("1.mp3", b"new cover");
    scan_tags(
        &mut conn,
        &dir,
        &[("1.mp3", with(1, "new")), ("2.mp3", with(2, "old"))],
    );
    assert_eq!(
        art_hashes(&conn),
        ["new", "old"],
        "2.mp3 still has the old one"
    );

    dir.write("2.mp3", b"new cover");
    scan_tags(
        &mut conn,
        &dir,
        &[("1.mp3", with(1, "new")), ("2.mp3", with(2, "new"))],
    );
    assert_eq!(art_hashes(&conn), ["new"]);

    dir.remove("1.mp3");
    dir.remove("2.mp3");
    scan_tags(&mut conn, &dir, &[]);
    assert_eq!(
        art_hashes(&conn),
        ["new"],
        "missing tracks keep their cover"
    );
}

#[test]
fn prune_drops_the_covers_of_the_tracks_it_removes() {
    let dir = TempDir::new();
    dir.touch("keep/1.mp3");
    dir.touch("drop/1.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let with = |hash| TrackMeta {
        art: art(hash),
        ..meta("A", hash, 1)
    };
    scan_tags(
        &mut conn,
        &dir,
        &[
            ("keep/1.mp3", with("kept")),
            ("drop/1.mp3", with("dropped")),
        ],
    );
    prune(&mut conn, &[dir.path().join("keep")]).unwrap();
    assert_eq!(art_hashes(&conn), ["kept"]);
}

#[test]
fn rescanning_a_folder_leaves_folders_that_share_its_name_prefix_alone() {
    let dir = TempDir::new();
    let mut conn = db::open_in_memory().unwrap();
    // "keep." sorts just before "keep/", "keeper" just after it.
    for f in ["keep/1.mp3", "keep./1.mp3", "keeper/1.mp3"] {
        dir.touch(f);
    }
    let all = [
        ("keep/1.mp3", meta("A", "Kept", 1)),
        ("keep./1.mp3", meta("B", "Dot", 1)),
        ("keeper/1.mp3", meta("C", "Sibling", 1)),
    ];
    scan_tags(&mut conn, &dir, &all);
    dir.remove("keep/1.mp3");
    let report = scan_with(
        &mut conn,
        &dir.path().join("keep"),
        ScanMode::Thorough,
        |_: &Path| Ok(meta("A", "Kept", 1)),
    )
    .unwrap();
    assert_eq!(report.missing, 1, "{report:?}");
    assert_eq!(query::stats(&conn).unwrap().tracks, 2);
}

#[cfg(unix)]
#[test]
fn the_prefix_range_covers_exactly_the_paths_under_a_root() {
    assert_eq!(
        path_prefix_range("/music"),
        ("/music/".to_owned(), "/music0".to_owned())
    );
    assert_eq!(
        path_prefix_range("/music/"),
        ("/music/".to_owned(), "/music0".to_owned())
    );
    assert_eq!(path_prefix_range("/"), ("/".to_owned(), "0".to_owned()));
}

fn track_list(conn: &Connection) -> HashMap<u32, ListTrack> {
    query::track_list(conn)
        .unwrap()
        .into_iter()
        .map(|t| (t.track_no.unwrap(), t))
        .collect()
}

#[test]
fn a_new_track_is_dated_by_the_earlier_of_its_created_and_modified_times() {
    let dir = TempDir::new();
    let path = dir.touch("1.mp3");
    let modified = UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(modified).unwrap();
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(&mut conn, &dir, &[("1.mp3", meta("A", "Album", 1))]);
    assert_eq!(track_list(&conn)[&1].added_ns, Some(unix_ns(modified)));
}

#[test]
fn a_changed_file_keeps_its_date_added_and_last_play() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_tags(&mut conn, &dir, &[("1.mp3", meta("A", "Album", 1))]);
    let before = track_list(&conn)[&1].clone();
    let played = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    query::record_play(&conn, before.id, played).unwrap();

    dir.write("1.mp3", b"retagged");
    let report = scan_tags(&mut conn, &dir, &[("1.mp3", meta("A", "Album", 1))]);
    assert_eq!(report.updated, 1);
    let after = &track_list(&conn)[&1];
    assert_eq!(after.added_ns, before.added_ns);
    assert_eq!(after.played_ns, Some(unix_ns(played)));
}

#[test]
fn a_rescan_dates_unchanged_tracks_from_before_dates_were_stored() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    dir.touch("2.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let tags = [
        ("1.mp3", meta("A", "Album", 1)),
        ("2.mp3", meta("A", "Album", 2)),
    ];
    scan_tags(&mut conn, &dir, &tags);
    let dated = track_list(&conn)[&1].added_ns;
    conn.execute("UPDATE tracks SET added_ns = NULL", [])
        .unwrap();

    let report = scan_tags(&mut conn, &dir, &tags);
    assert_eq!((report.unchanged, report.dated), (2, 2));
    assert_eq!(track_list(&conn)[&1].added_ns, dated);

    let report = scan_tags(&mut conn, &dir, &tags);
    assert_eq!(report.dated, 0);
}

#[test]
fn a_play_of_a_removed_track_is_ignored() {
    let conn = db::open_in_memory().unwrap();
    query::record_play(&conn, 42, SystemTime::now()).unwrap();
}

#[test]
fn the_track_list_has_each_files_codec_and_bit_rate() {
    let dir = TempDir::new();
    dir.write("1.wav", &wav_bytes(1000));
    let mut conn = db::open_in_memory().unwrap();
    scan(&mut conn, dir.path(), ScanMode::Thorough, &mut |_| {}).unwrap();
    let track = &query::track_list(&conn).unwrap()[0];
    // 44.1 kHz 16-bit mono is 705.6 kbps.
    assert_eq!(
        (track.codec.as_str(), track.bitrate_kbps),
        ("wav", Some(706))
    );
}

/// Scans `root`, tagging each file with its name as the title. Returns the
/// report and how many files had their tags read.
fn scan_counting(conn: &mut Connection, root: &Path, mode: ScanMode) -> (ScanReport, usize) {
    let reads = AtomicUsize::new(0);
    let report = scan_with(conn, root, mode, |path: &Path| {
        reads.fetch_add(1, Ordering::Relaxed);
        let mut m = meta("A", "Album", 1);
        m.title = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        Ok(m)
    })
    .unwrap();
    (report, reads.into_inner())
}

fn present_titles(conn: &Connection) -> Vec<String> {
    let mut titles: Vec<String> = query::track_list(conn)
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    titles.sort();
    titles
}

fn missing_titles(conn: &Connection) -> Vec<String> {
    query::missing_tracks(conn)
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect()
}

#[test]
fn a_deleted_file_is_kept_as_missing_and_hidden_from_the_library() {
    let dir = TempDir::new();
    dir.touch("keep.mp3");
    dir.touch("gone.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    dir.remove("gone.mp3");

    let (report, _) = scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    assert_eq!((report.missing, report.unchanged), (1, 1), "{report:?}");
    assert_eq!(present_titles(&conn), ["keep"]);
    assert_eq!(missing_titles(&conn), ["gone"]);
    assert_eq!(query::stats(&conn).unwrap().tracks, 1);
    let album = query::albums(&conn).unwrap()[0].clone();
    assert_eq!(album.track_count, 1);
    assert_eq!(titles(&conn, album.id), ["keep"]);
    assert!(query::search(&conn, "gone", 10).unwrap().is_empty());
    assert!(query::search_ids(&conn, "gone").unwrap().is_empty());

    let (report, _) = scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    assert_eq!(report.missing, 0, "it was already missing");
    assert_eq!(missing_titles(&conn), ["gone"]);
}

#[test]
fn a_missing_file_that_returns_unchanged_is_restored_without_rereading_it() {
    let dir = TempDir::new();
    dir.touch("music/song.mp3");
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, &music, ScanMode::Thorough);
    let before = query::track_list(&conn).unwrap()[0].clone();
    let played = SystemTime::now();
    query::record_play(&conn, before.id, played).unwrap();

    dir.rename("music/song.mp3", "away/song.mp3");
    scan_counting(&mut conn, &music, ScanMode::Thorough);
    assert!(present_titles(&conn).is_empty());

    dir.rename("away/song.mp3", "music/song.mp3");
    let (report, reads) = scan_counting(&mut conn, &music, ScanMode::Thorough);
    assert_eq!((report.restored, reads), (1, 0), "{report:?}");
    let after = query::track_list(&conn).unwrap()[0].clone();
    assert_eq!(after.id, before.id, "the queue and history refer to the id");
    assert_eq!(after.added_ns, before.added_ns);
    assert_eq!(after.played_ns, Some(unix_ns(played)));
    assert!(missing_titles(&conn).is_empty());
}

#[test]
fn a_missing_file_that_returns_changed_is_reread() {
    let dir = TempDir::new();
    dir.touch("song.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    dir.remove("song.mp3");
    scan_counting(&mut conn, dir.path(), ScanMode::Thorough);

    dir.write("song.mp3", b"re-encoded");
    let (report, reads) = scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    assert_eq!((report.updated, reads), (1, 1), "{report:?}");
    assert_eq!(present_titles(&conn), ["song"]);
    assert!(missing_titles(&conn).is_empty());
}

#[test]
fn a_folder_that_looks_empty_keeps_its_tracks_until_they_return() {
    // An unmounted share leaves an empty mount point behind.
    let dir = TempDir::new();
    dir.touch("share/x/1.mp3");
    dir.touch("share/x/2.mp3");
    let share = dir.path().join("share");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, &share, ScanMode::Thorough);

    dir.rename("share/x", "offline/x");
    let (report, _) = scan_counting(&mut conn, &share, ScanMode::Quick);
    assert_eq!((report.missing, report.files_found), (2, 0), "{report:?}");
    assert_eq!(missing_titles(&conn), ["1", "2"]);

    dir.rename("offline/x", "share/x");
    let (report, reads) = scan_counting(&mut conn, &share, ScanMode::Quick);
    assert_eq!((report.restored, reads), (2, 0), "{report:?}");
    assert_eq!(present_titles(&conn), ["1", "2"]);
}

/// A library at `music` of `1.mp3` and `sub/2.mp3`, scanned after its
/// directories settled, so the next quick scan may skip them.
fn settled_library() -> (TempDir, PathBuf, Connection) {
    let dir = TempDir::new();
    dir.touch("music/1.mp3");
    dir.touch("music/sub/2.mp3");
    dir.age("music");
    dir.age("music/sub");
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Thorough);
    assert_eq!(report.added, 2);
    (dir, music, conn)
}

// `age` sets a directory's mtime, which works on Unix only; the code under
// test is portable.
#[cfg(unix)]
#[test]
fn a_quick_scan_skips_unchanged_directories() {
    let (_dir, music, mut conn) = settled_library();
    let (report, reads) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(reads, 0);
    assert_eq!(report.dirs_skipped, 2, "{report:?}");
    assert_eq!((report.unchanged, report.files_found), (2, 2), "{report:?}");
    assert_eq!(report.missing, 0);
    assert_eq!(present_titles(&conn), ["1", "2"]);
}

#[cfg(unix)]
#[test]
fn a_quick_scan_finds_a_file_added_deep_inside_an_unchanged_directory() {
    let (dir, music, mut conn) = settled_library();
    dir.touch("music/sub/3.mp3");
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!((report.added, report.dirs_skipped), (1, 1), "{report:?}");
    assert_eq!(present_titles(&conn), ["1", "2", "3"]);
}

#[cfg(unix)]
#[test]
fn a_quick_scan_marks_the_tracks_of_a_deleted_directory_missing() {
    let (dir, music, mut conn) = settled_library();
    dir.remove_dir("music/sub");
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(report.missing, 1, "{report:?}");
    assert_eq!(missing_titles(&conn), ["2"]);
}

#[cfg(unix)]
#[test]
fn a_file_rewritten_in_place_is_missed_by_a_quick_scan_and_found_by_a_thorough_one() {
    let (dir, music, mut conn) = settled_library();
    dir.write("music/1.mp3", b"retagged");
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(report.updated, 0, "the directory's mtime didn't change");
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Thorough);
    assert_eq!(report.updated, 1, "{report:?}");
}

#[cfg(unix)]
#[test]
fn a_quick_scan_rereads_tracks_read_by_an_older_reader_once() {
    let (_dir, music, mut conn) = settled_library();
    let before = query::track_list(&conn).unwrap();
    conn.execute(
        "UPDATE tracks SET reader_version = ?1 WHERE title = '2'",
        [READER_VERSION - 1],
    )
    .unwrap();

    let (report, reads) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(
        (reads, report.updated, report.unchanged),
        (1, 1, 1),
        "{report:?}"
    );
    assert_eq!(report.dirs_skipped, 1, "only `sub` holds a stale track");
    assert_eq!(query::track_list(&conn).unwrap(), before);

    let (report, reads) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!((reads, report.dirs_skipped), (0, 2), "{report:?}");
}

#[test]
fn a_thorough_scan_rereads_a_track_read_by_an_older_reader() {
    let dir = TempDir::new();
    dir.touch("1.mp3");
    dir.touch("2.mp3");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    conn.execute(
        "UPDATE tracks SET reader_version = ?1 WHERE title = '1'",
        [READER_VERSION - 1],
    )
    .unwrap();

    let (report, reads) = scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    assert_eq!(
        (reads, report.updated, report.unchanged),
        (1, 1, 1),
        "{report:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_just_modified_directory_is_not_skipped_by_the_next_quick_scan() {
    let dir = TempDir::new();
    dir.touch("music/1.mp3");
    let now = SystemTime::now();
    dir.set_mtime("music", now - Duration::from_secs(1));
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, &music, ScanMode::Thorough);
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(report.dirs_skipped, 0, "{report:?}");
    assert_eq!(report.unchanged, 1);
}

// A network share whose clock is behind ours: by our clock everything
// changed an hour ago, but `sub` changed a second before the newest file.
#[cfg(unix)]
#[test]
fn a_directory_changed_just_before_the_newest_change_seen_is_not_skipped() {
    let dir = TempDir::new();
    dir.touch("music/sub/1.mp3");
    let newest = SystemTime::now() - Duration::from_secs(3600);
    dir.set_mtime("music/sub/1.mp3", newest);
    dir.set_mtime("music/sub", newest - Duration::from_secs(1));
    dir.set_mtime("music", newest - Duration::from_secs(3600));
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, &music, ScanMode::Thorough);
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(report.dirs_skipped, 1, "only music: {report:?}");
    assert_eq!(report.unchanged, 1);
}

// A file dated in the future doesn't make every directory look settled.
#[cfg(unix)]
#[test]
fn a_file_dated_in_the_future_does_not_settle_a_just_modified_directory() {
    let dir = TempDir::new();
    dir.touch("music/1.mp3");
    let now = SystemTime::now();
    dir.set_mtime("music/1.mp3", now + Duration::from_secs(3600));
    dir.set_mtime("music", now - Duration::from_secs(1));
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, &music, ScanMode::Thorough);
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(report.dirs_skipped, 0, "{report:?}");
}

#[cfg(unix)]
#[test]
fn a_file_whose_tags_failed_is_retried_by_a_quick_scan() {
    let dir = TempDir::new();
    dir.touch("music/bad.mp3");
    dir.touch("music/good.mp3");
    dir.age("music");
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();
    let report = scan_with(&mut conn, &music, ScanMode::Thorough, |path: &Path| {
        if path.ends_with("bad.mp3") {
            return Err(CoreError::NotADirectory(path.to_path_buf()));
        }
        Ok(meta("A", "Album", 1))
    })
    .unwrap();
    assert_eq!(report.failed, 1);

    let (report, reads) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!((report.added, reads), (1, 1), "{report:?}");
}

#[cfg(unix)]
#[test]
fn a_folder_pruned_and_added_back_is_read_again_by_a_quick_scan() {
    let (_dir, music, mut conn) = settled_library();
    prune(&mut conn, &[]).unwrap();
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(report.added, 2, "{report:?}");
}

#[test]
fn tags_are_read_one_file_at_a_time() {
    let dir = TempDir::new();
    for i in 0..8 {
        dir.touch(&format!("{i}.mp3"));
    }
    let mut conn = db::open_in_memory().unwrap();
    // One thread can't read two files at once.
    let readers = std::sync::Mutex::new(HashSet::new());
    let report = scan_with(&mut conn, dir.path(), ScanMode::Thorough, |_: &Path| {
        readers.lock().unwrap().insert(std::thread::current().id());
        Ok(meta("A", "Album", 1))
    })
    .unwrap();
    assert_eq!(report.added, 8);
    assert_eq!(readers.into_inner().unwrap().len(), 1);
}

#[test]
fn progress_counts_the_files_listed_then_the_tags_read_of_a_known_total() {
    let dir = TempDir::new();
    dir.touch("a/1.mp3");
    dir.touch("a/2.mp3");
    dir.touch("b/3.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let mut seen = Vec::new();
    scan_using(
        &mut conn,
        dir.path(),
        ScanMode::Thorough,
        |_: &Path| Ok(meta("A", "Album", 1)),
        &mut |p| seen.push(p),
    )
    .unwrap();
    let listed: Vec<usize> = seen
        .iter()
        .filter_map(|p| match p {
            ScanProgress::Listing { files } => Some(*files),
            ScanProgress::Reading { .. } => None,
        })
        .collect();
    assert!(listed.is_sorted(), "{listed:?}");
    assert_eq!(listed.last(), Some(&3));
    let read: Vec<ScanProgress> = seen
        .iter()
        .copied()
        .filter(|p| matches!(p, ScanProgress::Reading { .. }))
        .collect();
    assert_eq!(
        read,
        (0..=3)
            .map(|done| ScanProgress::Reading { done, total: 3 })
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(seen.last(), Some(ScanProgress::Reading { .. })),
        "listing comes first"
    );

    let mut seen = Vec::new();
    scan_using(
        &mut conn,
        dir.path(),
        ScanMode::Thorough,
        |_: &Path| Ok(meta("A", "Album", 1)),
        &mut |p| seen.push(p),
    )
    .unwrap();
    assert!(
        !seen
            .iter()
            .any(|p| matches!(p, ScanProgress::Reading { .. })),
        "nothing to read: {seen:?}"
    );
}

// `age` sets directory mtimes, which works on Unix only; the code under test
// is portable.
#[cfg(unix)]
#[test]
fn an_interrupted_scan_resumes_from_the_directories_it_had_not_finished() {
    // Each directory fills a fifth of a batch, so the first batch to commit
    // finishes five of the six, whatever order they're read in.
    let per_dir = BATCH_SIZE / 5;
    let dir = TempDir::new();
    for d in 0..6 {
        for f in 0..per_dir {
            dir.touch(&format!("music/{d}/{f}.mp3"));
        }
        dir.age(&format!("music/{d}"));
    }
    dir.age("music");
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();

    // Stops the scan as the app quitting would, just after the first batch.
    let stopped = std::panic::catch_unwind(AssertUnwindSafe(|| {
        scan_using(
            &mut conn,
            &music,
            ScanMode::Quick,
            |_: &Path| Ok(meta("A", "Album", 1)),
            &mut |p| {
                if p == (ScanProgress::Reading {
                    done: BATCH_SIZE + 1,
                    total: 6 * per_dir,
                }) {
                    panic!("stopped");
                }
            },
        )
    }));
    assert!(stopped.is_err());
    assert_eq!(present_titles(&conn).len(), BATCH_SIZE);

    let (report, reads) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(reads, per_dir, "only the unfinished directory is read");
    assert_eq!(report.added, per_dir);
    assert_eq!(
        report.dirs_skipped, 6,
        "the root and five finished: {report:?}"
    );
    assert_eq!(report.missing, 0);
    assert_eq!(present_titles(&conn).len(), 6 * per_dir);
}

#[cfg(unix)]
#[test]
fn a_subdirectory_gone_from_a_directory_whose_mtime_is_unchanged_is_marked_missing() {
    let (dir, music, mut conn) = settled_library();
    let mtime = dir.mtime("music");
    dir.remove_dir("music/sub");
    dir.set_mtime("music", mtime);
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Quick);
    assert_eq!(report.dirs_skipped, 1, "{report:?}");
    assert_eq!(missing_titles(&conn), ["2"]);
}

#[test]
fn an_interrupted_scan_leaves_its_albums_up_to_date() {
    let dir = TempDir::new();
    for f in 0..=BATCH_SIZE {
        dir.touch(&format!("{f}.mp3"));
    }
    let mut conn = db::open_in_memory().unwrap();
    let stopped = std::panic::catch_unwind(AssertUnwindSafe(|| {
        scan_using(
            &mut conn,
            dir.path(),
            ScanMode::Thorough,
            |_: &Path| Ok(with_year(meta("A", "Album", 1), Some(1999))),
            &mut |p| {
                if matches!(p, ScanProgress::Reading { done, .. } if done == BATCH_SIZE + 1) {
                    panic!("stopped");
                }
            },
        )
    }));
    assert!(stopped.is_err());
    assert_eq!(query::albums(&conn).unwrap()[0].year_max, Some(1999));
}

fn track_id(conn: &Connection, title: &str) -> i64 {
    conn.query_row("SELECT id FROM tracks WHERE title = ?1", [title], |r| {
        r.get(0)
    })
    .unwrap()
}

#[test]
fn forgetting_missing_tracks_deletes_only_those_asked_for_and_never_present_ones() {
    let dir = TempDir::new();
    for f in ["keep.mp3", "gone1.mp3", "gone2.mp3"] {
        dir.touch(f);
    }
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, dir.path(), ScanMode::Thorough);
    dir.remove("gone1.mp3");
    dir.remove("gone2.mp3");
    scan_counting(&mut conn, dir.path(), ScanMode::Thorough);

    let ids = [track_id(&conn, "gone1"), track_id(&conn, "keep"), i64::MAX];
    assert_eq!(forget_missing(&mut conn, Some(&ids)).unwrap(), 1);
    assert_eq!(missing_titles(&conn), ["gone2"]);
    assert_eq!(present_titles(&conn), ["keep"]);
    assert_eq!(forget_missing(&mut conn, Some(&[])).unwrap(), 0);
    assert_eq!(missing_titles(&conn), ["gone2"]);
}

#[test]
fn forgetting_every_missing_track_drops_their_albums_and_covers() {
    let dir = TempDir::new();
    dir.touch("keep.mp3");
    dir.touch("gone.mp3");
    let mut conn = db::open_in_memory().unwrap();
    let with = |album, hash| TrackMeta {
        art: art(hash),
        ..meta("A", album, 1)
    };
    let tags = [
        ("keep.mp3", with("Kept", "kept")),
        ("gone.mp3", with("Gone", "gone")),
    ];
    scan_tags(&mut conn, &dir, &tags);
    dir.remove("gone.mp3");
    scan_tags(&mut conn, &dir, &tags[..1]);
    let album_count = |conn: &Connection| -> i64 {
        conn.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(album_count(&conn), 2, "a missing track keeps its album");

    assert_eq!(forget_missing(&mut conn, None).unwrap(), 1);
    assert!(query::missing_tracks(&conn).unwrap().is_empty());
    assert_eq!(album_count(&conn), 1);
    assert_eq!(art_hashes(&conn), ["kept"]);
    assert_eq!(forget_missing(&mut conn, None).unwrap(), 0);
}

#[test]
fn a_forgotten_track_whose_file_returns_is_added_anew() {
    let dir = TempDir::new();
    dir.touch("music/song.mp3");
    let music = dir.path().join("music");
    let mut conn = db::open_in_memory().unwrap();
    scan_counting(&mut conn, &music, ScanMode::Thorough);
    dir.rename("music/song.mp3", "away/song.mp3");
    scan_counting(&mut conn, &music, ScanMode::Thorough);
    forget_missing(&mut conn, None).unwrap();

    dir.rename("away/song.mp3", "music/song.mp3");
    let (report, _) = scan_counting(&mut conn, &music, ScanMode::Thorough);
    assert_eq!((report.added, report.restored), (1, 0), "{report:?}");
    assert_eq!(present_titles(&conn), ["song"]);
}
