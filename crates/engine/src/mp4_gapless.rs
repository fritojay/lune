//! Symphonia's MP4 demuxer does not trim AAC priming or padding, so we read
//! it ourselves:
//!
//! 1. `iTunSMPB` (iTunes / Apple encoders): exact delay, padding, and length.
//! 2. Otherwise the audio track's edit list (`elst`, written by ffmpeg and
//!    others): exact delay, but the length is in the movie timescale (often
//!    milliseconds), so the end trim can be off by up to half a timescale unit.
//!
//! The file is opened read-only, and only the `moov` box is read.
//!
//! Everything here parses untrusted bytes: sizes and counts are bounded by
//! the data actually present, and arithmetic is checked.

#![warn(clippy::arithmetic_side_effects)]

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const MAX_MOOV_BYTES: u64 = 64 * 1024 * 1024;
/// Largest believable encoder delay. Real encoders use 1024 to 2624 frames;
/// anything far beyond that is a corrupt or hostile tag, so it's ignored.
const MAX_DELAY_FRAMES: u64 = 8 * 4096;

/// Trim for one AAC track, in frames at the track's sample rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp4Trim {
    /// Priming frames at the start of the decoded output to drop.
    pub delay: u64,
    /// Number of real frames after the delay; frames past this are padding.
    pub valid_frames: Option<u64>,
}

pub fn read_trim(path: &Path, sample_rate: u32) -> std::io::Result<Option<Mp4Trim>> {
    let file = File::open(path)?;
    let len = file.metadata()?.len();
    read_trim_from(&mut BufReader::new(file), len, sample_rate)
}

fn read_trim_from(
    file: &mut (impl Read + Seek),
    len: u64,
    sample_rate: u32,
) -> std::io::Result<Option<Mp4Trim>> {
    let Some(moov) = read_top_level_box(file, len, *b"moov")? else {
        return Ok(None);
    };
    let trim = itunsmpb(&moov).or_else(|| edit_list(&moov, sample_rate));
    Ok(trim.filter(|t| t.delay <= MAX_DELAY_FRAMES))
}

/// Box header: (type, header length, total length).
fn header(data: &[u8]) -> Option<([u8; 4], usize, usize)> {
    let size32 = u32::from_be_bytes(data.get(0..4)?.try_into().ok()?);
    let kind: [u8; 4] = data.get(4..8)?.try_into().ok()?;
    match size32 {
        0 => Some((kind, 8, data.len())),
        1 => {
            let size64 = u64::from_be_bytes(data.get(8..16)?.try_into().ok()?);
            Some((kind, 16, usize::try_from(size64).ok()?))
        }
        n => Some((kind, 8, usize::try_from(n).ok()?)),
    }
}

fn children(mut data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        let (kind, head, total) = header(data)?;
        if total < head || total > data.len() {
            return None;
        }
        let payload = data.get(head..total)?;
        data = data.get(total..)?;
        Some((kind, payload))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    children(data).find(|(k, _)| k == kind).map(|(_, p)| p)
}

/// Fills `buf`, or returns `false` if the stream ends first.
fn read_full(file: &mut impl Read, buf: &mut [u8]) -> std::io::Result<bool> {
    match file.read_exact(buf) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e),
    }
}

/// Finds the top-level box `want` in a stream of `len` bytes and returns its
/// payload. Stops (returning `None`) on any size that doesn't fit the stream.
fn read_top_level_box(
    file: &mut (impl Read + Seek),
    len: u64,
    want: [u8; 4],
) -> std::io::Result<Option<Vec<u8>>> {
    let mut offset = 0u64;
    while offset < len {
        file.seek(SeekFrom::Start(offset))?;
        let mut head = [0u8; 16];
        if !read_full(file, &mut head[..8])? {
            return Ok(None);
        }
        let size32 = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
        let (head_len, total) = match size32 {
            0 => return Ok(None), // box runs to EOF; moov is never like this
            1 => {
                if !read_full(file, &mut head[8..16])? {
                    return Ok(None);
                }
                let mut size = [0u8; 8];
                size.copy_from_slice(&head[8..16]);
                (16u64, u64::from_be_bytes(size))
            }
            n => (8u64, u64::from(n)),
        };
        // Every box must make progress and end inside the stream; this also
        // rules out sizes that would wrap `offset` around.
        let Some(end) = offset.checked_add(total) else {
            return Ok(None);
        };
        if total < head_len || end > len {
            return Ok(None);
        }
        if head[4..8] == want {
            if total > MAX_MOOV_BYTES {
                return Ok(None);
            }
            let Ok(payload_len) = usize::try_from(total.saturating_sub(head_len)) else {
                return Ok(None);
            };
            let mut payload = vec![0u8; payload_len];
            file.read_exact(&mut payload)?;
            return Ok(Some(payload));
        }
        offset = end;
    }
    Ok(None)
}

