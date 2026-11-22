use super::*;
use crate::SortColumn;
use crate::test_util::TempDir;

fn custom() -> WindowState {
    WindowState {
        width: 1000,
        height: 700,
        maximized: true,
        filter_pane_width: 320,
        queue_pane_width: 410,
        queue_shown: false,
        hidden_columns: [SortColumn::FileType, SortColumn::BitRate].into(),
        browse: BrowseState {
            grouping: Grouping::Artist,
            selected: FilterValue::Artist("radiohead".into()),
            sort: Some(Sort {
                column: SortColumn::Duration,
                descending: true,
            }),
        },
    }
}

fn browsing(selected: FilterValue) -> WindowState {
    WindowState {
        browse: BrowseState {
            grouping: Grouping::Year,
            selected,
            sort: None,
        },
        ..WindowState::default()
    }
}

fn round_trip(state: &WindowState) -> WindowState {
    let dir = TempDir::new();
    let path = dir.path().join("window.toml");
    state.save(&path).unwrap();
    WindowState::load(&path).unwrap()
}

#[test]
fn a_missing_file_gives_the_defaults() {
    let dir = TempDir::new();
    assert_eq!(
        WindowState::load(&dir.path().join("window.toml")).unwrap(),
        WindowState::default()
    );
}

#[test]
fn the_state_survives_a_save_and_load_and_creates_the_folder() {
    let dir = TempDir::new();
    let path = dir.path().join("nested/window.toml");
    custom().save(&path).unwrap();
    assert_eq!(WindowState::load(&path).unwrap(), custom());
    let entries = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
    assert_eq!(entries, 1, "no temp file left behind");
}

#[test]
fn saving_again_replaces_the_previous_state() {
    let dir = TempDir::new();
    let path = dir.path().join("window.toml");
    custom().save(&path).unwrap();
    WindowState::default().save(&path).unwrap();
    assert_eq!(WindowState::load(&path).unwrap(), WindowState::default());
}

#[test]
fn missing_fields_keep_their_defaults_and_unknown_ones_are_ignored() {
    let dir = TempDir::new();
    let path = dir.write(
        "window.toml",
        b"queue_shown = false\nwidth = 900\nfrom_a_newer_version = 1\n",
    );
    assert_eq!(
        WindowState::load(&path).unwrap(),
        WindowState {
            queue_shown: false,
            width: 900,
            ..WindowState::default()
        }
    );
}

#[test]
fn every_column_is_shown_by_default() {
    assert!(WindowState::default().hidden_columns.is_empty());
}

#[test]
fn a_hidden_column_this_version_lacks_is_dropped_and_the_rest_kept() {
    let dir = TempDir::new();
    let path = dir.write(
        "window.toml",
        b"width = 900\nhidden_columns = [\"bit_rate\", \"from_a_newer_version\", 3]\n",
    );
    let state = WindowState::load(&path).unwrap();
    assert_eq!(state.hidden_columns, [SortColumn::BitRate].into());
    assert_eq!(state.width, 900);
}

#[test]
fn unreadable_hidden_columns_show_every_column_without_losing_the_rest() {
    let dir = TempDir::new();
    let path = dir.write("window.toml", b"width = 900\nhidden_columns = \"artist\"\n");
    let state = WindowState::load(&path).unwrap();
    assert!(state.hidden_columns.is_empty());
    assert_eq!(state.width, 900);
}

#[test]
fn an_empty_file_gives_the_defaults() {
    let dir = TempDir::new();
    let path = dir.write("window.toml", b"");
    assert_eq!(WindowState::load(&path).unwrap(), WindowState::default());
}

