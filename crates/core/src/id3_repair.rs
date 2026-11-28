//! Recovers ID3v2 tags that lofty rejects but other readers accept, by
//! repairing a copy of the tag in memory. The file itself is only read.

use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::TaggedFile;
use lofty::probe::Probe;

const HEADER_LEN: usize = 10;
const FRAME_HEADER_LEN: usize = 10;
const UNSYNCHRONISATION: u8 = 0x80;
const EXTENDED_HEADER: u8 = 0x40;
const FOOTER: u8 = 0x10;
const UTF16_WITH_BOM: u8 = 1;
const UTF16_BE: u8 = 2;

/// Reads `path` with its tag repaired, for when lofty can't read the file
/// itself. `None` when there is nothing to repair or lofty can't read the
/// repaired file either. Only the tag is read into memory.
pub(crate) fn read_repaired(path: &Path, options: ParseOptions) -> Option<TaggedFile> {
    let mut file = File::open(path).ok()?;
    let mut header = [0; HEADER_LEN];
    file.read_exact(&mut header).ok()?;
    let tag_len = tag_len(&header)?;
    let mut tag = header.to_vec();
    // `take` rather than a buffer of `tag_len`: a size that runs past the end
    // of the file allocates no more than the file holds.
    (&mut file)
        .take(u64::try_from(tag_len - HEADER_LEN).ok()?)
        .read_to_end(&mut tag)
        .ok()?;
    let repaired = repair(&tag)?;
    let tagged = Probe::new(BufReader::new(Spliced::new(repaired, file)))
        .options(options)
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;
    tracing::debug!(path = %path.display(), "read tags after repairing odd-length UTF-16 text frames");
    Some(tagged)
}

/// The length of the ID3v2.3 or 2.4 tag `header` starts, header included,
/// when it's one [`repair`] can rewrite: not with tag-wide
/// unsynchronisation or a footer, and not ID3v2.3 with an extended header,
/// which lofty 0.25 misreads as the 2.4 layout and so reads no frames from
/// (a repair could never help).
fn tag_len(header: &[u8]) -> Option<usize> {
    let [b'I', b'D', b'3', major @ (3 | 4), _, flags, a, b, c, d] = *header.get(..HEADER_LEN)?
    else {
        return None;
    };
    if flags & (UNSYNCHRONISATION | FOOTER) != 0 || (major == 3 && flags & EXTENDED_HEADER != 0) {
        return None;
    }
    HEADER_LEN.checked_add(usize::try_from(syncsafe([a, b, c, d])?).ok()?)
}

/// The ID3v2 tag at the start of `file`, with the leftover byte dropped from
/// each UTF-16 text frame that has an odd length, which lofty rejects (some
/// taggers write a 3-byte null terminator). The dropped bytes become padding,
/// so the tag keeps its size and the audio its offset. `None` when no frame
/// needs it, or the tag is one this doesn't rewrite (see [`tag_len`]).
pub(crate) fn repair(file: &[u8]) -> Option<Vec<u8>> {
    let tag_end = tag_len(file)?;
    let tag = file.get(..tag_end)?;
    let major = tag[3];
    let frame_size = |bytes: [u8; 4]| match major {
        3 => Some(u32::from_be_bytes(bytes)),
        _ => syncsafe(bytes),
    };

    let mut pos = HEADER_LEN;
    if tag[5] & EXTENDED_HEADER != 0 {
        // Only ID3v2.4 gets here; its size counts the size field itself.
        pos = pos.checked_add(usize::try_from(syncsafe(size_at(tag, pos)?)?).ok()?)?;
    }

    let mut out = Vec::with_capacity(tag_end);
    out.extend_from_slice(tag.get(..pos)?);
    let mut repaired = false;
    while let Some(frame_header) = tag.get(pos..pos + FRAME_HEADER_LEN)
        && frame_header[0] != 0
    {
        let size = usize::try_from(frame_size(size_at(frame_header, 4)?)?).ok()?;
        let body_start = pos + FRAME_HEADER_LEN;
        let body = tag.get(body_start..body_start.checked_add(size)?)?;
        let format_flags = frame_header[9];
        let body = match body {
            [UTF16_WITH_BOM | UTF16_BE, text @ ..]
                if frame_header[0] == b'T' && format_flags == 0 && text.len() % 2 == 1 =>
            {
                repaired = true;
                &body[..size - 1]
            }
            _ => body,
        };
        let new_size = u32::try_from(body.len()).ok()?;
        out.extend_from_slice(&frame_header[..4]);
        out.extend_from_slice(&match major {
            3 => new_size.to_be_bytes(),
            _ => to_syncsafe(new_size),
        });
        out.extend_from_slice(&frame_header[8..]);
        out.extend_from_slice(body);
        pos = body_start + size;
    }
    if !repaired {
        return None;
    }
    out.resize(tag_end, 0);
    Some(out)
}

/// `file` read with its first `head.len()` bytes replaced by `head`. `file`
/// is kept at `max(pos, head.len())`, so reads past the head need no seek.
struct Spliced {
    head: Vec<u8>,
    file: File,
    pos: u64,
}

impl Spliced {
    /// `file` must be at `head.len()`.
    fn new(head: Vec<u8>, file: File) -> Self {
        Self { head, file, pos: 0 }
    }

    fn head_len(&self) -> u64 {
        u64::try_from(self.head.len()).unwrap_or(u64::MAX)
    }
}

impl Read for Spliced {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let in_head = usize::try_from(self.pos)
            .ok()
            .and_then(|pos| self.head.get(pos..))
            .filter(|rest| !rest.is_empty());
        let n = match in_head {
            Some(rest) => {
                let n = rest.len().min(buf.len());
                buf[..n].copy_from_slice(&rest[..n]);
                n
            }
            None => self.file.read(buf)?,
        };
        self.pos += u64::try_from(n).unwrap_or(u64::MAX);
        Ok(n)
    }
}

impl Seek for Spliced {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        // The head is as long as the bytes it replaces, so the file's end is
        // this reader's end.
        let pos = match from {
            SeekFrom::Start(pos) => pos,
            SeekFrom::End(_) => self.file.seek(from)?,
            SeekFrom::Current(by) => self.pos.checked_add_signed(by).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "seek before the start")
            })?,
        };
        self.file.seek(SeekFrom::Start(pos.max(self.head_len())))?;
        self.pos = pos;
        Ok(pos)
    }
}

fn size_at(bytes: &[u8], at: usize) -> Option<[u8; 4]> {
    bytes.get(at..at.checked_add(4)?)?.try_into().ok()
}

fn syncsafe(bytes: [u8; 4]) -> Option<u32> {
    bytes
        .iter()
        .try_fold(0u32, |n, &b| (b < 0x80).then_some((n << 7) | u32::from(b)))
}

/// `n` must fit in 28 bits; callers pass a size no larger than one read with
/// [`syncsafe`].
fn to_syncsafe(n: u32) -> [u8; 4] {
    [21, 14, 7, 0].map(|shift| ((n >> shift) & 0x7f) as u8)
}

#[cfg(test)]
#[path = "tests/id3_repair_test.rs"]
mod tests;
