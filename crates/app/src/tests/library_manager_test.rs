use super::*;
use crate::test_util::{TempDir, entry_value};

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

#[test]
fn a_scanned_folder_entry_has_its_path_and_time_in_ms() {
    let entry = folder_entry(&Folder {
        path: PathBuf::from("/music/a"),
        last_scanned_ns: Some(1_700_000_000_123_456_789),
        left_out: None,
    });
    let path = entry_value(&entry, "path").unwrap().value::<QString>();
    assert_eq!(path, Some(QString::from("/music/a")));
    let scanned = entry_value(&entry, "lastScanned").unwrap().value::<f64>();
    assert_eq!(scanned, Some(1_700_000_000_123.0));
}

#[test]
fn a_never_scanned_folder_entry_has_no_time() {
    let entry = folder_entry(&Folder {
        path: PathBuf::from("/music/a"),
        last_scanned_ns: None,
        left_out: None,
    });
    assert!(entry_value(&entry, "lastScanned").is_none());
    assert!(entry_value(&entry, "leftOut").is_none());
}

#[test]
fn a_folder_entry_says_what_its_scan_left_out() {
    let entry = folder_entry(&Folder {
        path: PathBuf::from("/music/a"),
        last_scanned_ns: None,
        left_out: Some("Not added: 2 linked files.".into()),
    });
    let left_out = entry_value(&entry, "leftOut").unwrap().value::<QString>();
    assert_eq!(left_out, Some(QString::from("Not added: 2 linked files.")));
}

#[test]
fn replacing_a_folder_list_reports_whether_it_changed() {
    let mut list = vec![PathBuf::from("/a")];
    assert!(!replace(&mut list, &[PathBuf::from("/a")]));
    assert!(replace(&mut list, &[PathBuf::from("/b")]));
    assert_eq!(list, [PathBuf::from("/b")]);
    assert!(replace(&mut list, &[]));
    assert!(list.is_empty());
}

#[test]
fn a_folder_is_found_by_its_shown_path_wherever_it_moved_in_the_list() {
    let before = [PathBuf::from("/m/a"), PathBuf::from("/b")];
    let after = [PathBuf::from("/b"), PathBuf::from("/m")];
    for folders in [&before[..], &after[..]] {
        assert_eq!(shown_folder(folders, "/b"), Some(&PathBuf::from("/b")));
    }
    assert_eq!(shown_folder(&after, "/m/a"), None);
    assert_eq!(shown_folder(&[], "/b"), None);
}

#[cfg(unix)]
#[test]
fn folders_that_show_the_same_are_not_removed_by_name() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let folders = [
        PathBuf::from(OsStr::from_bytes(b"/m/\xff")),
        PathBuf::from(OsStr::from_bytes(b"/m/\xfe")),
    ];
    let shown = shown_path(&folders[0]);
    assert_eq!(shown, shown_path(&folders[1]));
    assert_eq!(shown_folder(&folders, &shown), None);
    assert_eq!(shown_folder(&folders[..1], &shown), Some(&folders[0]));
}
