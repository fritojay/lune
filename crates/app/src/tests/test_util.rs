use std::path::{Path, PathBuf};

use lune_core::ListTrack;

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        let path = create_fresh_dir(&std::env::temp_dir(), "lune-app-test");
        // Canonical, like the paths the scanner stores.
        Self(std::fs::canonicalize(path).unwrap())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Writes a short silent WAV file at `rel`.
    pub fn wav(&self, rel: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
        std::fs::write(&path, wav_bytes(4410)).unwrap();
        path
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
        std::fs::write(&path, bytes).unwrap();
        path
    }

    pub fn remove_dir(&self, rel: &str) {
        #[expect(clippy::disallowed_methods, reason = "test fixture in a temp dir")]
        std::fs::remove_dir_all(self.0.join(rel)).unwrap();
    }

    /// Sets the mtime of `rel` to an hour ago, so a scan treats it as
    /// settled. Opening a directory to set its times works on Unix only.
    pub fn age(&self, rel: &str) {
        let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::open(self.0.join(rel))
            .unwrap()
            .set_modified(an_hour_ago)
            .unwrap();
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        #[expect(clippy::disallowed_methods, reason = "removes the test's own temp dir")]
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 16-bit mono PCM at 44.1 kHz, all zeros.
pub fn wav_bytes(frames: u32) -> Vec<u8> {
    let data = frames * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes()); // PCM
    b.extend_from_slice(&1u16.to_le_bytes()); // mono
    b.extend_from_slice(&44_100u32.to_le_bytes());
    b.extend_from_slice(&88_200u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    b.resize(b.len() + data as usize, 0);
    b
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

/// A track of album `album_id` ("Album N" by "A").
pub fn list_track(id: i64, album_id: i64, art_id: Option<i64>) -> ListTrack {
    ListTrack {
        id,
        path: PathBuf::from(format!("{id}.flac")),
        title: format!("Track {id}"),
        artist: None,
        album_id,
        album: format!("Album {album_id}"),
        album_key: format!("album {album_id}"),
        album_artist: "A".to_owned(),
        album_artist_key: "a".to_owned(),
        art_id,
        album_year: None,
        disc_no: None,
        track_no: None,
        duration_ms: 0,
        added_ns: None,
        played_ns: None,
    }
}

#[path = "test_util_test.rs"]
mod tests;
