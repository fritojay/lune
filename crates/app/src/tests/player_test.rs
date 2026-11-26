use std::path::PathBuf;

use lune_core::Grouping;
use proptest::prelude::*;

use super::*;
use crate::test_util::list_track;

fn queued(track_ids: &[i64]) -> Vec<QueueItem> {
    (0..)
        .zip(track_ids)
        .map(|(slot, &track_id)| QueueItem {
            slot: SlotId(slot),
            track_id,
            path: PathBuf::from(format!("queued {track_id}")),
        })
        .collect()
}

proptest! {
    #[test]
    fn a_selection_is_its_rows_in_list_order_once_each_without_rows_past_the_end(
        albums in prop::collection::vec(1..4i64, 0..6),
        queue in prop::collection::vec(0..9i64, 0..6),
        source in prop::sample::select(vec!["view", "queue"]),
        rows in prop::collection::vec(prop_oneof![-2..9i32, Just(i32::MAX)], 0..8),
    ) {
        let catalog = Catalog::new(
            (1..).zip(&albums).map(|(id, &album)| list_track(id, album, None)).collect(),
        );
        let view = catalog.view(Grouping::AlbumArtist, &lune_core::TrackFilter::All);
        let queue = queued(&queue);
        let selection = Selection::new(source, rows.iter().copied()).unwrap();
        let rows = |row: usize| rows.iter().any(|&r| usize::try_from(r) == Ok(row));
        let expected: Vec<TrackRef> = match source {
            "view" => (0..)
                .zip(&view.tracks)
                .filter(|(row, _)| rows(*row))
                .map(|(_, &i)| {
                    let track = catalog.track(i).unwrap();
                    TrackRef { id: track.id, path: track.path.clone() }
                })
                .collect(),
            _ => (0..)
                .zip(&queue)
                .filter(|(row, _)| rows(*row))
                .map(|(_, item)| TrackRef { id: item.track_id, path: item.path.clone() })
                .collect(),
        };
        prop_assert_eq!(selection.tracks(&catalog, &view, &queue), expected);
    }
}

#[test]
fn an_unknown_source_selects_nothing() {
    assert_eq!(Selection::new("playlist", [0]), None);
    assert_eq!(Selection::new("", [0]), None);
}

#[test]
fn every_track_list_qml_names_is_a_selection_source() {
    let panes = [
        include_str!("../../qml/TrackPane.qml"),
        include_str!("../../qml/QueuePane.qml"),
    ];
    let menus = panes
        .iter()
        .flat_map(|qml| qml.lines())
        .filter_map(|line| line.trim().strip_prefix("source: \""));
    let plays = include_str!("../../qml/main.qml")
        .split("player.play(\"")
        .skip(1);
    let names: Vec<&str> = menus
        .chain(plays)
        .map(|rest| &rest[..rest.find('"').unwrap()])
        .collect();
    assert_eq!(names, ["view", "queue", "queue", "view"]);
    for name in names {
        assert!(Selection::new(name, [0]).is_some(), "{name}");
    }
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

fn status(
    state: PlaybackState,
    entry: Option<u64>,
    output_failed: bool,
    preferred_back: bool,
) -> Status {
    Status {
        state,
        entry: entry.map(EntryId),
        position: Duration::from_secs(42),
        duration: None,
        underruns: 0,
        output_failed,
        preferred_back,
        output_rate: 48_000,
        output_channels: 2,
    }
}

#[test]
fn the_output_is_opened_again_when_missing_failed_or_its_preferred_device_is_back() {
    let now = Instant::now();
    let fine = status(PlaybackState::Playing, Some(1), false, false);
    let failed = status(PlaybackState::Playing, Some(1), true, false);
    let back = status(PlaybackState::Paused, Some(1), false, true);
    for (status, last_open, due) in [
        (None, None, true),
        (None, Some(now), false),
        (None, Some(now - REOPEN_EVERY), true),
        (Some(&fine), Some(now - REOPEN_EVERY), false),
        (Some(&failed), Some(now - REOPEN_EVERY), true),
        (Some(&failed), Some(now), false),
        (Some(&back), Some(now - REOPEN_EVERY), true),
    ] {
        assert_eq!(
            reopen_due(status, last_open, now),
            due,
            "{status:?} {last_open:?}"
        );
    }
}

#[test]
fn a_new_output_goes_on_from_the_entry_heard_and_starts_it_only_if_it_was_playing() {
    let at = Duration::from_secs(42);
    for (state, entry, resume) in [
        (
            PlaybackState::Playing,
            Some(3),
            Some((EntryId(3), at, true)),
        ),
        (
            PlaybackState::Paused,
            Some(3),
            Some((EntryId(3), at, false)),
        ),
        (PlaybackState::Stopped, None, None),
        (PlaybackState::Playing, None, None),
    ] {
        assert_eq!(
            resume_from(&status(state, entry, false, false)),
            resume,
            "{state:?}"
        );
    }
}
