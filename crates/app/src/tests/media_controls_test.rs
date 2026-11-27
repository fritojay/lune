use proptest::prelude::*;

use super::*;

#[test]
fn transport_events_map_to_commands() {
    assert_eq!(
        command_for(MediaControlEvent::Play),
        Some(MediaCommand::Play)
    );
    assert_eq!(
        command_for(MediaControlEvent::Pause),
        Some(MediaCommand::Pause)
    );
    assert_eq!(
        command_for(MediaControlEvent::Toggle),
        Some(MediaCommand::Toggle)
    );
    assert_eq!(
        command_for(MediaControlEvent::Next),
        Some(MediaCommand::Next)
    );
    assert_eq!(
        command_for(MediaControlEvent::Previous),
        Some(MediaCommand::Previous)
    );
    assert_eq!(
        command_for(MediaControlEvent::Stop),
        Some(MediaCommand::Stop)
    );
}

#[test]
fn seeks_become_signed_offsets_or_absolute_positions() {
    assert_eq!(
        command_for(MediaControlEvent::Seek(SeekDirection::Backward)),
        Some(MediaCommand::SeekBy(-5.0))
    );
    assert_eq!(
        command_for(MediaControlEvent::SeekBy(
            SeekDirection::Forward,
            Duration::from_millis(1500)
        )),
        Some(MediaCommand::SeekBy(1.5))
    );
    assert_eq!(
        command_for(MediaControlEvent::SetPosition(MediaPosition(
            Duration::from_secs(42)
        ))),
        Some(MediaCommand::Seek(Duration::from_secs(42)))
    );
}

#[test]
fn volume_is_clamped_and_nonsense_is_ignored() {
    assert_eq!(
        command_for(MediaControlEvent::SetVolume(1.7)),
        Some(MediaCommand::SetVolume(1.0))
    );
    assert_eq!(
        command_for(MediaControlEvent::SetVolume(-1.0)),
        Some(MediaCommand::SetVolume(0.0))
    );
    assert_eq!(command_for(MediaControlEvent::SetVolume(f64::NAN)), None);
}

#[test]
fn events_the_player_does_not_support_are_ignored() {
    assert_eq!(command_for(MediaControlEvent::Raise), None);
    assert_eq!(command_for(MediaControlEvent::Quit), None);
    assert_eq!(
        command_for(MediaControlEvent::OpenUri("file:///x.mp3".into())),
        None
    );
    assert_eq!(command_for(MediaControlEvent::SetShuffle(true)), None);
}

#[test]
fn cover_urls_are_encoded_only_where_they_are_uris() {
    for (path, linux, elsewhere) in [
        (
            "/home/u/.cache/lune/thumbnails/ab-320.jpg",
            "file:///home/u/.cache/lune/thumbnails/ab-320.jpg",
            "file:///home/u/.cache/lune/thumbnails/ab-320.jpg",
        ),
        (
            "/home/Zoë Smith/c#/100%.jpg",
            "file:///home/Zo%C3%AB%20Smith/c%23/100%25.jpg",
            "file:///home/Zoë Smith/c#/100%.jpg",
        ),
    ] {
        let expected = if cfg!(target_os = "linux") {
            linux
        } else {
            elsewhere
        };
        assert_eq!(
            cover_url(std::path::Path::new(path)).as_deref(),
            Some(expected)
        );
    }
}

fn percent_decoded(encoded: &str) -> Vec<u8> {
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
            decoded.push(u8::from_str_radix(hex, 16).unwrap());
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    decoded
}

proptest! {
    #[test]
    fn an_encoded_path_holds_only_uri_characters_and_decodes_back(path in "\\PC*") {
        let encoded = percent_encoded(&path);
        prop_assert!(encoded.bytes().all(|b| b.is_ascii_alphanumeric() || b"-._~/%".contains(&b)));
        prop_assert_eq!(percent_decoded(&encoded), path.as_bytes());
    }
}
