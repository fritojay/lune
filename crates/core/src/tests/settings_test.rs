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
fn a_malformed_or_misspelled_file_is_an_error() {
    let dir = TempDir::new();
    for (name, text) in [
        ("broken.toml", "library_folders = [".as_bytes()),
        ("wrong-type.toml", b"library_folders = \"/a\""),
        // A typo must not silently mean "no folders".
        ("typo.toml", b"library_folder = [\"/a\"]"),
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
    };
    assert!(settings.remove_folder(Path::new("/a")));
    assert!(!settings.remove_folder(Path::new("/a")));
    assert_eq!(folders(&settings), ["/b"]);
}
