use super::*;

fn track(title: &str, artist: Option<&str>, album: &str) -> MissingTrack {
    MissingTrack {
        id: 1,
        path: "/music/a.flac".into(),
        title: title.to_owned(),
        artist: artist.map(str::to_owned),
        album: album.to_owned(),
        missing_ns: 0,
    }
}

#[test]
fn placeholders_in_tags_are_shown_as_written() {
    let track = track("50%1 Off %3", Some("%2"), "100%");
    assert_eq!(label(&track), "50%1 Off %3 – %2 · 100%");
}

#[test]
fn a_missing_artist_or_album_gets_a_placeholder() {
    assert_eq!(
        label(&track("Song", None, "")),
        "Song – Unknown Artist · Unknown Album"
    );
}
