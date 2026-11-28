use super::*;
use crate::tags::{read_cover, read_track_meta};
use crate::test_util::TempDir;
use proptest::prelude::*;

const COVER: &[u8] = b"not really a jpeg";

/// MPEG-1 Layer III frames at 128 kbps and 44.1 kHz, enough for lofty to find
/// the audio and estimate a duration.
fn audio() -> Vec<u8> {
    let mut frame = vec![0; 417];
    frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x64]);
    frame.repeat(20)
}

fn frame(major: u8, id: &[u8; 4], format_flags: u8, body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(body.len()).unwrap();
    let mut frame = id.to_vec();
    frame.extend(match major {
        3 => size.to_be_bytes(),
        _ => to_syncsafe(size),
    });
    frame.extend([0, format_flags]);
    frame.extend(body);
    frame
}

/// A UTF-16 text frame body; `stray` adds the extra null some taggers write
/// after the terminator.
fn utf16(encoding: u8, text: &str, stray: bool) -> Vec<u8> {
    let mut body = vec![encoding];
    if encoding == UTF16_WITH_BOM {
        body.extend([0xFE, 0xFF]);
    }
    body.extend(text.encode_utf16().flat_map(u16::to_be_bytes));
    body.extend([0, 0]);
    if stray {
        body.push(0);
    }
    body
}

fn extended_header(major: u8) -> Vec<u8> {
    match major {
        3 => vec![0, 0, 0, 6, 0, 0, 0, 0, 0, 0],
        _ => vec![0, 0, 0, 6, 1, 0],
    }
}

fn tag(major: u8, flags: u8, frames: &[Vec<u8>], padding: usize) -> Vec<u8> {
    let mut body = Vec::new();
    if flags & EXTENDED_HEADER != 0 {
        body.extend(extended_header(major));
    }
    body.extend(frames.concat());
    body.resize(body.len() + padding, 0);
    let mut tag = vec![b'I', b'D', b'3', major, 0, flags];
    tag.extend(to_syncsafe(u32::try_from(body.len()).unwrap()));
    tag.extend(body);
    tag
}

fn mp3(tag: Vec<u8>) -> Vec<u8> {
    [tag, audio()].concat()
}

fn picture_frame(major: u8) -> Vec<u8> {
    let body = [&[0][..], b"image/jpeg\0", &[3, 0], COVER].concat();
    frame(major, b"APIC", 0, &body)
}

/// Shaped like the files in lune-871: every UTF-16 frame has the stray null.
fn odd_length_tag(major: u8, flags: u8) -> Vec<u8> {
    let frames = [
        frame(
            major,
            b"TIT2",
            0,
            &utf16(UTF16_WITH_BOM, "VIPs (Demo)", true),
        ),
        frame(
            major,
            b"TPE1",
            0,
            &utf16(UTF16_WITH_BOM, "MUST DIE! & Skrillex", true),
        ),
        frame(
            major,
            b"TALB",
            0,
            &utf16(UTF16_WITH_BOM, "Unreleased", true),
        ),
        frame(major, b"TRCK", 0, b"\x0032/37\0"),
        picture_frame(major),
    ];
    tag(major, flags, &frames, 64)
}

#[test]
fn a_file_with_odd_length_utf16_text_frames_is_read_with_its_tags_and_cover() {
    let dir = TempDir::new();
    for major in [3, 4] {
        let path = dir.write(&format!("v2.{major}.mp3"), &mp3(odd_length_tag(major, 0)));

        let meta = read_track_meta(&path).unwrap();
        assert_eq!(meta.title.as_deref(), Some("VIPs (Demo)"), "ID3v2.{major}");
        assert_eq!(meta.artist.as_deref(), Some("MUST DIE! & Skrillex"));
        assert_eq!(meta.album.as_deref(), Some("Unreleased"));
        assert_eq!(meta.track_no, Some(32));
        assert_eq!(meta.codec, "mp3");
        assert!(meta.duration_ms > 0);
        assert!(meta.art.is_some());
        assert_eq!(read_cover(&path).unwrap().as_deref(), Some(COVER));
    }
}

#[test]
fn a_tag_that_cannot_be_repaired_fails_with_its_cause_shown_once() {
    let dir = TempDir::new();
    let unrepairable = [UNSYNCHRONISATION, FOOTER];
    for flags in unrepairable {
        let file = mp3(odd_length_tag(3, flags));
        assert_eq!(repair(&file), None, "flags {flags:#x}");

        let path = dir.write("unrepairable.mp3", &file);
        let err = read_track_meta(&path).unwrap_err();
        let mut chain = err.to_string();
        let mut source = std::error::Error::source(&err);
        while let Some(cause) = source {
            chain.push_str(&format!(": {cause}"));
            source = cause.source();
        }
        assert_eq!(
            chain.matches("UTF-16 string has an odd length").count(),
            1,
            "flags {flags:#x}: {chain}"
        );
    }
}

