use proptest::prelude::*;

use super::*;
use crate::SortColumn;
use crate::test_util::{TempDir, arb_filter_value, arb_grouping, arb_sort, arb_sort_column};

fn custom() -> WindowState {
    WindowState {
        width: 1000,
        height: 700,
        maximized: true,
        filter_pane_width: 320,
        queue_pane_width: 410,
        queue_shown: false,
        hidden_columns: [SortColumn::FileType, SortColumn::BitRate].into(),
        album_grid: true,
        browse: BrowseState {
            grouping: Grouping::Artist,
            selected: FilterValue::Text("radiohead".into()),
            sort: Some(Sort {
                column: SortColumn::Duration,
                descending: true,
            }),
            album_descending: true,
        },
    }
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
fn albums_are_a_list_by_default() {
    assert!(!WindowState::default().album_grid);
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
fn selections_saved_with_a_variant_per_field_are_read() {
    let dir = TempDir::new();
    for (saved, selected) in [
        (
            "{ album_artist = \"pj harvey\" }",
            FilterValue::Text("pj harvey".into()),
        ),
        ("{ artist = \"björk\" }", FilterValue::Text("björk".into())),
        ("{ genre = \"\" }", FilterValue::Text(String::new())),
        ("{ album = 42 }", FilterValue::Album(42)),
        ("{ year = 1997 }", FilterValue::Year(1997)),
        ("\"unknown_year\"", FilterValue::UnknownYear),
        ("\"all\"", FilterValue::All),
    ] {
        let path = dir.write(
            "window.toml",
            format!("[browse]\nselected = {saved}\n").as_bytes(),
        );
        assert_eq!(
            WindowState::load(&path).unwrap().browse.selected,
            selected,
            "{saved}"
        );
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
            selected: FilterValue::Text("pj harvey".into()),
            sort: Some(Sort {
                column: SortColumn::Title,
                descending: false,
            }),
            album_descending: false,
        }
    );
}

#[test]
fn album_order_direction_is_read_and_defaults_to_ascending() {
    let dir = TempDir::new();
    let path = dir.write(
        "window.toml",
        b"[browse]
album_descending = true
",
    );
    assert!(WindowState::load(&path).unwrap().browse.album_descending);
    let path = dir.write(
        "window.toml",
        b"[browse]
grouping = \"year\"
",
    );
    assert!(!WindowState::load(&path).unwrap().browse.album_descending);
}

#[test]
fn an_unreadable_browse_value_falls_back_to_its_default_and_keeps_the_rest() {
    let dir = TempDir::new();
    let kept = "grouping = \"year\"\nselected = { year = 1997 }\nalbum_descending = true\n";
    let saved = BrowseState {
        grouping: Grouping::Year,
        selected: FilterValue::Year(1997),
        sort: None,
        album_descending: true,
    };
    for (name, unreadable, expected) in [
        (
            "unknown-grouping.toml",
            "grouping = \"composer\"",
            BrowseState {
                grouping: Grouping::default(),
                ..saved.clone()
            },
        ),
        (
            "unknown-filter.toml",
            "selected = { composer = \"bach\" }",
            BrowseState {
                selected: FilterValue::All,
                ..saved.clone()
            },
        ),
        (
            "wrong-type-filter.toml",
            "selected = 5",
            BrowseState {
                selected: FilterValue::All,
                ..saved.clone()
            },
        ),
        (
            "unknown-column.toml",
            "sort = { column = \"bpm\", descending = true }",
            saved.clone(),
        ),
        (
            "wrong-type-direction.toml",
            "album_descending = \"yes\"",
            BrowseState {
                album_descending: false,
                ..saved.clone()
            },
        ),
    ] {
        let key = unreadable.split(' ').next().unwrap();
        let rest: String = kept
            .lines()
            .filter(|line| !line.starts_with(key))
            .map(|line| format!("{line}\n"))
            .collect();
        let path = dir.write(
            name,
            format!("width = 900\n[browse]\n{rest}{unreadable}\n").as_bytes(),
        );
        let state = WindowState::load(&path).unwrap();
        assert_eq!(state.width, 900, "{name}");
        assert_eq!(state.browse, expected, "{name}");
    }
}

#[test]
fn a_browse_section_that_is_not_a_table_is_dropped() {
    let dir = TempDir::new();
    let path = dir.write("window.toml", b"width = 900\nbrowse = 3\n");
    let state = WindowState::load(&path).unwrap();
    assert_eq!((state.width, state.browse), (900, BrowseState::default()));
}

/// Mostly sizes near the minimums and real screens, sometimes huge ones.
fn arb_size() -> impl Strategy<Value = u32> {
    prop_oneof![3 => 0..3000u32, 1 => any::<u32>()]
}

fn arb_window_state() -> impl Strategy<Value = WindowState> {
    let browse = (
        arb_grouping(),
        arb_filter_value(),
        proptest::option::of(arb_sort()),
        any::<bool>(),
    )
        .prop_map(|(grouping, selected, sort, album_descending)| BrowseState {
            grouping,
            selected,
            sort,
            album_descending,
        });
    (
        (arb_size(), arb_size(), arb_size(), arb_size()),
        (any::<bool>(), any::<bool>(), any::<bool>()),
        prop::collection::btree_set(arb_sort_column(), 0..=SortColumn::ALL.len()),
        browse,
    )
        .prop_map(
            |(
                (width, height, filter_pane_width, queue_pane_width),
                (maximized, queue_shown, album_grid),
                hidden_columns,
                browse,
            )| {
                WindowState {
                    width,
                    height,
                    maximized,
                    filter_pane_width,
                    queue_pane_width,
                    queue_shown,
                    hidden_columns,
                    album_grid,
                    browse,
                }
            },
        )
}

/// A screen size, sometimes unknown (0).
fn arb_screen() -> impl Strategy<Value = u32> {
    prop_oneof![Just(0), arb_size()]
}

proptest! {
    #[test]
    fn the_last_saved_state_loads_with_its_size_grown_to_the_minimum(
        earlier in arb_window_state(),
        state in arb_window_state(),
    ) {
        let dir = TempDir::new();
        let path = dir.path().join("window.toml");
        earlier.save(&path).unwrap();
        state.save(&path).unwrap();
        prop_assert_eq!(
            WindowState::load(&path).unwrap(),
            WindowState {
                width: state.width.max(MIN_WINDOW_WIDTH),
                height: state.height.max(MIN_WINDOW_HEIGHT),
                ..state
            }
        );
    }

    #[test]
    fn a_fitted_window_and_its_panes_fit_the_screen(
        state in arb_window_state(),
        screen_width in arb_screen(),
        screen_height in arb_screen(),
        main_pane_width in 0..2000u32,
    ) {
        let fitted = state.clone().fitted(screen_width, screen_height, main_pane_width);
        let fit = |size: u32, screen: u32| if screen == 0 { size } else { size.min(screen) };
        prop_assert_eq!(fitted.width, fit(state.width, screen_width));
        prop_assert_eq!(fitted.height, fit(state.height, screen_height));

        let window = if state.maximized && screen_width > 0 { screen_width } else { fitted.width };
        let room = u64::from(window.saturating_sub(main_pane_width));
        let sides = u64::from(state.filter_pane_width) + u64::from(state.queue_pane_width);
        for (old, new) in [
            (state.filter_pane_width, fitted.filter_pane_width),
            (state.queue_pane_width, fitted.queue_pane_width),
        ] {
            if sides <= room {
                prop_assert_eq!(new, old);
            } else {
                // Shrunk in proportion, rounding down.
                let (old, new) = (u64::from(old), u64::from(new));
                prop_assert!(new * sides <= old * room && old * room < (new + 1) * sides);
            }
        }
        prop_assert!(u64::from(fitted.filter_pane_width) + u64::from(fitted.queue_pane_width) <= room);

        let unchanged = WindowState {
            width: state.width,
            height: state.height,
            filter_pane_width: state.filter_pane_width,
            queue_pane_width: state.queue_pane_width,
            ..fitted.clone()
        };
        prop_assert_eq!(unchanged, state);
        prop_assert_eq!(fitted.clone().fitted(screen_width, screen_height, main_pane_width), fitted);
    }
}
