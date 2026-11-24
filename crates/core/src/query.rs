use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use rusqlite::{Connection, Row, RowIndex, named_params};

use crate::Result;
use crate::scan::unix_ns;

/// Track order within an album: disc (missing = 1), then track number
/// (missing last), then path. The single definition of album order: every
/// query that needs it aliases `tracks` as `t`.
pub(crate) const ALBUM_TRACK_ORDER: &str =
    "COALESCE(t.disc_no, 1), t.track_no IS NULL, t.track_no, t.path";

/// What [`ListTrack::from_row`] reads, as `(expression, name)`. Queries
/// alias `tracks` as `t` and `albums` as `a`.
const LIST_TRACK_COLUMNS: [(&str, &str); 21] = [
    ("t.id", "id"),
    ("t.path", "path"),
    ("t.title", "title"),
    ("t.artist", "artist"),
    ("a.id", "album_id"),
    ("a.title", "album"),
    ("a.title_key", "album_key"),
    ("a.album_artist", "album_artist"),
    ("a.album_artist_key", "album_artist_key"),
    ("a.art_id", "art_id"),
    ("a.year_max", "album_year"),
    ("a.genre", "album_genre"),
    ("t.genre", "genre"),
    ("t.disc_no", "disc_no"),
    ("t.track_no", "track_no"),
    ("t.duration_ms", "duration_ms"),
    ("t.codec", "codec"),
    ("t.sample_rate", "sample_rate"),
    ("t.bitrate_kbps", "bitrate_kbps"),
    ("t.added_ns", "added_ns"),
    ("t.played_ns", "played_ns"),
];

/// The `SELECT` list of [`LIST_TRACK_COLUMNS`].
static LIST_TRACK_SELECT: LazyLock<String> = LazyLock::new(|| {
    LIST_TRACK_COLUMNS
        .map(|(expr, name)| format!("{expr} AS {name}"))
        .join(", ")
});

/// The position of column `name` in [`LIST_TRACK_COLUMNS`]. Looking it up by
/// name per row costs rusqlite a scan of the column names, which doubles the
/// time to load a large library, so it's done at compile time: call it in a
/// `const` block, where a name not in the list is a compile error.
const fn list_column(name: &str) -> usize {
    let mut i = 0;
    while i < LIST_TRACK_COLUMNS.len() {
        if LIST_TRACK_COLUMNS[i].1.eq_ignore_ascii_case(name) {
            return i;
        }
        i += 1;
    }
    panic!("no such column");
}

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

/// One row of a flat track list: what the browser displays, groups by, and
/// hands to the queue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
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
    /// The first genre in album order.
    pub album_genre: Option<String>,
    pub genre: Option<String>,
    pub disc_no: Option<u32>,
    pub track_no: Option<u32>,
    pub duration_ms: u64,
    /// Lowercase codec name, as in [`TrackMeta::codec`](crate::TrackMeta::codec).
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub bitrate_kbps: Option<u32>,
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
fn get_u64(row: &Row<'_>, idx: impl RowIndex) -> rusqlite::Result<u64> {
    Ok(u64::try_from(row.get::<_, i64>(idx)?).unwrap_or(0))
}

impl ListTrack {
    /// Reads a row of [`LIST_TRACK_SELECT`].
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        macro_rules! col {
            ($name:literal) => {
                const { list_column($name) }
            };
        }
        let path: String = row.get(col!("path"))?;
        let title = match row.get::<_, Option<String>>(col!("title"))? {
            Some(title) => title,
            None => file_name(&path),
        };
        Ok(Self {
            id: row.get(col!("id"))?,
            path: PathBuf::from(path),
            title,
            artist: row.get(col!("artist"))?,
            album_id: row.get(col!("album_id"))?,
            album: row.get(col!("album"))?,
            album_key: row.get(col!("album_key"))?,
            album_artist: row.get(col!("album_artist"))?,
            album_artist_key: row.get(col!("album_artist_key"))?,
            art_id: row.get(col!("art_id"))?,
            album_year: row.get(col!("album_year"))?,
            album_genre: row.get(col!("album_genre"))?,
            genre: row.get(col!("genre"))?,
            disc_no: row.get(col!("disc_no"))?,
            track_no: row.get(col!("track_no"))?,
            duration_ms: get_u64(row, col!("duration_ms"))?,
            codec: row.get(col!("codec"))?,
            sample_rate: row.get(col!("sample_rate"))?,
            bitrate_kbps: row.get(col!("bitrate_kbps"))?,
            added_ns: row.get(col!("added_ns"))?,
            played_ns: row.get(col!("played_ns"))?,
        })
    }
}

