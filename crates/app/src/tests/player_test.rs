use lune_core::Grouping;

use super::*;
use crate::test_util::list_track;

fn ids(tracks: &[(i64, PathBuf)]) -> Vec<i64> {
    tracks.iter().map(|(id, _)| *id).collect()
}

#[test]
fn view_tracks_takes_a_range_of_the_view_in_order() {
    let catalog = Catalog::new(vec![
        list_track(1, 1, None),
        list_track(2, 1, None),
        list_track(3, 2, None),
        list_track(4, 2, None),
    ]);
    let view = catalog.view(Grouping::AlbumArtist, &lune_core::TrackFilter::All);
    assert_eq!(
        ids(&view_tracks(&catalog, &view, 0, usize::MAX)),
        [1, 2, 3, 4]
    );
    // An album: its first index and track count.
    assert_eq!(ids(&view_tracks(&catalog, &view, 2, 2)), [3, 4]);
    assert_eq!(
        view_tracks(&catalog, &view, 1, 1)[0].1,
        PathBuf::from("2.flac")
    );
}

#[test]
fn view_tracks_clamps_to_the_view() {
    let catalog = Catalog::new(vec![list_track(1, 1, None), list_track(2, 1, None)]);
    let view = catalog.view(Grouping::AlbumArtist, &lune_core::TrackFilter::All);
    assert_eq!(ids(&view_tracks(&catalog, &view, 1, 10)), [2]);
    assert!(view_tracks(&catalog, &view, 5, 1).is_empty());
    assert!(view_tracks(&catalog, &view, 0, 0).is_empty());
    assert!(view_tracks(&catalog, &View::default(), 0, usize::MAX).is_empty());
}

fn unmuted(level: f64) -> Volume {
    Volume {
        level,
        muted: false,
    }
}

#[test]
fn muting_silences_output_and_unmuting_restores_the_level() {
    let mut volume = unmuted(0.6);
    volume.toggle_mute();
    assert_eq!(volume.output(), 0.0);
    volume.toggle_mute();
    assert_eq!(volume, unmuted(0.6));
    assert_eq!(volume.output(), 0.6);
}

#[test]
fn choosing_a_level_while_muted_unmutes_at_that_level() {
    let mut volume = unmuted(0.6);
    volume.toggle_mute();
    volume.set_level(0.3);
    assert_eq!(volume, unmuted(0.3));
    assert_eq!(volume.output(), 0.3);
}

#[test]
fn muting_at_zero_stays_silent_either_way() {
    let mut volume = unmuted(0.0);
    volume.toggle_mute();
    assert_eq!(volume.output(), 0.0);
    volume.toggle_mute();
    assert_eq!(volume, unmuted(0.0));
}

#[test]
fn levels_are_clamped_and_a_non_finite_level_is_full_volume() {
    let mut volume = unmuted(0.5);
    volume.set_level(1.5);
    assert_eq!(volume, unmuted(1.0));
    volume.set_level(-0.2);
    assert_eq!(volume, unmuted(0.0));
    volume.set_level(f64::NAN);
    assert_eq!(volume, unmuted(1.0));
    volume.set_level(f64::NEG_INFINITY);
    assert_eq!(volume, unmuted(1.0));
}
