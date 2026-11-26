use std::path::Path;

use lune_core::DEFAULT_SCROLL_SPEED;

use super::*;
use crate::test_util::TempDir;

fn saved(path: &Path) -> Settings {
    Settings::load(path).unwrap().unwrap()
}

#[test]
fn without_a_file_the_defaults_load_and_nothing_is_pruned() {
    let dir = TempDir::new();
    let mut store = Store::load(dir.path().join("config.toml"));
    assert_eq!(store.settings().scroll_speed, DEFAULT_SCROLL_SPEED);
    assert_eq!(store.startup_folders(), None);
    assert_eq!(store.take_load_error(), None);
}

#[test]
fn a_saved_file_loads_with_its_speed_clamped() {
    let dir = TempDir::new();
    let path = dir.write(
        "config.toml",
        b"library_folders = [\"/a\"]\nscroll_speed = 99\n",
    );
    let store = Store::load(path);
    assert_eq!(store.settings().scroll_speed, *SCROLL_SPEEDS.end());
    assert_eq!(store.startup_folders(), Some(vec![PathBuf::from("/a")]));
}

#[test]
fn a_change_is_saved_with_the_other_settings_and_unknown_keys() {
    let dir = TempDir::new();
    let path = dir.write("config.toml", b"library_folders = [\"/a\"]\nfuture = 1\n");
    let mut store = Store::load(path.clone());
    assert_eq!(store.update(|s| s.set_scroll_speed(7)), Ok(()));
    let saved = saved(&path);
    assert_eq!(saved.scroll_speed, 7);
    assert_eq!(saved.library_folders, [PathBuf::from("/a")]);
    assert!(saved.extra.contains_key("future"));
}

#[test]
fn the_first_change_creates_the_file_and_then_its_folders_count() {
    let dir = TempDir::new();
    let path = dir.path().join("config/config.toml");
    let mut store = Store::load(path.clone());
    assert_eq!(store.update(|s| s.set_scroll_speed(5)), Ok(()));
    assert_eq!(saved(&path).scroll_speed, 5);
    assert_eq!(store.startup_folders(), Some(Vec::new()));
}

#[test]
fn nothing_is_saved_without_a_change() {
    let dir = TempDir::new();
    let text = b"# hand-written\nscroll_speed = 4\n";
    let path = dir.write("config.toml", text);
    let mut store = Store::load(path.clone());
    assert_eq!(store.update(|s| s.set_scroll_speed(4)), Ok(()));
    assert_eq!(std::fs::read(&path).unwrap(), text);

    let missing = dir.path().join("missing.toml");
    let mut store = Store::load(missing.clone());
    assert_eq!(store.update(|_| false), Ok(()));
    assert!(!missing.exists());
}

#[test]
fn an_unreadable_file_is_reported_once_never_saved_over_and_prunes_nothing() {
    let dir = TempDir::new();
    let broken = b"library_folders = [";
    let path = dir.write("config.toml", broken);
    let mut store = Store::load(path.clone());
    assert!(store.take_load_error().is_some());
    assert_eq!(store.take_load_error(), None);
    assert_eq!(store.startup_folders(), None);

    let saved = store.update(|s| s.set_scroll_speed(7));
    assert!(saved.is_err_and(|message| message.contains("not saved")));
    assert_eq!(store.settings().scroll_speed, 7, "kept for this run");
    assert_eq!(std::fs::read(&path).unwrap(), broken);
    assert_eq!(store.startup_folders(), None);
}

#[test]
fn a_failed_save_is_reported() {
    let dir = TempDir::new();
    let mut store = Store::load(dir.path().join("config/config.toml"));
    // A file where the settings directory should be.
    dir.write("config", b"");
    let saved = store.update(|s| s.set_scroll_speed(7));
    assert!(saved.is_err_and(|message| message.starts_with("Could not save")));
    assert_eq!(store.startup_folders(), None);
}

#[test]
fn a_scroll_speed_from_qml_is_clamped_even_when_negative() {
    assert_eq!(scroll_speed_from_qml(4), 4);
    assert_eq!(scroll_speed_from_qml(-5), *SCROLL_SPEEDS.start());
    assert_eq!(scroll_speed_from_qml(0), *SCROLL_SPEEDS.start());
    assert_eq!(scroll_speed_from_qml(i32::MAX), *SCROLL_SPEEDS.end());
}

#[test]
fn the_default_and_every_connected_device_can_be_chosen_and_a_saved_one_stays_listed() {
    let devices = [
        OutputDevice {
            id: "alsa:usb".to_owned(),
            name: "USB DAC".to_owned(),
        },
        OutputDevice {
            id: "alsa:hdmi".to_owned(),
            name: "HDMI".to_owned(),
        },
    ];
    let ids = |choices: &[(Option<String>, String)]| -> Vec<Option<String>> {
        choices.iter().map(|(id, _)| id.clone()).collect()
    };
    let connected = vec![
        None,
        Some("alsa:usb".to_owned()),
        Some("alsa:hdmi".to_owned()),
    ];
    for (saved, listed, chosen) in [
        (None, connected.clone(), 0),
        (Some("alsa:hdmi"), connected.clone(), 2),
        (
            Some("alsa:gone"),
            [connected.clone(), vec![Some("alsa:gone".to_owned())]].concat(),
            3,
        ),
    ] {
        let (choices, index) = device_choices(&devices, saved);
        assert_eq!(ids(&choices), listed, "{saved:?}");
        assert_eq!(index, chosen, "{saved:?}");
    }
    let (choices, index) = device_choices(&[], None);
    assert_eq!((ids(&choices), index), (vec![None], 0));
}
