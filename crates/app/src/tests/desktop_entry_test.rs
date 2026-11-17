use std::path::PathBuf;

use super::*;

#[test]
fn register_sets_the_desktop_file_name_qt_reports() {
    register();
    assert_eq!(ffi::desktop_file_name().to_string(), APP_ID);
}

fn packaging_file(path: &str) -> String {
    let packaging = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging");
    std::fs::read_to_string(packaging.join(path)).unwrap()
}

fn has_line(text: &str, line: &str) -> bool {
    text.lines().any(|l| l.trim_end() == line)
}

#[test]
fn every_packaging_file_uses_the_app_id() {
    let linux = |ext: &str| packaging_file(&format!("linux/{APP_ID}.{ext}"));
    assert!(has_line(&linux("desktop"), &format!("Icon={APP_ID}")));
    assert!(linux("metainfo.xml").contains(&format!("<id>{APP_ID}</id>")));
    assert_eq!(
        linux("yml").lines().next(),
        Some(format!("id: {APP_ID}").as_str())
    );
    assert!(has_line(
        &packaging_file("linux/build-flatpak.sh"),
        &format!("APP_ID={APP_ID}")
    ));
    let plist: String = packaging_file("macos/Info.plist")
        .split_whitespace()
        .collect();
    assert!(plist.contains(&format!(
        "<key>CFBundleIdentifier</key><string>{APP_ID}</string>"
    )));
}
