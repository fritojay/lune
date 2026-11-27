use proptest::prelude::*;

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
        output_device: Some("alsa:hw:CARD=USB,DEV=0".to_owned()),
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
fn a_file_without_a_folder_list_lists_none_and_is_saved_without_one_until_one_is_set() {
    let dir = TempDir::new();
    for text in ["", " \n\n", "scroll_speed = 4\n", "scrol_speed = 9\n"] {
        let path = dir.write("config.toml", text.as_bytes());
        let mut settings = Settings::load(&path).unwrap().unwrap();
        assert!(settings.library_folders.is_empty(), "{text:?}");
        assert!(!settings.folders_listed, "{text:?}");

        settings.set_scroll_speed(6);
        settings.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains("library_folders"), "{text:?}: {saved}");
        assert!(!Settings::load(&path).unwrap().unwrap().folders_listed);

        assert!(settings.set_library_folders(&[PathBuf::from("/a")]));
        settings.save(&path).unwrap();
        let saved = Settings::load(&path).unwrap().unwrap();
        assert!(saved.folders_listed, "{text:?}");
        assert_eq!(folders(&saved), ["/a"], "{text:?}");
    }
}

#[test]
fn an_empty_folder_list_is_listed_and_saved() {
    let dir = TempDir::new();
    let path = dir.write("config.toml", b"library_folders = []\n");
    let mut settings = Settings::load(&path).unwrap().unwrap();
    assert!(settings.folders_listed);
    assert!(settings.library_folders.is_empty());

    let mut unlisted = Settings::load(&dir.write("other.toml", b""))
        .unwrap()
        .unwrap();
    assert!(unlisted.set_library_folders(&[PathBuf::from("/a")]));
    assert!(unlisted.set_library_folders(&[]));
    for settings in [&mut settings, &mut unlisted] {
        settings.save(&path).unwrap();
        assert!(Settings::load(&path).unwrap().unwrap().folders_listed);
    }
}

#[test]
fn setting_the_folders_reports_whether_they_changed() {
    let mut settings = Settings {
        library_folders: vec![PathBuf::from("/a")],
        folders_listed: false,
        ..Settings::default()
    };
    assert!(!settings.set_library_folders(&[PathBuf::from("/a")]));
    assert!(
        !settings.folders_listed,
        "an unchanged list stays as it was"
    );
    assert!(settings.set_library_folders(&[PathBuf::from("/b")]));
    assert_eq!(folders(&settings), ["/b"]);
    assert!(settings.folders_listed);
    assert!(settings.set_library_folders(&[]));
    assert!(settings.library_folders.is_empty());
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
fn a_file_without_a_scroll_speed_gets_the_default() {
    let dir = TempDir::new();
    let path = dir.write("config.toml", b"library_folders = [\"/a\"]\n");
    assert_eq!(
        Settings::load(&path).unwrap().unwrap().scroll_speed,
        Settings::default().scroll_speed
    );
}

/// Folders that nest, and one that shares a name prefix without nesting.
fn arb_folder() -> impl Strategy<Value = PathBuf> {
    prop::sample::select(&["/m", "/m/a", "/m/a/b", "/m/ab", "/n"][..]).prop_map(PathBuf::from)
}

proptest! {
    #[test]
    fn listed_folders_never_nest_and_cover_every_added_one(
        edits in prop::collection::vec((any::<bool>(), arb_folder()), 0..20),
    ) {
        let mut list: Vec<PathBuf> = Vec::new();
        for (add, folder) in edits {
            let before = list.clone();
            let changed = if add {
                add_folder(&mut list, folder.clone())
            } else {
                remove_folder(&mut list, &folder)
            };
            prop_assert_eq!(changed, list != before);
            if add {
                prop_assert!(list.iter().any(|f| folder.starts_with(f)));
            } else {
                prop_assert!(!list.contains(&folder));
            }
            for (i, a) in list.iter().enumerate() {
                for b in &list[i + 1..] {
                    prop_assert!(!a.starts_with(b) && !b.starts_with(a), "{a:?} and {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_scroll_speed_set_or_loaded_is_clamped_into_range(speed: u32, from in 0..20u32) {
        let mut settings = Settings::default();
        settings.set_scroll_speed(from);
        let before = settings.scroll_speed;
        let changed = settings.set_scroll_speed(speed);
        let nearest = speed.max(*SCROLL_SPEEDS.start()).min(*SCROLL_SPEEDS.end());
        prop_assert_eq!(settings.scroll_speed, nearest);
        prop_assert_eq!(changed, nearest != before);

        let dir = TempDir::new();
        let path = dir.write("config.toml", format!("scroll_speed = {speed}").as_bytes());
        prop_assert_eq!(
            Settings::load(&path).unwrap().unwrap().scroll_speed,
            nearest
        );
    }
}
