use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use proptest::prelude::*;
use proptest::sample::select;
use rusqlite::{Connection, named_params};

use crate::browse::{FilterValue, Grouping, Sort, SortColumn, Source, TrackFilter};
use crate::lists::ListId;
use crate::normalize;
use crate::query::ListTrack;
use crate::tags::TrackMeta;

/// Each test gets a name no earlier one had: Windows can't create a
/// directory where a dropped one is still pending deletion.
static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = create_fresh_dir(
            &std::env::temp_dir(),
            &format!(
                "lune-core-test-{}",
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ),
        );
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn touch(&self, rel: &str) -> PathBuf {
        self.write(rel, b"")
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
        std::fs::write(&path, bytes).unwrap();
        path
    }

    pub fn remove(&self, rel: &str) {
        #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
        std::fs::remove_file(self.0.join(rel)).unwrap();
    }

    pub fn remove_dir(&self, rel: &str) {
        #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
        std::fs::remove_dir_all(self.0.join(rel)).unwrap();
    }

    /// Moves a file or directory, keeping its mtime.
    pub fn rename(&self, from: &str, to: &str) {
        let to = self.0.join(to);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
        std::fs::rename(self.0.join(from), to).unwrap();
    }

    /// Sets the mtime of `rel` (a file or directory, "" for the root) to an
    /// hour ago, so a scan treats it as settled.
    pub fn age(&self, rel: &str) {
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        self.set_mtime(rel, an_hour_ago);
    }

    pub fn mtime(&self, rel: &str) -> std::time::SystemTime {
        std::fs::metadata(self.0.join(rel))
            .unwrap()
            .modified()
            .unwrap()
    }

    pub fn set_mtime(&self, rel: &str, mtime: std::time::SystemTime) {
        open_for_times(&self.0.join(rel))
            .set_modified(mtime)
            .unwrap();
    }
}

#[cfg(unix)]
fn open_for_times(path: &Path) -> std::fs::File {
    std::fs::File::open(path).unwrap()
}

/// Windows needs write access to set times, and backup semantics to open a
/// directory at all.
#[cfg(windows)]
fn open_for_times(path: &Path) -> std::fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
    std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .unwrap()
}