#[test]
fn a_tiny_saved_size_grows_to_the_minimum() {
    let dir = TempDir::new();
    let path = dir.write("window.toml", b"width = 0\nheight = 10\n");
    let state = WindowState::load(&path).unwrap();
    assert_eq!(
        (state.width, state.height),
        (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
    );
}

#[test]
fn the_minimum_size_itself_is_kept() {
    let dir = TempDir::new();
    let path = dir.path().join("window.toml");
    WindowState {
        width: MIN_WINDOW_WIDTH,
        height: MIN_WINDOW_HEIGHT,
        ..WindowState::default()
    }
    .save(&path)
    .unwrap();
    let state = WindowState::load(&path).unwrap();
    assert_eq!(
        (state.width, state.height),
        (MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
    );
}

#[test]
fn a_malformed_file_is_an_error() {
    let dir = TempDir::new();
    for (name, text) in [
        ("broken.toml", "width = ".as_bytes()),
        ("wrong-type.toml", b"maximized = \"yes\""),
        ("negative.toml", b"width = -5"),
        ("binary.toml", &[0xff, 0xfe, 0x00]),
    ] {
        let path = dir.write(name, text);
        let result = WindowState::load(&path);
        assert!(
            matches!(
                result,
                Err(CoreError::WindowState { .. }) | Err(CoreError::Io { .. })
            ),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn a_malformed_file_is_reported_as_window_state_not_settings() {
    let dir = TempDir::new();
    let path = dir.write("window.toml", b"width = ");
    let message = WindowState::load(&path).unwrap_err().to_string();
    assert!(message.contains("window state"), "{message}");
    assert!(!message.contains("settings"), "{message}");
}

#[test]
fn every_kind_of_selected_filter_survives_a_save_and_load() {
    for selected in [
        FilterValue::All,
        FilterValue::AlbumArtist("the beatles".into()),
        FilterValue::Artist("björk".into()),
        FilterValue::Artist(String::new()),
        FilterValue::Album(42),
        FilterValue::Album(-1),
        FilterValue::Year(Some(1997)),
        FilterValue::Year(None),
        FilterValue::Genre("trip hop".into()),
        FilterValue::Genre(String::new()),
    ] {
        let state = browsing(selected.clone());
        assert_eq!(round_trip(&state), state, "{selected:?}");
    }
}

#[test]
fn every_grouping_and_sort_survives_a_save_and_load() {
    for grouping in Grouping::ALL {
        for sort in SortColumn::ALL
            .into_iter()
            .flat_map(|column| [false, true].map(|descending| Some(Sort { column, descending })))
            .chain([None])
        {
            let state = WindowState {
                browse: BrowseState {
                    grouping,
                    selected: FilterValue::All,
                    sort,
                },
                ..WindowState::default()
            };
            assert_eq!(round_trip(&state), state, "{grouping:?} {sort:?}");
        }
    }
}

#[test]
fn a_hand_written_browse_section_is_read() {
    let dir = TempDir::new();
    let path = dir.write(
        "window.toml",
        b"[browse]\ngrouping = \"album_artist\"\nselected = { album_artist = \"pj harvey\" }\nsort = { column = \"title\", descending = false }\n",
    );
    assert_eq!(
        WindowState::load(&path).unwrap().browse,
        BrowseState {
            grouping: Grouping::AlbumArtist,
            selected: FilterValue::AlbumArtist("pj harvey".into()),
            sort: Some(Sort {
                column: SortColumn::Title,
                descending: false,
            }),
        }
    );
}

#[test]
fn an_unreadable_browse_section_is_dropped_but_the_window_size_is_kept() {
    let dir = TempDir::new();
    for (name, browse) in [
        ("unknown-grouping.toml", "grouping = \"composer\""),
        ("unknown-filter.toml", "selected = { composer = \"bach\" }"),
        (
            "unknown-column.toml",
            "sort = { column = \"bpm\", descending = true }",
        ),
        ("wrong-type.toml", "selected = 5"),
    ] {
        let path = dir.write(
            name,
            format!("width = 900\n[browse]\n{browse}\n").as_bytes(),
        );
        let state = WindowState::load(&path).unwrap();
        assert_eq!(state.width, 900, "{name}");
        assert_eq!(state.browse, BrowseState::default(), "{name}");
    }
}

#[test]
fn a_browse_section_that_is_not_a_table_is_dropped() {
    let dir = TempDir::new();
    let path = dir.write("window.toml", b"width = 900\nbrowse = 3\n");
    let state = WindowState::load(&path).unwrap();
    assert_eq!((state.width, state.browse), (900, BrowseState::default()));
}

fn sized(width: u32, maximized: bool, filter: u32, queue: u32) -> WindowState {
    WindowState {
        width,
        height: 800,
        maximized,
        filter_pane_width: filter,
        queue_pane_width: queue,
        ..WindowState::default()
    }
}

fn panes(state: &WindowState) -> (u32, u32) {
    (state.filter_pane_width, state.queue_pane_width)
}

#[test]
fn a_layout_that_fits_is_unchanged() {
    let state = sized(1280, false, 250, 300);
    assert_eq!(state.clone().fitted(1920, 1080, 400), state);
}

#[test]
fn a_window_bigger_than_the_screen_shrinks_to_it() {
    let state = WindowState {
        height: 1400,
        ..sized(2400, false, 250, 300)
    }
    .fitted(1440, 900, 400);
    assert_eq!((state.width, state.height), (1440, 900));
}

#[test]
fn an_unknown_screen_size_leaves_the_window_alone() {
    let state = WindowState {
        height: 1400,
        ..sized(2400, false, 250, 300)
    }
    .fitted(0, 0, 400);
    assert_eq!((state.width, state.height), (2400, 1400));
}

#[test]
fn panes_too_wide_for_the_window_shrink_in_proportion() {
    let state = sized(2400, false, 1400, 700).fitted(1440, 900, 400);
    assert_eq!(panes(&state), (693, 346));
    assert!(state.filter_pane_width + state.queue_pane_width + 400 <= state.width);
}

#[test]
fn panes_that_exactly_leave_the_main_pane_its_width_are_kept() {
    let state = sized(1000, false, 300, 300).fitted(1920, 1080, 400);
    assert_eq!(panes(&state), (300, 300));
}

#[test]
fn a_hidden_queue_still_counts() {
    let state = WindowState {
        queue_shown: false,
        ..sized(1000, false, 400, 400)
    }
    .fitted(1920, 1080, 400);
    assert_eq!(panes(&state), (300, 300));
}

#[test]
fn a_maximized_window_fits_its_panes_to_the_screen_not_the_normal_size() {
    let state = sized(800, true, 600, 600).fitted(2560, 1440, 400);
    assert_eq!(state.width, 800);
    assert_eq!(panes(&state), (600, 600));

    let state = sized(800, true, 1200, 1200).fitted(1440, 900, 400);
    assert_eq!(panes(&state), (520, 520));
}

#[test]
fn a_window_narrower_than_the_main_pane_leaves_no_room_for_panes() {
    let state = sized(400, false, 250, 300).fitted(1920, 1080, 600);
    assert_eq!(panes(&state), (0, 0));
}

#[test]
fn huge_saved_panes_do_not_overflow() {
    let state = sized(1280, false, u32::MAX, u32::MAX).fitted(1920, 1080, 400);
    assert_eq!(panes(&state), (440, 440));
}
