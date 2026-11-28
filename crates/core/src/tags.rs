//! Files are opened with `File::open` (read-only). Never call lofty write APIs here.

use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::Path;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, FileType, TaggedFile};
use lofty::mp4::{Mp4Codec, Mp4File};
use lofty::picture::{Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Tag;

use crate::{CoreError, Result};

/// File extensions the scanner picks up (lowercase). These are the formats
/// Symphonia can decode; Opus is out of scope for v1.
const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "m4b", "mp4", "aac", "wav", "ogg", "oga", "aif", "aiff",
];

pub fn is_audio_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            AUDIO_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub duration_ms: u64,
    /// Lowercase codec name, e.g. `mp3`, `flac`, `aac`, `alac`, `vorbis`.
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub art: Option<EmbeddedArt>,
}

/// A reference to embedded cover art. The image itself is not stored; it is
/// read from the file again when the UI needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedArt {
    /// blake3 hex digest of the image bytes, used to share art across tracks.
    pub hash: String,
    pub mime: Option<String>,
}

/// Bump when [`read_track_meta`] returns new or different data: the next scan
/// re-reads every track stored by an older version.
pub(crate) const READER_VERSION: i64 = 2;

pub fn read_track_meta(path: &Path) -> Result<TrackMeta> {
    let io_err = |source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    };
    let tag_err = |cause| CoreError::Tags {
        path: path.to_path_buf(),
        cause,
    };

    let file = File::open(path).map_err(io_err)?;
    let options = ParseOptions::new()
        .read_properties(true)
        .read_cover_art(true);
    let probe = Probe::new(BufReader::new(file))
        .options(options)
        .guess_file_type()
        .map_err(io_err)?;

    // The generic properties don't say whether an MP4 holds AAC or ALAC, so MP4
    // files are read through `Mp4File` to get the codec.
    let (tagged, mp4_codec) = if probe.file_type() == Some(FileType::Mp4) {
        let mut reader = probe.into_inner();
        reader.rewind().map_err(io_err)?;
        let mp4 = Mp4File::read_from(&mut reader, options).map_err(tag_err)?;
        let codec = mp4.properties().codec();
        (TaggedFile::from(mp4), codec)
    } else {
        (read_tagged(path, probe, options)?, None)
    };

    let props = tagged.properties();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());

    let text = |f: fn(&Tag) -> Option<String>| tag.and_then(f).filter(|s| !s.trim().is_empty());

    Ok(TrackMeta {
        title: text(|t| t.title().map(|s| s.into_owned())),
        artist: text(|t| t.artist().map(|s| s.into_owned())),
        album_artist: text(|t| t.get_string(ItemKey::AlbumArtist).map(str::to_owned)),
        album: text(|t| t.album().map(|s| s.into_owned())),
        track_no: tag.and_then(Accessor::track),
        disc_no: tag.and_then(Accessor::disk),
        year: tag
            .and_then(Accessor::date)
            .map(|d| i32::from(d.year))
            .filter(|y| *y > 0),
        genre: text(|t| t.genre().map(|s| s.into_owned())),
        duration_ms: u64::try_from(props.duration().as_millis()).unwrap_or(u64::MAX),
        codec: codec_name(tagged.file_type(), mp4_codec),
        sample_rate: props.sample_rate(),
        bitrate_kbps: props.audio_bitrate().filter(|&kbps| kbps > 0),
        art: tag.and_then(embedded_art),
    })
}

fn read_tagged(
    path: &Path,
    probe: Probe<BufReader<File>>,
    options: ParseOptions,
) -> Result<TaggedFile> {
    probe.read().or_else(|cause| {
        crate::id3_repair::read_repaired(path, options).ok_or_else(|| CoreError::Tags {
            path: path.to_path_buf(),
            cause,
        })
    })
}

fn codec_name(file_type: FileType, mp4_codec: Option<Mp4Codec>) -> String {
    let name = match (file_type, mp4_codec) {
        (FileType::Mp4, Some(Mp4Codec::AAC)) => "aac",
        (FileType::Mp4, Some(Mp4Codec::ALAC)) => "alac",
        (FileType::Mp4, Some(Mp4Codec::MP3)) => "mp3",
        (FileType::Mp4, Some(Mp4Codec::FLAC)) => "flac",
        (FileType::Mp4, _) => "mp4",
        (FileType::Mpeg, _) => "mp3",
        (FileType::Flac, _) => "flac",
        (FileType::Aac, _) => "aac",
        (FileType::Vorbis, _) => "vorbis",
        (FileType::Opus, _) => "opus",
        (FileType::Wav, _) => "wav",
        (FileType::Aiff, _) => "aiff",
        (other, _) => return format!("{other:?}").to_lowercase(),
    };
    name.to_owned()
}

/// The picture the library uses as the cover: the front cover, else the first.
fn cover(tag: &Tag) -> Option<&Picture> {
    let pictures = tag.pictures();
    pictures
        .iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures.first())
        .filter(|p| !p.data().is_empty())
}

/// The bytes of the file's cover image (the one [`TrackMeta::art`] refers to).
pub fn read_cover(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = File::open(path).map_err(|source| CoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let options = ParseOptions::new()
        .read_properties(false)
        .read_cover_art(true);
    let probe = Probe::new(BufReader::new(file))
        .options(options)
        .guess_file_type()
        .map_err(|source| CoreError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    let tagged = read_tagged(path, probe, options)?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    Ok(tag.and_then(cover).map(|p| p.data().to_vec()))
}

fn embedded_art(tag: &Tag) -> Option<EmbeddedArt> {
    let picture = cover(tag)?;
    Some(EmbeddedArt {
        hash: blake3::hash(picture.data()).to_hex().to_string(),
        mime: picture.mime_type().map(|m| m.as_str().to_owned()),
    })
}
