//! The schema version lives in `PRAGMA user_version`. Each entry in [`MIGRATIONS`]
//! upgrades the schema by one version and runs in its own transaction.

use std::path::Path;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::{CoreError, Result};

/// Migration `i` upgrades the schema from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &[
    // v1: initial schema.
    r#"
    CREATE TABLE art (
        id         INTEGER PRIMARY KEY,
        hash       TEXT NOT NULL UNIQUE,   -- blake3 of the embedded image bytes
        mime       TEXT,
        cache_path TEXT                    -- never used; dropped in v4
    );

    CREATE TABLE albums (
        id               INTEGER PRIMARY KEY,
        album_artist     TEXT NOT NULL,    -- display string (album artist, or track artist fallback)
        album_artist_key TEXT NOT NULL,
        title            TEXT NOT NULL,
        title_key        TEXT NOT NULL,
        year_max         INTEGER,          -- max track year; NULL = Unknown
        art_id           INTEGER REFERENCES art(id) ON DELETE SET NULL,
        UNIQUE (album_artist_key, title_key)
    );

    CREATE TABLE tracks (
        id           INTEGER PRIMARY KEY,
        path         TEXT NOT NULL UNIQUE,
        mtime_ns     INTEGER NOT NULL,
        size         INTEGER NOT NULL,
        title        TEXT,
        artist       TEXT,
        album_artist TEXT,                 -- the track's own album artist tag, if any
        album_id     INTEGER NOT NULL REFERENCES albums(id),
        disc_no      INTEGER,
        track_no     INTEGER,
        year         INTEGER,              -- the track's own year tag
        duration_ms  INTEGER NOT NULL,
        codec        TEXT NOT NULL,
        sample_rate  INTEGER,
        art_id       INTEGER REFERENCES art(id) ON DELETE SET NULL
    );
    CREATE INDEX tracks_album ON tracks(album_id, disc_no, track_no);
    CREATE INDEX tracks_art ON tracks(art_id);

    -- Full-text search over track and album fields. rowid = tracks.id.
    CREATE VIRTUAL TABLE tracks_fts USING fts5(
        title, artist, album_artist, album,
        tokenize = 'unicode61 remove_diacritics 2',
        prefix = '2 3'
    );
    CREATE TRIGGER tracks_fts_insert AFTER INSERT ON tracks BEGIN
        INSERT INTO tracks_fts(rowid, title, artist, album_artist, album)
        SELECT new.id, new.title, new.artist, a.album_artist, a.title
        FROM albums a WHERE a.id = new.album_id;
    END;
    CREATE TRIGGER tracks_fts_delete AFTER DELETE ON tracks BEGIN
        DELETE FROM tracks_fts WHERE rowid = old.id;
    END;
    CREATE TRIGGER tracks_fts_update AFTER UPDATE ON tracks BEGIN
        DELETE FROM tracks_fts WHERE rowid = old.id;
        INSERT INTO tracks_fts(rowid, title, artist, album_artist, album)
        SELECT new.id, new.title, new.artist, a.album_artist, a.title
        FROM albums a WHERE a.id = new.album_id;
    END;

    -- Queue: an ordered snapshot of track IDs. No foreign key on
    -- purpose: rescans must never alter the queue.
    CREATE TABLE queue (
        position INTEGER PRIMARY KEY,
        track_id INTEGER NOT NULL
    );
    CREATE TABLE queue_state (
        id     INTEGER PRIMARY KEY CHECK (id = 1),
        cursor INTEGER
    );
    "#,
    // v2: the track's own album tag, so album display strings can be
    // refreshed from the album's first track (rows from v1 stay NULL until
    // the file changes and is re-read).
    r#"
    ALTER TABLE tracks ADD COLUMN album TEXT;
    "#,
    // v3: shuffle. `queue.position` is the play order; while shuffled,
    // `original_position` is the unshuffled order. `cursor` is a position.
    r#"
    ALTER TABLE queue ADD COLUMN original_position INTEGER;
    ALTER TABLE queue_state ADD COLUMN shuffle INTEGER NOT NULL DEFAULT 0;
    "#,
    // v4: thumbnails are named by cover hash, so `art` needs no cache path.
    r#"
    ALTER TABLE art DROP COLUMN cache_path;
    "#,
    // v5: Recently Added and Recently Played. Existing rows get `added_ns`
    // from their file times on the next scan; a SQL migration can't stat.
    r#"
    ALTER TABLE tracks ADD COLUMN added_ns INTEGER;   -- earliest of the file's created and modified times
    ALTER TABLE tracks ADD COLUMN played_ns INTEGER;  -- when playback of it last started
    "#,
    // v6: files a scan no longer finds are kept as missing, so an unmounted
    // share doesn't wipe its tracks. `dirs` lets a scan skip
    // directories whose mtime hasn't changed.
    r#"
    ALTER TABLE tracks ADD COLUMN missing_ns INTEGER; -- when a scan first found it gone; NULL = present
    CREATE TABLE dirs (
        path     TEXT PRIMARY KEY,
        mtime_ns INTEGER NOT NULL                  -- 0 = never skip
    ) WITHOUT ROWID;
    "#,
    // v7: when each library folder was last scanned.
    r#"
    CREATE TABLE folder_scans (
        path       TEXT PRIMARY KEY,
        scanned_ns INTEGER NOT NULL
    ) WITHOUT ROWID;
    "#,
    // v8: bit rates. An mtime no file has (an unreadable one is stored as 0)
    // makes the next scan re-read every file to fill them in; the upsert keeps
    // each track's id and dates.
    r#"
    ALTER TABLE tracks ADD COLUMN bitrate_kbps INTEGER;
    UPDATE tracks SET mtime_ns = -1;
    UPDATE dirs SET mtime_ns = 0;
    "#,
    // v9: the tag reader version each track was read with (`READER_VERSION`
    // in tags.rs), so a scan re-reads tracks read by an older one.
    r#"
    ALTER TABLE tracks ADD COLUMN reader_version INTEGER NOT NULL DEFAULT 0;
    "#,
    // v10: genres. Reader version 2 reads them, so the next scan fills them in.
    r#"
    ALTER TABLE tracks ADD COLUMN genre TEXT;
    ALTER TABLE albums ADD COLUMN genre TEXT;  -- the first genre in album order
    "#,
    // v11: where in the current track playback resumes.
    r#"
    ALTER TABLE queue_state ADD COLUMN resume_ms INTEGER NOT NULL DEFAULT 0;
    "#,
];

pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| CoreError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
    enable_wal(conn)?;
    conn.execute_batch(
        "PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    Ok(())
}

/// Switching a new file to WAL needs an exclusive lock, and SQLite can report
/// "busy" for it without waiting on the busy timeout when another connection
/// (one of the app's threads) is opening the same file. So retry briefly.
fn enable_wal(conn: &Connection) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = conn
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0));
        match result {
            Ok(_) => return Ok(()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if err.code == rusqlite::ErrorCode::DatabaseBusy && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => return Err(err.into()),
        }
    }
}

fn migrate(conn: &Connection) -> Result<()> {
    let found: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let Ok(skip) = usize::try_from(found) else {
        return Err(CoreError::SchemaInvalid(found));
    };
    if found > SCHEMA_VERSION {
        return Err(CoreError::SchemaTooNew {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    for (version, sql) in MIGRATIONS.iter().enumerate().skip(skip) {
        let next = version as i64 + 1;
        // Several connections can open a new file at once (the app's
        // threads): take the write lock, then check nobody migrated meanwhile.
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if current >= next {
            conn.execute_batch("COMMIT")?;
            continue;
        }
        tracing::info!(from = version, to = next, "migrating library schema");
        let migrated = conn.execute_batch(&format!("{sql}; PRAGMA user_version = {next};"));
        if let Err(err) = migrated {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(err.into());
        }
        conn.execute_batch("COMMIT")?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/db_test.rs"]
mod tests;
