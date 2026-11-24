use super::*;
use crate::test_util::TempDir;

#[test]
fn each_library_gets_its_own_window_state() {
    assert_ne!(
        window_state_path(Path::new("/data/library.sqlite3")),
        window_state_path(Path::new("/data/scratch.sqlite3"))
    );
}

#[test]
fn the_window_state_sits_beside_the_db() {
    assert_eq!(
        window_state_path(Path::new("/data/library.sqlite3")),
        Path::new("/data/library.sqlite3.window.toml")
    );
}

#[test]
fn the_window_state_is_never_the_db_itself() {
    for db in ["library", "/", "", "data/..", "library.window.toml"] {
        assert_ne!(window_state_path(Path::new(db)), Path::new(db), "{db:?}");
    }
}

#[test]
fn a_data_folder_beside_the_exe_holds_everything() {
    let install = TempDir::new();
    let data = install.path().join(PORTABLE_DIR);
    std::fs::create_dir(&data).unwrap();
    let dirs = dirs_for(Some(&install.path().join("lune.exe"))).unwrap();
    assert_eq!(dirs.db_path(), data.join("library.sqlite3"));
    assert_eq!(dirs.config_path(), data.join("config.toml"));
    assert_eq!(dirs.log_path(), data.join("lune.log"));
    assert!(dirs.thumbnail_dir().starts_with(&data), "{dirs:?}");
}

#[test]
fn the_portable_cache_never_holds_the_db_or_settings() {
    let install = TempDir::new();
    std::fs::create_dir(install.path().join(PORTABLE_DIR)).unwrap();
    let dirs = dirs_for(Some(&install.path().join("lune.exe"))).unwrap();
    for own in [dirs.db_path(), dirs.config_path(), dirs.log_path()] {
        assert!(!own.starts_with(dirs.thumbnail_dir()), "{own:?}");
    }
}

#[test]
fn without_a_data_folder_the_user_profile_is_used() {
    let install = TempDir::new();
    let dirs = dirs_for(Some(&install.path().join("lune.exe"))).unwrap();
    for path in [
        dirs.db_path(),
        dirs.config_path(),
        dirs.thumbnail_dir(),
        dirs.log_path(),
    ] {
        assert!(!path.starts_with(install.path()), "{path:?}");
    }
}

#[test]
fn a_data_file_does_not_make_the_install_portable() {
    let install = TempDir::new();
    install.touch(PORTABLE_DIR);
    let dirs = dirs_for(Some(&install.path().join("lune.exe"))).unwrap();
    assert!(!dirs.db_path().starts_with(install.path()), "{dirs:?}");
}
