use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::{Connection, Row};

use crate::Result;
use crate::scan::unix_ns;

/// Track order within an album: disc (missing = 1), then track number
/// (missing last), then path. The single definition of album order: every
/// query that needs it aliases `tracks` as `t`.
pub(crate) const ALBUM_TRACK_ORDER: &str =
    "COALESCE(t.disc_no, 1), t.track_no IS NULL, t.track_no, t.path";

const TRACK_COLUMNS: &str = "t.id, t.path, t.title, t.artist, t.album_id, a.title, a.album_artist,
     t.disc_no, t.track_no, t.year, a.year_max, t.duration_ms, t.codec, t.sample_rate";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryStats {
    pub tracks: u64,
    pub albums: u64,
    pub art: u64,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumRow {
    pub id: i64,
    pub album_artist: String,
    pub title: String,
    /// Max year across the album's tracks; `None` = Unknown.
    pub year_max: Option<i32>,
    pub track_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackRow {
    pub id: i64,
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_id: i64,
    pub album: String,
    pub album_artist: String,
    pub disc_no: Option<u32>,
    pub track_no: Option<u32>,
    /// The track's own year tag.
    pub year: Option<i32>,
    /// The album's max year, used for grouping.
    pub album_year: Option<i32>,
    pub duration_ms: u64,
    pub codec: String,
    pub sample_rate: Option<u32>,
}

/// One row of a flat track list: what the browser displays, groups by, and
/// hands to the queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListTrack {
    pub id: i64,
    pub path: PathBuf,
    /// The title tag, or the file name when the track has none.
    pub title: String,
    pub artist: Option<String>,
    pub album_id: i64,
    pub album: String,
    pub album_key: String,
    pub album_artist: String,
    pub album_artist_key: String,
    /// The album's cover.
    pub art_id: Option<i64>,
    /// The album's max year; `None` = Unknown.
    pub album_year: Option<i32>,
    pub disc_no: Option<u32>,
    pub track_no: Option<u32>,
    pub duration_ms: u64,
    /// Nanoseconds since the Unix epoch; `None` until a scan dates it.
    pub added_ns: Option<i64>,
    /// When playback of it last started, in nanoseconds since the Unix
    /// epoch; `None` if never.
    pub played_ns: Option<i64>,
}

/// A track whose file a scan no longer finds. It is hidden from the library
/// until the file reappears.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingTrack {
    pub id: i64,
    pub path: PathBuf,
    /// The title tag, or the file name when the track has none.
    pub title: String,
    pub artist: Option<String>,
    pub album: String,
    /// When a scan first found it gone, in nanoseconds since the Unix epoch.
    pub missing_ns: i64,
}

/// Reads a non-negative integer column. SQLite has no unsigned type; negative
/// values never occur for these columns and read as 0.
fn get_u64(row: &Row<'_>, idx: usize) -> rusqlite::Result<u64> {
    Ok(u64::try_from(row.get::<_, i64>(idx)?).unwrap_or(0))
}

impl TrackRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            path: PathBuf::from(row.get::<_, String>(1)?),
            title: row.get(2)?,
            artist: row.get(3)?,
            album_id: row.get(4)?,
            album: row.get(5)?,
            album_artist: row.get(6)?,
            disc_no: row.get(7)?,
            track_no: row.get(8)?,
            year: row.get(9)?,
            album_year: row.get(10)?,
            duration_ms: get_u64(row, 11)?,
            codec: row.get(12)?,
            sample_rate: row.get(13)?,
        })
    }
}

pub(crate) fn stats(conn: &Connection) -> Result<LibraryStats> {
    Ok(conn.query_row(
        "SELECT (SELECT COUNT(*) FROM tracks WHERE missing_ns IS NULL),
                (SELECT COUNT(DISTINCT album_id) FROM tracks WHERE missing_ns IS NULL),
                (SELECT COUNT(*) FROM art),
                (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks WHERE missing_ns IS NULL)",
        [],
        |row| {
            Ok(LibraryStats {
                tracks: get_u64(row, 0)?,
                albums: get_u64(row, 1)?,
                art: get_u64(row, 2)?,
                total_duration_ms: get_u64(row, 3)?,
            })
        },
    )?)
}