pub(crate) fn stats(conn: &Connection) -> Result<LibraryStats> {
    Ok(conn.query_row(
        "SELECT (SELECT COUNT(*) FROM tracks WHERE missing_ns IS NULL) AS tracks,
                (SELECT COUNT(DISTINCT album_id) FROM tracks WHERE missing_ns IS NULL) AS albums,
                (SELECT COUNT(*) FROM art) AS art,
                (SELECT COALESCE(SUM(duration_ms), 0) FROM tracks WHERE missing_ns IS NULL)
                    AS total_duration_ms",
        [],
        |row| {
            Ok(LibraryStats {
                tracks: get_u64(row, "tracks")?,
                albums: get_u64(row, "albums")?,
                art: get_u64(row, "art")?,
                total_duration_ms: get_u64(row, "total_duration_ms")?,
            })
        },
    )?)
}

pub(crate) fn albums(conn: &Connection) -> Result<Vec<AlbumRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.album_artist, a.title, a.year_max, COUNT(*) AS track_count
         FROM albums a JOIN tracks t ON t.album_id = a.id
         WHERE t.missing_ns IS NULL
         GROUP BY a.id
         ORDER BY a.album_artist_key, a.title_key",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AlbumRow {
            id: row.get("id")?,
            album_artist: row.get("album_artist")?,
            title: row.get("title")?,
            year_max: row.get("year_max")?,
            track_count: row.get("track_count")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn album_tracks(conn: &Connection, album_id: i64) -> Result<Vec<ListTrack>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {}
         FROM tracks t JOIN albums a ON a.id = t.album_id
         WHERE t.album_id = :album_id AND t.missing_ns IS NULL
         ORDER BY {ALBUM_TRACK_ORDER}",
        *LIST_TRACK_SELECT
    ))?;
    let rows = stmt.query_map(named_params! { ":album_id": album_id }, ListTrack::from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Every track, grouped by album and in album order within each album. Albums
/// are ordered by album artist (by key), then year (Unknown last), then title.
pub(crate) fn track_list(conn: &Connection) -> Result<Vec<ListTrack>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {}
         FROM tracks t JOIN albums a ON a.id = t.album_id
         WHERE t.missing_ns IS NULL
         ORDER BY a.album_artist_key, a.year_max IS NULL, a.year_max, a.title_key, a.id,
                  {ALBUM_TRACK_ORDER}",
        *LIST_TRACK_SELECT
    ))?;
    let rows = stmt.query_map([], ListTrack::from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Every missing track, by path.
pub(crate) fn missing_tracks(conn: &Connection) -> Result<Vec<MissingTrack>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.path, t.title, t.artist, a.title AS album, t.missing_ns
         FROM tracks t JOIN albums a ON a.id = t.album_id
         WHERE t.missing_ns IS NOT NULL
         ORDER BY t.path",
    )?;
    let rows = stmt.query_map([], |row| {
        let path: String = row.get("path")?;
        let title = match row.get::<_, Option<String>>("title")? {
            Some(title) => title,
            None => file_name(&path),
        };
        Ok(MissingTrack {
            id: row.get("id")?,
            path: PathBuf::from(path),
            title,
            artist: row.get("artist")?,
            album: row.get("album")?,
            missing_ns: row.get("missing_ns")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn record_play(conn: &Connection, track_id: i64, at: SystemTime) -> Result<()> {
    conn.prepare_cached("UPDATE tracks SET played_ns = :played_ns WHERE id = :id")?
        .execute(named_params! { ":id": track_id, ":played_ns": unix_ns(at) })?;
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
pub(crate) fn search(conn: &Connection, text: &str, limit: u32) -> Result<Vec<ListTrack>> {
    let Some(query) = fts_query(text) else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT {}
         FROM tracks_fts f
         JOIN tracks t ON t.id = f.rowid
         JOIN albums a ON a.id = t.album_id
         WHERE tracks_fts MATCH :query AND t.missing_ns IS NULL
         ORDER BY f.rank
         LIMIT :limit",
        *LIST_TRACK_SELECT
    ))?;
    let rows = stmt.query_map(
        named_params! { ":query": query, ":limit": limit },
        ListTrack::from_row,
    )?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// The ids of every track that matches `text`, the same way as [`search`].
/// Empty input matches nothing.
pub(crate) fn search_ids(conn: &Connection, text: &str) -> Result<HashSet<i64>> {
    let Some(query) = fts_query(text) else {
        return Ok(HashSet::new());
    };
    let mut stmt = conn.prepare(
        "SELECT f.rowid AS id FROM tracks_fts f JOIN tracks t ON t.id = f.rowid
         WHERE tracks_fts MATCH :query AND t.missing_ns IS NULL",
    )?;
    let rows = stmt.query_map(named_params! { ":query": query }, |row| row.get("id"))?;
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
