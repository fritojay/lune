use super::*;
use crate::test_util::TempDir;

fn folders(settings: &Settings) -> Vec<&str> {
    settings
        .library_folders
        .iter()
        .map(|f| f.to_str().unwrap())
        .collect()
}

#[test]
fn a_missing_file_is_none() {
    let dir = TempDir::new();
    assert_eq!(
        Settings::load(&dir.path().join("config.toml")).unwrap(),
        None
    );
}

#[test]
fn settings_survive_a_save_and_load_and_create_the_folder() {
    let dir = TempDir::new();
    let path = dir.path().join("nested/config.toml");
    let settings = Settings {
        library_folders: vec![PathBuf::from("/music"), PathBuf::from("/mnt/Ünïcode dir")],
        scroll_speed: 7,
        ..Settings::default()
    };
    settings.save(&path).unwrap();
    assert_eq!(Settings::load(&path).unwrap(), Some(settings));
    let entries = std::fs::read_dir(path.parent().unwrap()).unwrap().count();
    assert_eq!(entries, 1, "no temp file left behind");
}

#[test]
fn a_hand_written_file_is_read() {
    let dir = TempDir::new();
    let path = dir.write(
        "config.toml",
        b"# my folders\nlibrary_folders = [\"/a\", '/b']\n",
    );
    assert_eq!(
        folders(&Settings::load(&path).unwrap().unwrap()),
        ["/a", "/b"]
    );
}

#[test]
fn an_empty_file_has_no_folders() {
    let dir = TempDir::new();
    let path = dir.write("config.toml", b"");
    assert_eq!(Settings::load(&path).unwrap(), Some(Settings::default()));
}

#[test]
fn a_malformed_file_or_folder_list_is_an_error() {
    let dir = TempDir::new();
    for (name, text) in [
        ("broken.toml", "library_folders = [".as_bytes()),
        ("wrong-type.toml", b"library_folders = \"/a\""),
        ("binary.toml", &[0xff, 0xfe, 0x00]),
    ] {
        let path = dir.write(name, text);
        let result = Settings::load(&path);
        assert!(
            matches!(
                result,
                Err(CoreError::Settings { .. }) | Err(CoreError::Io { .. })
            ),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn unknown_keys_are_kept_through_a_load_and_save() {
    let dir = TempDir::new();
    let path = dir.write(
        "config.toml",
        b"library_folders = [\"/a\"]\nscroll_speed = 5\nfuture_option = true\n\n[future_section]\nlevel = 2\n",
    );
    let mut settings = Settings::load(&path).unwrap().unwrap();
    assert_eq!(folders(&settings), ["/a"]);
    assert_eq!(settings.scroll_speed, 5);

    settings.set_scroll_speed(6);
    settings.save(&path).unwrap();
    let saved = Settings::load(&path).unwrap().unwrap();
    assert_eq!(saved.scroll_speed, 6);
    assert_eq!(saved.extra, settings.extra);
    assert_eq!(saved.extra["future_option"].as_bool(), Some(true));
    assert_eq!(saved.extra["future_section"]["level"].as_integer(), Some(2));
}

#[test]
fn a_misspelled_key_gets_the_default_and_is_kept() {
    let dir = TempDir::new();
    let path = dir.write("config.toml", b"scrol_speed = 9\n");
    let settings = Settings::load(&path).unwrap().unwrap();
    assert_eq!(settings.scroll_speed, DEFAULT_SCROLL_SPEED);
    settings.save(&path).unwrap();
    let saved = Settings::load(&path).unwrap().unwrap();
    assert_eq!(saved.extra["scrol_speed"].as_integer(), Some(9));
}

#[test]
fn an_unreadable_scroll_speed_gets_the_default_and_keeps_the_folders() {
    let dir = TempDir::new();
    for text in ["scroll_speed = -1", "scroll_speed = \"fast\""] {
        let path = dir.write(
            "config.toml",
            format!("library_folders = [\"/a\"]\n{text}\n").as_bytes(),
        );
        let settings = Settings::load(&path).unwrap().unwrap();
        assert_eq!(settings.scroll_speed, DEFAULT_SCROLL_SPEED, "{text}");
        assert_eq!(folders(&settings), ["/a"], "{text}");
    }
}

#[test]
fn adding_a_folder_skips_duplicates_and_folders_already_covered() {
    let mut settings = Settings::default();
    assert!(settings.add_folder(PathBuf::from("/music/rock")));
    assert!(!settings.add_folder(PathBuf::from("/music/rock")));
    assert!(!settings.add_folder(PathBuf::from("/music/rock/old")));
    assert!(
        settings.add_folder(PathBuf::from("/music/rockabilly")),
        "not inside /music/rock"
    );
    // A parent replaces the folders inside it.
    assert!(settings.add_folder(PathBuf::from("/music")));
    assert_eq!(folders(&settings), ["/music"]);
}

#[test]
fn removing_a_folder() {
    let mut settings = Settings {
        library_folders: vec![PathBuf::from("/a"), PathBuf::from("/b")],
        ..Settings::default()
    };
    assert!(settings.remove_folder(Path::new("/a")));
    assert!(!settings.remove_folder(Path::new("/a")));
    assert_eq!(folders(&settings), ["/b"]);
}

#[test]
fn a_file_without_a_scroll_speed_gets_the_default() {
    let dir = TempDir::new();
    let path = dir.write("config.toml", b"library_folders = [\"/a\"]\n");
    assert_eq!(
        Settings::load(&path).unwrap().unwrap().scroll_speed,
        Settings::default().scroll_speed
    );
}

#[test]
fn an_out_of_range_scroll_speed_in_the_file_is_clamped() {
    let dir = TempDir::new();
    for (text, expected) in [
        ("scroll_speed = 0", *SCROLL_SPEEDS.start()),
        ("scroll_speed = 1000", *SCROLL_SPEEDS.end()),
    ] {
        let path = dir.write("config.toml", text.as_bytes());
        assert_eq!(
            Settings::load(&path).unwrap().unwrap().scroll_speed,
            expected,
            "{text}"
        );
    }
}

#[test]
fn setting_the_scroll_speed_clamps_it_and_reports_changes() {
    let mut settings = Settings::default();
    assert!(settings.set_scroll_speed(5));
    assert!(!settings.set_scroll_speed(5));
    assert_eq!(settings.scroll_speed, 5);
    assert!(settings.set_scroll_speed(0));
    assert_eq!(settings.scroll_speed, *SCROLL_SPEEDS.start());
    assert!(settings.set_scroll_speed(u32::MAX));
    assert_eq!(settings.scroll_speed, *SCROLL_SPEEDS.end());
    assert!(
        !settings.set_scroll_speed(u32::MAX),
        "already at the maximum"
    );
}
