use super::*;

#[test]
fn a_negative_schema_version_is_rejected() {
    let conn = Connection::open_in_memory().unwrap();
    conn.pragma_update(None, "user_version", -1).unwrap();
    assert!(matches!(migrate(&conn), Err(CoreError::SchemaInvalid(-1))));
}

#[test]
fn a_newer_schema_is_rejected_and_migrations_reach_the_latest() {
    let conn = open_in_memory().unwrap();
    let v: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(v, SCHEMA_VERSION);
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(
        migrate(&conn),
        Err(CoreError::SchemaTooNew { .. })
    ));
}

#[test]
fn a_v1_database_with_data_upgrades() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "BEGIN; {}; PRAGMA user_version = 1; COMMIT;",
        MIGRATIONS[0]
    ))
    .unwrap();
    conn.execute_batch(
        "INSERT INTO albums (id, album_artist, album_artist_key, title, title_key)
             VALUES (1, 'A', 'a', 'T', 't');
         INSERT INTO tracks (path, mtime_ns, size, album_id, duration_ms, codec)
             VALUES ('/x.mp3', 0, 0, 1, 0, 'mp3');",
    )
    .unwrap();
    migrate(&conn).unwrap();
    let album: Option<String> = conn
        .query_row("SELECT album FROM tracks WHERE path = '/x.mp3'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(album, None, "v1 rows have no album tag until re-read");
}

#[test]
fn a_v3_database_drops_the_art_cache_path_and_keeps_its_covers() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "BEGIN; {}; {}; {}; PRAGMA user_version = 3; COMMIT;",
        MIGRATIONS[0], MIGRATIONS[1], MIGRATIONS[2]
    ))
    .unwrap();
    conn.execute(
        "INSERT INTO art (id, hash, mime, cache_path) VALUES (1, 'h', 'image/png', '/old.jpg')",
        [],
    )
    .unwrap();
    migrate(&conn).unwrap();
    let has_cache_path: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM pragma_table_info('art') WHERE name = 'cache_path'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!has_cache_path);
    let cover: (String, String) = conn
        .query_row("SELECT hash, mime FROM art WHERE id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(cover, ("h".to_string(), "image/png".to_string()));
}

#[test]
fn many_connections_can_open_a_new_database_at_once() {
    let dir = crate::test_util::TempDir::new();
    let path = dir.path().join("new.sqlite3");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (path, barrier) = (path.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                open(&path).map(|_| ())
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap().unwrap();
    }
    let conn = open(&path).unwrap();
    let v: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(v, SCHEMA_VERSION);
}

#[test]
fn a_v5_database_keeps_its_tracks_present() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(&format!(
        "BEGIN; {}; PRAGMA user_version = 5; COMMIT;",
        MIGRATIONS[..5].join(";")
    ))
    .unwrap();
    conn.execute_batch(
        "INSERT INTO albums (id, album_artist, album_artist_key, title, title_key)
         VALUES (1, 'A', 'a', 'T', 't');
         INSERT INTO tracks (path, mtime_ns, size, album_id, duration_ms, codec)
         VALUES ('/x.mp3', 0, 0, 1, 1000, 'mp3');",
    )
    .unwrap();
    migrate(&conn).unwrap();
    assert_eq!(crate::query::track_list(&conn).unwrap().len(), 1);
    assert!(crate::query::missing_tracks(&conn).unwrap().is_empty());
}

#[test]
fn a_v7_database_rereads_every_file_once_to_fill_in_bit_rates() {
    let dir = crate::test_util::TempDir::new();
    dir.write("1.wav", &crate::test_util::wav_bytes(1000));
    let mut conn = open_in_memory().unwrap();
    let quick = |conn: &mut Connection| {
        crate::scan::scan(conn, dir.path(), crate::ScanMode::Quick, &mut |_| {}).unwrap()
    };
    quick(&mut conn);
    let before = crate::query::track_list(&conn).unwrap()[0].clone();
    conn.execute_batch(
        "ALTER TABLE tracks DROP COLUMN bitrate_kbps;
         ALTER TABLE tracks DROP COLUMN reader_version;
         PRAGMA user_version = 7;",
    )
    .unwrap();

    migrate(&conn).unwrap();
    assert_eq!(
        crate::query::track_list(&conn).unwrap()[0].bitrate_kbps,
        None
    );
    let report = quick(&mut conn);
    assert_eq!((report.updated, report.dirs_skipped), (1, 0), "{report:?}");
    let after = crate::query::track_list(&conn).unwrap()[0].clone();
    assert_eq!(after, before);

    let report = quick(&mut conn);
    assert_eq!(report.updated, 0, "{report:?}");
}

#[test]
fn a_v8_database_rereads_every_file_once() {
    let dir = crate::test_util::TempDir::new();
    dir.write("1.wav", &crate::test_util::wav_bytes(1000));
    let mut conn = open_in_memory().unwrap();
    let quick = |conn: &mut Connection| {
        crate::scan::scan(conn, dir.path(), crate::ScanMode::Quick, &mut |_| {}).unwrap()
    };
    quick(&mut conn);
    conn.execute_batch("ALTER TABLE tracks DROP COLUMN reader_version; PRAGMA user_version = 8;")
        .unwrap();

    migrate(&conn).unwrap();
    assert_eq!(quick(&mut conn).updated, 1);
    assert_eq!(quick(&mut conn).updated, 0);
}