impl Drop for TempDir {
    fn drop(&mut self) {
        #[expect(clippy::disallowed_methods, reason = "removes the test's own temp dir")]
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn meta(album_artist: &str, album: &str, track_no: u32) -> TrackMeta {
    TrackMeta {
        title: Some(format!("Track {track_no}")),
        artist: Some(album_artist.to_owned()),
        album_artist: Some(album_artist.to_owned()),
        album: Some(album.to_owned()),
        track_no: Some(track_no),
        duration_ms: 1000,
        codec: "mp3".to_owned(),
        sample_rate: Some(44_100),
        bitrate_kbps: Some(320),
        ..Default::default()
    }
}

pub fn wav_bytes(frames: u32) -> Vec<u8> {
    let data = frames * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    for v in [
        16u32.to_le_bytes().as_slice(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
    ] {
        b.extend_from_slice(v);
    }
    b.extend_from_slice(&44_100u32.to_le_bytes());
    b.extend_from_slice(&88_200u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    b
}

/// A FLAC file with no audio: STREAMINFO, then a front-cover PICTURE block
/// holding `cover` (none when `cover` is empty).
pub fn flac_with_cover(cover: &[u8], mime: &str) -> Vec<u8> {
    let mut b = b"fLaC".to_vec();
    let last = if cover.is_empty() { 0x80 } else { 0 };
    b.push(last); // STREAMINFO
    b.extend_from_slice(&34u32.to_be_bytes()[1..]);
    b.extend_from_slice(&4096u16.to_be_bytes());
    b.extend_from_slice(&4096u16.to_be_bytes());
    b.extend_from_slice(&[0; 6]); // min and max frame size: unknown
    // 44100 Hz (20 bits), 2 channels (3 bits, minus one), 16 bits (5 bits,
    // minus one), 0 total samples (36 bits).
    let packed: u64 = (44_100 << 44) | (1 << 41) | (15 << 36);
    b.extend_from_slice(&packed.to_be_bytes());
    b.extend_from_slice(&[0; 16]); // MD5
    if cover.is_empty() {
        return b;
    }
    let mut picture = Vec::new();
    picture.extend_from_slice(&3u32.to_be_bytes()); // front cover
    picture.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    picture.extend_from_slice(mime.as_bytes());
    picture.extend_from_slice(&0u32.to_be_bytes()); // description
    picture.extend_from_slice(&[0; 16]); // width, height, depth, colors
    picture.extend_from_slice(&(cover.len() as u32).to_be_bytes());
    picture.extend_from_slice(cover);
    b.push(0x80 | 6);
    b.extend_from_slice(&(picture.len() as u32).to_be_bytes()[1..]);
    b.extend_from_slice(&picture);
    b
}

/// Few, clashing values (case, spacing, and composition variants of one
/// name, and blanks), so generated tracks share keys and tie.
const NAMES: [&str; 7] = [
    "Air",
    "air",
    "  AIR ",
    "Beck",
    "",
    "Beyonc\u{e9}",
    "BEYONCE\u{301}",
];

/// Adds tracks with these ids, at paths `<id>.mp3`, all on one album.
pub fn library_with_tracks(conn: &Connection, ids: &[i64]) {
    conn.execute(
        "INSERT INTO albums (id, album_artist, album_artist_key, title, title_key)
         VALUES (1, 'A', 'a', 'T', 't')",
        [],
    )
    .unwrap();
    for id in ids {
        conn.execute(
            "INSERT INTO tracks (id, path, mtime_ns, size, album_id, duration_ms, codec)
             VALUES (:id, :path, 0, 0, 1, 1000, 'mp3')",
            named_params! { ":id": id, ":path": format!("{id}.mp3") },
        )
        .unwrap();
    }
}

pub fn arb_grouping() -> impl Strategy<Value = Grouping> {
    select(&Grouping::ALL[..])
}

pub fn arb_list_id() -> impl Strategy<Value = ListId> {
    any::<i64>().prop_map(|id| id.to_string().parse().unwrap())
}

pub fn arb_source() -> impl Strategy<Value = Source> {
    prop_oneof![
        arb_grouping().prop_map(Source::Library),
        arb_list_id().prop_map(Source::Playlist),
    ]
}

pub fn arb_sort_column() -> impl Strategy<Value = SortColumn> {
    select(&SortColumn::ALL[..])
}

pub fn arb_sort() -> impl Strategy<Value = Sort> {
    (arb_sort_column(), any::<bool>()).prop_map(|(column, descending)| Sort { column, descending })
}

pub fn arb_filter_value() -> impl Strategy<Value = FilterValue> {
    prop_oneof![
        Just(FilterValue::All),
        any::<String>().prop_map(FilterValue::Text),
        any::<i64>().prop_map(FilterValue::Album),
        any::<i32>().prop_map(FilterValue::Year),
        Just(FilterValue::UnknownYear),
    ]
}

fn arb_name() -> impl Strategy<Value = String> {
    select(&NAMES[..]).prop_map(str::to_owned)
}

/// A track of an album, without the album's fields or an id.
fn arb_album_track() -> impl Strategy<Value = ListTrack> {
    (
        (
            arb_name(),
            prop::option::of(arb_name()),
            prop::option::of(arb_name()),
        ),
        (prop::option::of(1..=2u32), prop::option::of(1..=3u32)),
        select(&[0, 1000, 90_000][..]),
        select(&["mp3", "flac", "aac"][..]),
        prop::option::of(select(&[128, 320][..])),
        (prop::option::of(0..3i64), prop::option::of(0..3i64)),
    )
        .prop_map(
            |(
                (title, artist, genre),
                (disc_no, track_no),
                duration_ms,
                codec,
                bitrate_kbps,
                (added_ns, played_ns),
            )| {
                ListTrack {
                    title,
                    artist,
                    genre,
                    disc_no,
                    track_no,
                    duration_ms,
                    codec: codec.to_owned(),
                    bitrate_kbps,
                    added_ns,
                    played_ns,
                    ..Default::default()
                }
            },
        )
}

/// A library's tracks in [`crate::Library::track_list`] order, as
/// [`crate::Catalog::new`] needs them, with ids from 1.
pub fn arb_tracks() -> impl Strategy<Value = Vec<ListTrack>> {
    let album = (
        arb_name(),
        arb_name(),
        prop::option::of(select(&[1994, 2000][..])),
        prop::collection::vec(arb_album_track(), 1..4),
    );
    prop::collection::vec(album, 0..6).prop_map(|albums| {
        let mut albums: Vec<_> = albums.into_iter().zip(1..).collect();
        albums.sort_by_cached_key(|((artist, title, year, _), id)| {
            (
                normalize::key(artist),
                year.is_none(),
                *year,
                normalize::key(title),
                *id,
            )
        });
        let mut tracks = Vec::new();
        for ((album_artist, album, album_year, album_tracks), album_id) in albums {
            for track in album_tracks {
                let id = tracks.len() as i64 + 1;
                tracks.push(ListTrack {
                    id,
                    path: PathBuf::from(format!("{id}.mp3")),
                    album_id,
                    album_key: normalize::key(&album),
                    album: album.clone(),
                    album_artist_key: normalize::key(&album_artist),
                    album_artist: album_artist.clone(),
                    album_year,
                    ..track
                });
            }
        }
        tracks
    })
}

/// Search results over [`arb_tracks`]' ids, or no search.
pub fn arb_search() -> impl Strategy<Value = TrackFilter> {
    prop::option::of(prop::collection::hash_set(1..20i64, 0..20))
        .prop_map(|ids| ids.map_or(TrackFilter::All, |ids| TrackFilter::Ids(Arc::new(ids))))
}

/// A file name that isn't valid UTF-8: a stray byte on Unix, an unpaired
/// surrogate on Windows.
pub fn non_utf8_name() -> std::ffi::OsString {
    #[cfg(unix)]
    let name = std::os::unix::ffi::OsStringExt::from_vec(b"\xff".to_vec());
    #[cfg(windows)]
    let name = std::os::windows::ffi::OsStringExt::from_wide(&[0xD800]);
    name
}

/// A new directory `<prefix>-<pid>-<n>` in `parent`. `create_dir` fails on
/// anything already there, so a link planted at a predictable name in a
/// shared temp dir is skipped instead of written through.
pub fn create_fresh_dir(parent: &Path, prefix: &str) -> PathBuf {
    let mut n = 0u64;
    loop {
        let path = parent.join(format!("{prefix}-{}-{n}", std::process::id()));
        match std::fs::create_dir(&path) {
            Ok(()) => return path,
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(err) => panic!("could not create {}: {err}", path.display()),
        }
    }
}

// Its only test plants a link with `std::os::unix`.
#[cfg(unix)]
#[path = "test_util_test.rs"]
mod tests;