pub(crate) fn albums(conn: &Connection) -> Result<Vec<AlbumRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.album_artist, a.title, a.year_max, COUNT(*)
         FROM albums a JOIN tracks t ON t.album_id = a.id
         WHERE t.missing_ns IS NULL
         GROUP BY a.id
         ORDER BY a.album_artist_key, a.title_key",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AlbumRow {
            id: row.get(0)?,
            album_artist: row.get(1)?,
            title: row.get(2)?,
            year_max: row.get(3)?,
            track_count: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn album_tracks(conn: &Connection, album_id: i64) -> Result<Vec<TrackRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TRACK_COLUMNS}
         FROM tracks t JOIN albums a ON a.id = t.album_id
         WHERE t.album_id = ?1 AND t.missing_ns IS NULL
         ORDER BY {ALBUM_TRACK_ORDER}"
    ))?;
    let rows = stmt.query_map([album_id], TrackRow::from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Every track, grouped by album and in album order within each album. Albums
/// are ordered by album artist (by key), then year (Unknown last), then title.
pub(crate) fn track_list(conn: &Connection) -> Result<Vec<ListTrack>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id, t.path, t.title, t.artist, a.id, a.title, a.title_key,
                a.album_artist, a.album_artist_key, a.art_id, a.year_max,
                t.disc_no, t.track_no, t.duration_ms, t.added_ns, t.played_ns
         FROM tracks t JOIN albums a ON a.id = t.album_id
         WHERE t.missing_ns IS NULL
         ORDER BY a.album_artist_key, a.year_max IS NULL, a.year_max, a.title_key, a.id,
                  {ALBUM_TRACK_ORDER}"
    ))?;
    let rows = stmt.query_map([], |row| {
        let path: String = row.get(1)?;
        let title = match row.get::<_, Option<String>>(2)? {
            Some(title) => title,
            None => file_name(&path),
        };
        Ok(ListTrack {
            id: row.get(0)?,
            path: PathBuf::from(path),
            title,
            artist: row.get(3)?,
            album_id: row.get(4)?,
            album: row.get(5)?,
            album_key: row.get(6)?,
            album_artist: row.get(7)?,
            album_artist_key: row.get(8)?,
            art_id: row.get(9)?,
            album_year: row.get(10)?,
            disc_no: row.get(11)?,
            track_no: row.get(12)?,
            duration_ms: get_u64(row, 13)?,
            added_ns: row.get(14)?,
            played_ns: row.get(15)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Every missing track, by path.
pub(crate) fn missing_tracks(conn: &Connection) -> Result<Vec<MissingTrack>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.path, t.title, t.artist, a.title, t.missing_ns
         FROM tracks t JOIN albums a ON a.id = t.album_id
         WHERE t.missing_ns IS NOT NULL
         ORDER BY t.path",
    )?;
    let rows = stmt.query_map([], |row| {
        let path: String = row.get(1)?;
        let title = match row.get::<_, Option<String>>(2)? {
            Some(title) => title,
            None => file_name(&path),
        };
        Ok(MissingTrack {
            id: row.get(0)?,
            path: PathBuf::from(path),
            title,
            artist: row.get(3)?,
            album: row.get(4)?,
            missing_ns: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn record_play(conn: &Connection, track_id: i64, at: SystemTime) -> Result<()> {
    conn.prepare_cached("UPDATE tracks SET played_ns = ?2 WHERE id = ?1")?
        .execute(rusqlite::params![track_id, unix_ns(at)])?;
    Ok(())
}

fn file_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(
        || path.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Full-text search over title, artist, album artist, and album. Every word
/// must match, as a prefix. Results are ordered by relevance.
pub(crate) fn search(conn: &Connection, text: &str, limit: u32) -> Result<Vec<TrackRow>> {
    let Some(query) = fts_query(text) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT {TRACK_COLUMNS}
         FROM tracks_fts f
         JOIN tracks t ON t.id = f.rowid
         JOIN albums a ON a.id = t.album_id
         WHERE tracks_fts MATCH ?1 AND t.missing_ns IS NULL
         ORDER BY f.rank
         LIMIT ?2"
    ))?;
    let rows = stmt.query_map(rusqlite::params![query, limit], TrackRow::from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The ids of every track that matches `text`, the same way as [`search`].
/// Empty input matches nothing.
pub(crate) fn search_ids(conn: &Connection, text: &str) -> Result<HashSet<i64>> {
    let Some(query) = fts_query(text) else {
        return Ok(HashSet::new());
    };
    let mut stmt = conn.prepare(
        "SELECT f.rowid FROM tracks_fts f JOIN tracks t ON t.id = f.rowid
         WHERE tracks_fts MATCH ?1 AND t.missing_ns IS NULL",
    )?;
    let rows = stmt.query_map([query], |row| row.get(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Turns user input into an FTS5 query: each word quoted (so FTS syntax in
/// the input is treated as text) and prefix-matched, all words required.
fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split_whitespace()
        .map(|word| word.replace('"', ""))
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{word}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

#[cfg(test)]
#[path = "tests/query_test.rs"]
mod tests;
