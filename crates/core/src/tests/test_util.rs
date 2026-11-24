use std::collections::HashSet;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use proptest::sample::select;

use crate::browse::{FilterValue, Grouping, Sort, SortColumn};
use crate::normalize;
use crate::query::ListTrack;
use crate::tags::TrackMeta;

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = create_fresh_dir(&std::env::temp_dir(), "lune-core-test");
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
    /// hour ago, so a scan treats it as settled. Opening a directory to set its
    /// times works on Unix only.
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

    /// Unix only for directories, like [`Self::age`].
    pub fn set_mtime(&self, rel: &str, mtime: std::time::SystemTime) {
        std::fs::File::open(self.0.join(rel))
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }
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

pub fn arb_grouping() -> impl Strategy<Value = Grouping> {
    select(&Grouping::ALL[..])
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

/// Search results over [`arb_tracks`]' ids, or none.
pub fn arb_matches() -> impl Strategy<Value = Option<HashSet<i64>>> {
    prop::option::of(prop::collection::hash_set(1..20i64, 0..20))
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

#[path = "test_util_test.rs"]
mod tests;
