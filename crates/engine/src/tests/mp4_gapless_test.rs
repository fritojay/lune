use std::io::Cursor;

use proptest::prelude::*;

use super::*;

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = u32::try_from(8 + payload.len())
        .unwrap()
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);
    out
}

fn timescale_box(ts: u32) -> Vec<u8> {
    let mut p = vec![0u8; 12];
    p.extend_from_slice(&ts.to_be_bytes());
    p.extend_from_slice(&[0u8; 8]);
    p
}

fn smpb_moov(text: &str) -> Vec<u8> {
    let mut item = bx(b"mean", b"\0\0\0\0com.apple.iTunes");
    item.extend(bx(b"name", &[b"\0\0\0\0".as_slice(), b"iTunSMPB"].concat()));
    item.extend(bx(
        b"data",
        &[[0u8; 8].as_slice(), text.as_bytes()].concat(),
    ));
    let ilst = bx(b"ilst", &bx(b"----", &item));
    let meta = bx(b"meta", &[[0u8; 4].as_slice(), &ilst].concat());
    bx(b"moov", &bx(b"udta", &meta))
}

fn elst_moov(entries: &[(u32, i32)], count: u32) -> Vec<u8> {
    let mut elst = vec![0u8; 4];
    elst.extend_from_slice(&count.to_be_bytes());
    for (duration, media_time) in entries {
        elst.extend_from_slice(&duration.to_be_bytes());
        elst.extend_from_slice(&media_time.to_be_bytes());
        elst.extend_from_slice(&[0, 1, 0, 0]);
    }
    let mut hdlr = vec![0u8; 8];
    hdlr.extend_from_slice(b"soun");
    hdlr.extend_from_slice(&[0u8; 12]);
    let mut mdia = bx(b"hdlr", &hdlr);
    mdia.extend(bx(b"mdhd", &timescale_box(44_100)));
    let mut trak = bx(b"edts", &bx(b"elst", &elst));
    trak.extend(bx(b"mdia", &mdia));
    let mut moov = bx(b"mvhd", &timescale_box(1000));
    moov.extend(bx(b"trak", &trak));
    bx(b"moov", &moov)
}

/// `bytes`, a box with a 32-bit size, rewritten with a 64-bit one.
fn with_large_size(bytes: &[u8]) -> Vec<u8> {
    let mut out = 1u32.to_be_bytes().to_vec();
    out.extend_from_slice(&bytes[4..8]);
    out.extend_from_slice(&(bytes.len() as u64 + 8).to_be_bytes());
    out.extend_from_slice(&bytes[8..]);
    out
}

fn trim(bytes: &[u8]) -> Option<Mp4Trim> {
    let len = bytes.len() as u64;
    read_trim_from(&mut Cursor::new(bytes), len, 44_100).unwrap()
}

/// Reads `bytes` on a thread so a regression that loops shows up as a
/// failure, not a hung test. `None` if it panicked or took too long.
fn read_in_time(bytes: Vec<u8>, rate: u32) -> Option<std::io::Result<Option<Mp4Trim>>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let len = bytes.len() as u64;
        tx.send(read_trim_from(&mut Cursor::new(&bytes), len, rate))
    });
    rx.recv_timeout(std::time::Duration::from_secs(5)).ok()
}

#[test]
fn reads_itunsmpb_after_other_boxes_with_either_size_width() {
    let moov = smpb_moov(" 00000000 00000840 000001CA 00000000003F31F6 00000000");
    for moov in [with_large_size(&moov), moov] {
        let mut file = bx(b"ftyp", b"M4A \0\0\0\0");
        file.extend(with_large_size(&bx(b"free", &[0u8; 100])));
        file.extend(moov);
        let t = trim(&file).unwrap();
        assert_eq!(t.delay, 0x840);
        assert_eq!(t.valid_frames, Some(0x3F_31F6));
    }
}

#[test]
fn reads_an_ffmpeg_edit_list() {
    let t = trim(&elst_moov(&[(1000, 1024)], 1)).unwrap();
    assert_eq!(t.delay, 1024);
    assert_eq!(t.valid_frames, Some(44_100));
}

#[test]
fn skips_empty_edits() {
    let t = trim(&elst_moov(&[(500, -1), (1000, 2048)], 2)).unwrap();
    assert_eq!(t.delay, 2048);
}

#[test]
fn rejects_an_absurd_delay() {
    let file = smpb_moov(" 00000000 FFFFFFFFFFFFFFFF 00000000 FFFFFFFFFFFFFFFF");
    assert_eq!(trim(&file), None);
}

#[test]
fn an_edit_list_count_larger_than_its_entries_is_bounded() {
    // Claims 4 billion entries, has one empty edit: no trim, no hang.
    assert_eq!(trim(&elst_moov(&[(500, -1)], u32::MAX)), None);
}

#[test]
fn hostile_top_level_sizes_end_the_walk() {
    let mut wrap = bx(b"free", &[0u8; 8]);
    // 64-bit size that would wrap the offset back to 0.
    wrap.extend_from_slice(&1u32.to_be_bytes());
    wrap.extend_from_slice(b"free");
    wrap.extend_from_slice(&(u64::MAX - 15).to_be_bytes());
    let size_zero = [0u8, 0, 0, 0, b'f', b'r', b'e', b'e'];
    let too_small = [0u8, 0, 0, 4, b'f', b'r', b'e', b'e'];
    let past_end = bx(b"moov", &[0u8; 16])[..12].to_vec();
    let cut_large_size = [0u8, 0, 0, 1, b'm', b'o', b'o', b'v', 0, 0];
    for bytes in [
        wrap,
        size_zero.to_vec(),
        too_small.to_vec(),
        past_end,
        cut_large_size.to_vec(),
    ] {
        let result = read_in_time(bytes, 44_100).expect("returned in time");
        assert_eq!(result.unwrap(), None);
    }
}

#[test]
fn truncated_children_are_ignored() {
    assert_eq!(parse_elst(&[0, 0, 0], 1000, 44_100, 44_100), None);
    assert_eq!(children(&[0, 0, 0, 99, b'a', b'b', b'c', b'd']).count(), 0);
    assert_eq!(timescale(&[1, 0, 0]), None);
}

/// Bytes from anywhere: noise, and real tags (with 32- or 64-bit box sizes)
/// with bytes overwritten and the end cut off.
fn arb_hostile_file() -> impl Strategy<Value = Vec<u8>> {
    let smpb = smpb_moov(" 00000000 00000840 000001CA 00000000003F31F6 00000000");
    let elst = elst_moov(&[(500, -1), (1000, 2048)], 2);
    let real = prop::sample::select(vec![
        with_large_size(&smpb),
        with_large_size(&elst),
        smpb,
        elst,
    ]);
    let damaged = (
        real,
        prop::collection::vec((any::<prop::sample::Index>(), any::<u8>()), 0..8),
        // Often inside the first box's header, where the reads are.
        prop_oneof![
            0..24usize,
            any::<prop::sample::Index>().prop_map(|i| i.index(usize::MAX))
        ],
    )
        .prop_map(|(mut bytes, writes, cut)| {
            for (at, byte) in writes {
                let at = at.index(bytes.len());
                bytes[at] = byte;
            }
            bytes.truncate(cut);
            bytes
        });
    prop_oneof![damaged, prop::collection::vec(any::<u8>(), 0..256)]
}

proptest! {
    #[test]
    fn hostile_bytes_never_panic_or_fail_to_read(bytes in arb_hostile_file(), rate: u32) {
        let result = read_in_time(bytes, rate);
        prop_assert!(matches!(result, Some(Ok(_))), "{result:?}");
    }
}
