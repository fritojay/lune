use std::path::{Path, PathBuf};

use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

/// The files in `dir` ending in `.extension`, sorted.
fn files(dir: &str, extension: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == extension) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn main() -> std::io::Result<()> {
    // Every QML file and icon in `qml/` is built in, so a new one can't be left out.
    println!("cargo::rerun-if-changed=qml");
    println!("cargo::rerun-if-changed=../../packaging/icon/lune.svg");
    const SINGLETONS: [&str; 1] = ["qml/Theme.qml"];
    let is_singleton = |path: &Path| SINGLETONS.iter().any(|singleton| path.ends_with(singleton));
    let components: Vec<PathBuf> = files("qml", "qml")?
        .into_iter()
        .filter(|path| !is_singleton(path))
        .collect();
    let module = QmlModule::new("com.lune.player")
        .qml_files(components)
        .qml_files(SINGLETONS.map(|path| QmlFile::from(path).singleton(true)));
    // Qt is found via `qmake`; the QMAKE env var picks which one (DEVELOPMENT.md).
    CxxQtBuilder::new_qml_module(module)
        .files([
            "src/album_grid_model.rs",
            "src/album_model.rs",
            "src/bench.rs",
            "src/columns.rs",
            "src/desktop_entry.rs",
            "src/filter_model.rs",
            "src/left_out_model.rs",
            "src/library_manager.rs",
            "src/list_model.rs",
            "src/missing_model.rs",
            "src/player.rs",
            "src/playlists_model.rs",
            "src/preferences.rs",
            "src/queue_model.rs",
            "src/sources_model.rs",
            "src/track_selection.rs",
            "src/window_layout.rs",
        ])
        .qrc_resources(files("qml/icons", "svg")?)
        // The app icon stays in packaging/, where the bundles take it from.
        .qrc("icon.qrc")
        .qt_module("Quick")
        .build();
    println!("cargo::rerun-if-changed=../../packaging/windows/lune.rc");
    println!("cargo::rerun-if-changed=../../packaging/windows/lune.ico");
    embed_resource::compile("../../packaging/windows/lune.rc", embed_resource::NONE)
        .manifest_required()
        .map_err(std::io::Error::other)?;
    Ok(())
}
