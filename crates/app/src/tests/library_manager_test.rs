use super::*;
use crate::test_util::TempDir;

#[test]
fn a_flatpak_is_detected_by_its_info_file() {
    let dir = TempDir::new();
    let marker = dir.write(".flatpak-info", b"[Application]\n");
    assert!(in_flatpak(&marker));
}

#[test]
fn without_the_info_file_it_is_not_a_flatpak() {
    let dir = TempDir::new();
    assert!(!in_flatpak(&dir.path().join(".flatpak-info")));
}
