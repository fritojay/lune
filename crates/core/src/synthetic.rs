//! Fake library rows for UI benchmarks, written through the scanner's insert path.

use std::collections::HashSet;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::scan::{FoundFile, Touched, refresh_albums, upsert_track};
use crate::tags::TrackMeta;
use crate::{CoreError, Result};

const BATCH_SIZE: usize = 5000;

const WORDS: &[&str] = &[
    "Midnight",
    "Echo",
    "River",
    "Glass",
    "Northern",
    "Sól",
    "Velvet",
    "Static",
    "Paper",
    "Hollow",
    "Neon",
    "Café",
    "Winter",
    "Signal",
    "Golden",
    "Distant",
    "Ninety",
    "Örbit",
    "Fever",
    "Lantern",
    "Satellite",
    "Quiet",
    "Electric",
    "Harbor",
    "Ghost",
    "Summer",
    "Concrete",
    "Wildflower",
    "Machine",
    "Ocean",
];

/// splitmix64: deterministic, so every generated library is the same.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn words(&mut self, max: u64) -> String {
        let count = 1 + self.below(max);
        (0..count)
            .map(|_| WORDS[self.below(WORDS.len() as u64) as usize])
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub(crate) fn generate(conn: &mut Connection, tracks: usize) -> Result<()> {
    let existing: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0))?;
    if existing > 0 {
        return Err(CoreError::NotEmpty(existing.unsigned_abs()));
    }

    let rows = rows(tracks);
    let mut touched = Touched::default();
    for batch in rows.chunks(BATCH_SIZE) {
        let tx = conn.transaction()?;
        for (file, meta) in batch {
            upsert_track(&tx, file, meta, &mut touched)?;
        }
        tx.commit()?;
    }
    let tx = conn.transaction()?;
    refresh_albums(&tx, &touched)?;
    tx.commit()?;
    Ok(())
}

/// Artists with 1 to 10 albums of 6 to 15 tracks, some on two discs, with
/// varied string lengths, featured artists, and a few missing titles and years.
fn rows(tracks: usize) -> Vec<(FoundFile, TrackMeta)> {
    let mut rng = Rng(0x5eed);
    let mut out = Vec::with_capacity(tracks);
    let mut artist = 0;
    while out.len() < tracks {
        artist += 1;
        let artist_name = format!("{} {artist}", rng.words(3));
        let mut titles = HashSet::new();
        for album in 1..=1 + rng.below(10) {
            // Equal titles would merge into one album (same identity).
            let album_title = loop {
                let title = rng.words(4);
                if titles.insert(title.clone()) {
                    break title;
                }
            };
            let discs = if rng.below(10) == 0 { 2 } else { 1 };
            let per_disc = 6 + rng.below(10);
            let base_year = 1960 + rng.below(65) as i32;
            for disc in 1..=discs {
                for track in 1..=per_disc {
                    if out.len() == tracks {
                        return out;
                    }
                    let meta = TrackMeta {
                        title: (rng.below(50) != 0).then(|| rng.words(5)),
                        artist: Some(if rng.below(8) == 0 {
                            format!("{artist_name} feat. {}", rng.words(2))
                        } else {
                            artist_name.clone()
                        }),
                        album_artist: Some(artist_name.clone()),
                        album: Some(album_title.clone()),
                        track_no: Some(track as u32),
                        disc_no: (discs > 1).then_some(disc as u32),
                        year: (rng.below(10) != 0).then(|| base_year + rng.below(3) as i32),
                        duration_ms: 60_000 + rng.below(480_000),
                        codec: "flac".to_owned(),
                        sample_rate: Some(44_100),
                        bitrate_kbps: Some(900 + rng.below(300) as u32),
                        ..Default::default()
                    };
                    let path = PathBuf::from("synthetic")
                        .join(format!("artist-{artist}"))
                        .join(format!("album-{album}"))
                        .join(format!("{disc}-{track:02}.flac"));
                    let file = FoundFile {
                        path: path.to_string_lossy().into_owned(),
                        mtime_ns: 0,
                        size: 0,
                        added_ns: i64::try_from(out.len()).ok(),
                        known: false,
                    };
                    out.push((file, meta));
                }
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "tests/synthetic_test.rs"]
mod tests;
