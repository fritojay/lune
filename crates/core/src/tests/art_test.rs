use std::cell::Cell;

use image::{ImageFormat, RgbaImage};
use rusqlite::params;

use super::*;
use crate::test_util::{TempDir, flac_with_cover};
use crate::{ScanMode, db};

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = RgbaImage::from_pixel(width, height, image::Rgba([200, 10, 10, 128]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// One album with cover `art_id` 1 (of `cover`) on the given track paths.
fn library(cover: &[u8], paths: &[&str]) -> Connection {
    let conn = db::open_in_memory().unwrap();
    conn.execute("INSERT INTO art (id, hash) VALUES (1, ?1)", [hash(cover)])
        .unwrap();
    conn.execute(
        "INSERT INTO albums (id, album_artist, album_artist_key, title, title_key, art_id)
         VALUES (1, 'A', 'a', 'T', 't', 1)",
        [],
    )
    .unwrap();
    for path in paths {
        conn.execute(
            "INSERT INTO tracks (path, mtime_ns, size, album_id, duration_ms, codec, art_id)
             VALUES (?1, 0, 0, 1, 1000, 'flac', 1)",
            params![path],
        )
        .unwrap();
    }
    conn
}

fn dimensions(path: &Path) -> (u32, u32) {
    image::image_dimensions(path).unwrap()
}

#[test]
fn a_cover_embedded_in_a_real_file_becomes_a_cached_jpeg_thumbnail() {
    let dir = TempDir::new();
    let cover = png(1000, 500);
    let track = dir.write("music/1.flac", &flac_with_cover(&cover, "image/png"));
    let conn = library(&cover, &[track.to_str().unwrap()]);
    let cache = dir.path().join("cache");

    let thumb = thumbnail(&conn, 1, &cache, 300).unwrap().unwrap();
    assert!(thumb.starts_with(&cache));
    assert_eq!(dimensions(&thumb), (300, 150), "aspect ratio kept");
    assert_eq!(
        image::ImageReader::open(&thumb)
            .unwrap()
            .with_guessed_format()
            .unwrap()
            .format(),
        Some(ImageFormat::Jpeg)
    );
    assert_eq!(
        std::fs::read_dir(&cache).unwrap().count(),
        1,
        "no temp files left"
    );
}

#[test]
fn a_cached_thumbnail_is_reused_without_reading_the_track() {
    let dir = TempDir::new();
    let cover = png(64, 64);
    let conn = library(&cover, &["1.flac"]);
    let reads = Cell::new(0);
    let read = |_: &Path| {
        reads.set(reads.get() + 1);
        Ok(Some(cover.clone()))
    };
    let first = thumbnail_with(&conn, 1, dir.path(), 32, read).unwrap();
    let second = thumbnail_with(&conn, 1, dir.path(), 32, read).unwrap();
    assert_eq!(first, second);
    assert_eq!(reads.get(), 1);
    // Another size is another file.
    let large = thumbnail_with(&conn, 1, dir.path(), 48, read).unwrap();
    assert_ne!(large, first);
}

#[test]
fn small_covers_are_not_scaled_up() {
    let dir = TempDir::new();
    let cover = png(40, 20);
    let conn = library(&cover, &["1.flac"]);
    let thumb = thumbnail_with(&conn, 1, dir.path(), 300, |_| Ok(Some(cover.clone())))
        .unwrap()
        .unwrap();
    assert_eq!(dimensions(&thumb), (40, 20));
}

#[test]
fn a_file_whose_cover_changed_since_the_scan_is_skipped_for_the_next_one() {
    let dir = TempDir::new();
    let cover = png(10, 10);
    let conn = library(&cover, &["changed.flac", "same.flac"]);
    let thumb = thumbnail_with(&conn, 1, dir.path(), 32, |path| {
        Ok(Some(if path.ends_with("changed.flac") {
            png(11, 11)
        } else {
            cover.clone()
        }))
    })
    .unwrap();
    assert!(thumb.is_some());
}

#[test]
fn no_cover_left_in_any_file_gives_none() {
    let dir = TempDir::new();
    let cover = png(10, 10);
    let conn = library(&cover, &["1.flac", "2.flac"]);
    let none = thumbnail_with(&conn, 1, dir.path(), 32, |path| {
        if path.ends_with("1.flac") {
            Err(CoreError::NotADirectory(path.to_path_buf()))
        } else {
            Ok(None)
        }
    })
    .unwrap();
    assert!(none.is_none());
}

#[test]
fn an_unknown_cover_gives_none() {
    let dir = TempDir::new();
    let conn = db::open_in_memory().unwrap();
    assert!(thumbnail(&conn, 42, dir.path(), 32).unwrap().is_none());
}

#[test]
fn a_corrupt_image_is_an_error_and_caches_nothing() {
    let dir = TempDir::new();
    let mut corrupt = png(10, 10);
    corrupt.truncate(30);
    let conn = library(&corrupt, &["1.flac"]);
    let cache = dir.path().join("cache");
    let result = thumbnail_with(&conn, 1, &cache, 32, |_| Ok(Some(corrupt.clone())));
    assert!(matches!(result, Err(CoreError::Image { .. })), "{result:?}");
    assert!(!cache.exists() || std::fs::read_dir(&cache).unwrap().count() == 0);
}

#[test]
fn a_cover_larger_than_the_limits_is_refused() {
    let dir = TempDir::new();
    let huge = png(MAX_DIMENSION + 1, 1);
    let conn = library(&huge, &["1.flac"]);
    let result = thumbnail_with(&conn, 1, dir.path(), 32, |_| Ok(Some(huge.clone())));
    assert!(matches!(result, Err(CoreError::Image { .. })), "{result:?}");
}

#[test]
fn a_panicking_reader_is_contained() {
    let dir = TempDir::new();
    let cover = png(10, 10);
    let conn = library(&cover, &["1.flac"]);
    let result = thumbnail_with(&conn, 1, dir.path(), 32, |_| panic!("simulated parser bug"));
    assert!(matches!(result, Ok(None)), "{result:?}");
}

#[test]
fn read_cover_returns_none_for_a_file_without_art() {
    let dir = TempDir::new();
    let track = dir.write("1.flac", &flac_with_cover(&[], "image/png"));
    assert_eq!(crate::tags::read_cover(&track).unwrap(), None);
}

#[test]
fn a_scanned_cover_can_be_thumbnailed_through_the_library() {
    let dir = TempDir::new();
    dir.write("music/1.flac", &flac_with_cover(&png(20, 20), "image/png"));
    let mut library = crate::Library::open_in_memory().unwrap();
    let report = library
        .scan(&dir.path().join("music"), ScanMode::Thorough, |_| {})
        .unwrap();
    assert_eq!(report.added, 1, "{report:?}");
    let art_id = library.track_list().unwrap()[0].art_id.unwrap();
    let thumb = library
        .thumbnail(art_id, &dir.path().join("cache"), 16)
        .unwrap();
    assert_eq!(dimensions(&thumb.unwrap()), (16, 16));
}

#[test]
fn a_malformed_hash_never_names_a_file_outside_the_cache() {
    let dir = TempDir::new();
    let outside = dir.write("outside-32.jpg", b"not a cover");
    // It must exist, or `cache/..` doesn't resolve and nothing escapes.
    let cache = dir
        .write("cache/other.jpg", b"")
        .parent()
        .unwrap()
        .to_path_buf();
    let conn = library(b"x", &["1.flac"]);
    conn.execute("UPDATE art SET hash = '../outside' WHERE id = 1", [])
        .unwrap();
    assert!(outside.is_file());
    let result = thumbnail_with(&conn, 1, &cache, 32, |_| Ok(Some(b"x".to_vec())));
    assert!(matches!(result, Ok(None)), "{result:?}");
}

#[test]
fn pruning_deletes_only_thumbnails_of_covers_no_longer_in_the_library() {
    let dir = TempDir::new();
    let kept = hash(b"kept");
    let gone = hash(b"gone");
    let upper = hash(b"upper").to_uppercase();
    let conn = library(b"kept", &["1.flac"]);
    let keep = [
        format!("{kept}-320.jpg"),
        format!("{kept}-32.jpg"),
        // Not named like a thumbnail: never touched.
        format!("{gone}.jpg"),
        format!("{gone}-320.png"),
        format!("{gone}-big.jpg"),
        format!("{upper}-320.jpg"),
        "notes.txt".to_owned(),
    ];
    for name in &keep {
        dir.write(&format!("cache/{name}"), b"");
    }
    dir.write(&format!("cache/{gone}-320.jpg"), b"");
    dir.write(&format!("cache/{gone}-32.jpg"), b"");
    let cache = dir.path().join("cache");

    assert_eq!(prune_thumbnails(&conn, &cache).unwrap(), 2);
    let mut left: Vec<String> = std::fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    let mut expected = keep.to_vec();
    expected.sort();
    assert_eq!(left, expected);
}

#[test]
fn pruning_a_missing_cache_dir_deletes_nothing() {
    let dir = TempDir::new();
    let conn = library(b"x", &[]);
    assert_eq!(
        prune_thumbnails(&conn, &dir.path().join("none")).unwrap(),
        0
    );
}

// Creating the link needs `std::os::unix`; the code under test is portable.
#[cfg(unix)]
#[test]
fn pruning_never_follows_a_link_named_like_a_thumbnail() {
    let dir = TempDir::new();
    let target = dir.write("precious.flac", b"music");
    let cache = dir.path().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::os::unix::fs::symlink(&target, cache.join(format!("{}-320.jpg", hash(b"gone")))).unwrap();
    let conn = library(b"x", &[]);
    assert_eq!(prune_thumbnails(&conn, &cache).unwrap(), 0);
    assert!(target.is_file());
}