#[test]
fn frames_that_are_not_plain_utf16_text_are_left_alone() {
    let odd_but_not_repaired = [
        (
            3,
            frame(3, b"TIT2", 0x80, &utf16(UTF16_WITH_BOM, "compressed", true)),
        ),
        (
            4,
            frame(
                4,
                b"TIT2",
                0x02,
                &utf16(UTF16_WITH_BOM, "unsynchronised", true),
            ),
        ),
        (
            3,
            frame(
                3,
                b"COMM",
                0,
                &utf16(UTF16_WITH_BOM, "not a text frame", true),
            ),
        ),
        (3, frame(3, b"TIT2", 0, b"\x00Latin-1 is any length")),
        (3, frame(3, b"TIT2", 0, b"\x03UTF-8 too")),
    ];
    for (major, frame) in odd_but_not_repaired {
        assert_eq!(repair(&mp3(tag(major, 0, &[frame], 8))), None);
    }
}

#[test]
fn files_without_a_tag_this_rewrites_are_not_repaired() {
    let not_repaired = [
        Vec::new(),
        audio(),
        b"ID3".to_vec(),
        mp3(odd_length_tag(2, 0)),
        // lofty reads no frames after an ID3v2.3 extended header.
        mp3(odd_length_tag(3, EXTENDED_HEADER)),
        // A tag size that runs past the end of the file.
        [&b"ID3\x03\x00\x00\x7f\x7f\x7f\x7f"[..], &audio()].concat(),
        // A tag size that isn't syncsafe.
        [&b"ID3\x03\x00\x00\x00\x00\x00\x80"[..], &audio()].concat(),
    ];
    for file in not_repaired {
        assert_eq!(repair(&file), None);
    }
}

fn arb_text_frame() -> impl Strategy<Value = (u8, String, bool)> {
    (
        prop_oneof![Just(UTF16_WITH_BOM), Just(UTF16_BE)],
        any::<String>(),
        any::<bool>(),
    )
}

proptest! {
    #[test]
    fn a_repair_is_the_tag_written_correctly_with_the_dropped_bytes_as_padding(
        major in 3u8..=4,
        extended in any::<bool>(),
        frames in prop::collection::vec(arb_text_frame(), 0..6),
        padding in 0usize..32,
    ) {
        let flags = if extended && major == 4 { EXTENDED_HEADER } else { 0 };
        let build = |stray: &dyn Fn(bool) -> bool| -> Vec<Vec<u8>> {
            frames
                .iter()
                .map(|(encoding, text, odd)| {
                    frame(major, b"TIT2", 0, &utf16(*encoding, text, stray(*odd)))
                })
                .chain([picture_frame(major)])
                .collect()
        };
        let odd = frames.iter().filter(|(_, _, odd)| *odd).count();
        let file = mp3(tag(major, flags, &build(&|odd| odd), padding));

        let expected = (odd > 0).then(|| tag(major, flags, &build(&|_| false), padding + odd));
        prop_assert_eq!(repair(&file), expected);
    }

    #[test]
    fn any_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..256)) {
        repair(&bytes);
    }

    #[test]
    fn any_bytes_after_an_id3v2_header_never_panic(
        major in 3u8..=4,
        flags in any::<u8>(),
        size in prop::array::uniform4(0u8..0x80),
        bytes in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let file = [&[b'I', b'D', b'3', major, 0, flags][..], &size, &bytes].concat();
        if let Some(repaired) = repair(&file) {
            prop_assert_eq!(Some(repaired.len()), tag_len(&file));
        }
    }
}

#[derive(Debug, Clone)]
enum Op {
    Read(usize),
    Seek(SeekFrom),
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0usize..64).prop_map(Op::Read),
        (0u64..80).prop_map(|n| Op::Seek(SeekFrom::Start(n))),
        (-80i64..80).prop_map(|n| Op::Seek(SeekFrom::Current(n))),
        (-80i64..8).prop_map(|n| Op::Seek(SeekFrom::End(n))),
    ]
}

proptest! {
    #[test]
    fn a_spliced_file_reads_and_seeks_like_the_head_followed_by_the_rest_of_the_file(
        (file, head) in prop::collection::vec(any::<u8>(), 0..64).prop_flat_map(|file| {
            let len = file.len();
            (Just(file), prop::collection::vec(any::<u8>(), 0..=len))
        }),
        ops in prop::collection::vec(arb_op(), 0..20),
    ) {
        let dir = TempDir::new();
        let path = dir.write("file", &file);
        let mut on_disk = File::open(&path).unwrap();
        on_disk.seek(SeekFrom::Start(head.len() as u64)).unwrap();
        let mut spliced = Spliced::new(head.clone(), on_disk);
        let mut model = io::Cursor::new([&head[..], &file[head.len()..]].concat());

        for op in ops {
            match op {
                Op::Read(n) => {
                    let (mut got, mut want) = (vec![0; n], vec![0; n]);
                    let got_n = spliced.read_exact(&mut got).map(|()| got);
                    let want_n = model.read_exact(&mut want).map(|()| want);
                    prop_assert_eq!(got_n.ok(), want_n.ok(), "{:?}", op);
                    // After a short read_exact the position is unspecified.
                    let pos = model.stream_position().unwrap();
                    spliced.seek(SeekFrom::Start(pos)).unwrap();
                }
                Op::Seek(from) => {
                    prop_assert_eq!(spliced.seek(from).ok(), model.seek(from).ok(), "{:?}", op);
                }
            }
        }
    }
}
