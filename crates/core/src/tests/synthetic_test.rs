use crate::{CoreError, Library};

#[test]
fn generates_exactly_the_requested_tracks_in_album_order() {
    let mut library = Library::open_in_memory().unwrap();
    library.generate_synthetic(1000).unwrap();
    let stats = library.stats().unwrap();
    assert_eq!(stats.tracks, 1000);
    assert!(stats.albums > 50, "{} albums", stats.albums);

    let list = library.track_list().unwrap();
    assert_eq!(list.len(), 1000);
    for pair in list.windows(2) {
        if pair[0].album_artist == pair[1].album_artist && pair[0].album == pair[1].album {
            let key = |t: &crate::ListTrack| (t.disc_no.unwrap_or(1), t.track_no);
            assert!(key(&pair[0]) < key(&pair[1]), "{pair:?}");
        }
    }
    assert!(
        list.iter().any(|t| t.title.ends_with(".flac")),
        "some titles missing"
    );
    assert!(
        list.iter().any(|t| t.disc_no == Some(2)),
        "some albums have two discs"
    );
}

#[test]
fn zero_tracks_leaves_the_library_empty() {
    let mut library = Library::open_in_memory().unwrap();
    library.generate_synthetic(0).unwrap();
    assert_eq!(library.stats().unwrap().tracks, 0);
}

#[test]
fn a_non_empty_library_is_refused() {
    let mut library = Library::open_in_memory().unwrap();
    library.generate_synthetic(3).unwrap();
    assert!(matches!(
        library.generate_synthetic(3),
        Err(CoreError::NotEmpty(3))
    ));
    assert_eq!(library.stats().unwrap().tracks, 3);
}
