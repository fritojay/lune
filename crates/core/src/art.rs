//! Cover thumbnails: the embedded cover is read from a track file (read-only),
//! downscaled, and cached as a JPEG named after the cover's hash, so every
//! album sharing a cover shares one file. Call from a background thread.

use std::io::Cursor;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ImageReader, Limits};
use rusqlite::{Connection, OptionalExtension};

use crate::{CoreError, Result};

/// Bounds what a malformed or hostile cover can make the decoder allocate.
const MAX_DIMENSION: u32 = 12_000;
const MAX_ALLOC: u64 = 256 * 1024 * 1024;
const JPEG_QUALITY: u8 = 85;
/// Tracks tried per cover before giving up: the art row outlives a file that
/// changed since the last scan, so the first file may no longer have it.
const CANDIDATES: u32 = 5;

/// The cached thumbnail of cover `art_id`, at most `size` pixels on its
/// longer side, made now if it isn't cached yet. `None` when the cover is
/// unknown or no track file still has it.
pub(crate) fn thumbnail(
    conn: &Connection,
    art_id: i64,
    dir: &Path,
    size: u32,
) -> Result<Option<PathBuf>> {
    thumbnail_with(conn, art_id, dir, size, crate::tags::read_cover)
}

fn thumbnail_with(
    conn: &Connection,
    art_id: i64,
    dir: &Path,
    size: u32,
    read_cover: impl Fn(&Path) -> Result<Option<Vec<u8>>>,
) -> Result<Option<PathBuf>> {
    let Some(hash) = conn
        .query_row("SELECT hash FROM art WHERE id = ?1", [art_id], |row| {
            row.get::<_, String>(0)
        })
        .optional()?
    else {
        return Ok(None);
    };
    // The hash becomes a file name; a database from elsewhere (`--db`) must
    // not be able to point it outside `dir`.
    if !is_cover_hash(&hash) {
        tracing::warn!(art_id, "cover hash is malformed; no thumbnail");
        return Ok(None);
    }
    let target = dir.join(format!("{hash}-{size}.jpg"));
    if target.is_file() {
        return Ok(Some(target));
    }

    let mut stmt =
        conn.prepare("SELECT path FROM tracks WHERE art_id = ?1 AND missing_ns IS NULL LIMIT ?2")?;
    let paths = stmt
        .query_map([art_id, i64::from(CANDIDATES)], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for path in paths.iter().map(PathBuf::from) {
        let bytes = match guarded(&path, || read_cover(&path)) {
            Ok(Some(bytes)) if blake3::hash(&bytes).to_hex().as_str() == hash => bytes,
            Ok(_) => continue,
            Err(err) => {
                tracing::debug!(%err, "skipping a cover candidate");
                continue;
            }
        };
        let jpeg = guarded(&path, || encode_thumbnail(&path, &bytes, size))?;
        crate::paths::write_own_file(&target, &jpeg)?;
        return Ok(Some(target));
    }
    Ok(None)
}

/// Deletes the thumbnails in `dir` (any size) of covers no longer in the
/// library. Only files named the way [`thumbnail`] names them are touched.
/// Returns how many were deleted.
pub(crate) fn prune_thumbnails(conn: &Connection, dir: &Path) -> Result<usize> {
    let io_err = |source| CoreError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(io_err(err)),
    };
    let mut known = conn.prepare("SELECT 1 FROM art WHERE hash = ?1")?;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(io_err)?;
        let Some(hash) = entry.file_name().to_str().and_then(thumbnail_hash) else {
            continue;
        };
        if !entry.file_type().is_ok_and(|t| t.is_file()) || known.exists([&hash])? {
            continue;
        }
        #[expect(
            clippy::disallowed_methods,
            reason = "deletes the app's own cached thumbnail, never a music file"
        )]
        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(err) => {
                tracing::warn!(%err, path = %entry.path().display(), "could not delete a stale thumbnail")
            }
        }
    }
    Ok(removed)
}

/// A blake3 digest as the scanner stores it: 64 lowercase hex digits.
fn is_cover_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The cover hash in a thumbnail file name (`<hash>-<size>.jpg`).
fn thumbnail_hash(name: &str) -> Option<String> {
    let (hash, size) = name.strip_suffix(".jpg")?.split_once('-')?;
    let size_ok = !size.is_empty() && size.bytes().all(|b| b.is_ascii_digit());
    (is_cover_hash(hash) && size_ok).then(|| hash.to_owned())
}

/// Turns a panic in a parser (lofty, image) into an error for this one file.
fn guarded<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(CoreError::ImagePanic(path.to_path_buf())))
}

fn encode_thumbnail(path: &Path, bytes: &[u8], size: u32) -> Result<Vec<u8>> {
    let image_err = |source| CoreError::Image {
        path: path.to_path_buf(),
        source,
    };
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    reader.limits(limits);
    let image = reader.decode().map_err(image_err)?;
    // `thumbnail` would also scale small covers up.
    let image = if image.width() > size || image.height() > size {
        image.thumbnail(size, size)
    } else {
        image
    };
    // JPEG has no alpha channel.
    let rgb = DynamicImage::ImageRgb8(image.to_rgb8());
    let mut out = Vec::new();
    rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY))
        .map_err(image_err)?;
    Ok(out)
}

#[cfg(test)]
#[path = "tests/art_test.rs"]
mod tests;