/// `moov/udta/meta/ilst/----` with name `iTunSMPB` (any `udta`, in case a
/// tool added a second one).
fn itunsmpb(moov: &[u8]) -> Option<Mp4Trim> {
    children(moov)
        .filter(|(kind, _)| kind == b"udta")
        .find_map(|(_, udta)| {
            let meta = child(udta, b"meta")?;
            // `meta` is a full box: 4 bytes of version/flags before its children.
            let ilst = child(meta.get(4..)?, b"ilst")?;
            children(ilst)
                .filter(|(kind, _)| kind == b"----")
                .find_map(|(_, item)| {
                    let name = child(item, b"name").and_then(|n| n.get(4..))?;
                    if name != b"iTunSMPB" {
                        return None;
                    }
                    // `data` payload: 4 bytes type + 4 bytes locale, then the text.
                    let text = child(item, b"data").and_then(|d| d.get(8..))?;
                    parse_itunsmpb(std::str::from_utf8(text).ok()?)
                })
        })
}

/// Parses " 00000000 00000840 000001CA 00000000003F31F6 ...": the second,
/// third, and fourth fields are delay, padding, and real length (hex).
fn parse_itunsmpb(text: &str) -> Option<Mp4Trim> {
    let mut fields = text.split_whitespace();
    let _ = fields.next()?;
    let delay = u64::from_str_radix(fields.next()?, 16).ok()?;
    let _padding = u64::from_str_radix(fields.next()?, 16).ok()?;
    let length = u64::from_str_radix(fields.next()?, 16).ok()?;
    Some(Mp4Trim {
        delay,
        valid_frames: (length > 0).then_some(length),
    })
}

fn edit_list(moov: &[u8], sample_rate: u32) -> Option<Mp4Trim> {
    let movie_timescale = timescale(child(moov, b"mvhd")?)?;
    for (kind, trak) in children(moov) {
        if &kind != b"trak" {
            continue;
        }
        let mdia = child(trak, b"mdia")?;
        let handler = child(mdia, b"hdlr").and_then(|h| h.get(8..12));
        if handler != Some(b"soun".as_slice()) {
            continue;
        }
        let media_timescale = timescale(child(mdia, b"mdhd")?)?;
        let elst = child(child(trak, b"edts")?, b"elst")?;
        return parse_elst(elst, movie_timescale, media_timescale, sample_rate);
    }
    None
}

fn timescale(full_box: &[u8]) -> Option<u32> {
    let (at, end) = if *full_box.first()? == 1 {
        (20, 24)
    } else {
        (12, 16)
    };
    Some(u32::from_be_bytes(full_box.get(at..end)?.try_into().ok()?))
}

fn parse_elst(
    elst: &[u8],
    movie_timescale: u32,
    media_timescale: u32,
    sample_rate: u32,
) -> Option<Mp4Trim> {
    if movie_timescale == 0 || media_timescale == 0 {
        return None;
    }
    let version = *elst.first()?;
    let count = usize::try_from(u32::from_be_bytes(elst.get(4..8)?.try_into().ok()?)).ok()?;
    let entry_len = if version == 1 { 20 } else { 12 };
    // `take(count)` bounds the loop by the entries actually present.
    for e in elst.get(8..)?.chunks_exact(entry_len).take(count) {
        let (duration, media_time) = if version == 1 {
            (
                u64::from_be_bytes(e[0..8].try_into().ok()?),
                i64::from_be_bytes(e[8..16].try_into().ok()?),
            )
        } else {
            (
                u64::from(u32::from_be_bytes(e[0..4].try_into().ok()?)),
                i64::from(i32::from_be_bytes(e[4..8].try_into().ok()?)),
            )
        };
        // media_time -1 is an empty edit (a pause before the track); skip it.
        if media_time < 0 {
            continue;
        }
        // u64 x u32 + u32 always fits in u128; `from` is non-zero (checked above).
        let rescale = |value: u64, from: u32| {
            let num = u128::from(value)
                .checked_mul(u128::from(sample_rate))?
                .checked_add(u128::from(from / 2))?;
            u64::try_from(num.checked_div(u128::from(from))?).ok()
        };
        let delay = rescale(u64::try_from(media_time).ok()?, media_timescale)?;
        let valid = rescale(duration, movie_timescale)?;
        return Some(Mp4Trim {
            delay,
            valid_frames: (valid > 0).then_some(valid),
        });
    }
    None
}

#[cfg(test)]
#[expect(clippy::arithmetic_side_effects, reason = "test fixtures")]
#[path = "tests/mp4_gapless_test.rs"]
mod tests;
