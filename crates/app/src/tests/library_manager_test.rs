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

fn entry_value(entry: &QVariant, key: &str) -> Option<QVariant> {
    entry
        .value::<QMap<QMapPair_QString_QVariant>>()
        .unwrap()
        .get(&QString::from(key))
}

#[test]
fn a_scanned_folder_entry_has_its_path_and_time_in_ms() {
    let entry = folder_entry(&Folder {
        path: PathBuf::from("/music/a"),
        last_scanned_ns: Some(1_700_000_000_123_456_789),
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
    });
    assert!(entry_value(&entry, "lastScanned").is_none());
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
