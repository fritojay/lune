use super::*;

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
