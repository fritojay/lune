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
fn cover_urls_are_plain_file_urls() {
    assert_eq!(
        cover_url(std::path::Path::new(
            "/home/u/.cache/lune/thumbnails/ab-320.jpg"
        ))
        .as_deref(),
        Some("file:///home/u/.cache/lune/thumbnails/ab-320.jpg")
    );
}
