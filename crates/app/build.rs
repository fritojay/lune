use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() -> std::io::Result<()> {
    // Every QML file in `qml/` is in the module, so a new component can't be left out.
    println!("cargo::rerun-if-changed=qml");
    println!("cargo::rerun-if-changed=../../packaging/icon/lune.svg");
    const SINGLETONS: [&str; 3] = ["qml/Columns.qml", "qml/Groupings.qml", "qml/Theme.qml"];
    let mut components = Vec::new();
    for entry in std::fs::read_dir("qml")? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "qml")
            && !SINGLETONS.iter().any(|singleton| path.ends_with(singleton))
        {
            components.push(path);
        }
    }
    components.sort();
    let module = QmlModule::new("com.lune.player")
        .qml_files(components)
        .qml_files(SINGLETONS.map(|path| QmlFile::from(path).singleton(true)));
    // Qt is found via `qmake`; the QMAKE env var picks which one (DEVELOPMENT.md).
    CxxQtBuilder::new_qml_module(module)
        .files([
            "src/album_grid_model.rs",
            "src/album_model.rs",
            "src/bench.rs",
            "src/desktop_entry.rs",
            "src/filter_model.rs",
            "src/library_manager.rs",
            "src/list_model.rs",
            "src/missing_model.rs",
            "src/player.rs",
            "src/preferences.rs",
            "src/queue_model.rs",
            "src/window_layout.rs",
        ])
        .qrc_resources([
            "qml/icons/clear.svg",
            "qml/icons/error.svg",
            "qml/icons/grid.svg",
            "qml/icons/list.svg",
            "qml/icons/missing.svg",
            "qml/icons/next.svg",
            "qml/icons/previous.svg",
            "qml/icons/queue.svg",
            "qml/icons/shuffle.svg",
            "qml/icons/volume-high.svg",
            "qml/icons/volume-low.svg",
            "qml/icons/volume-muted.svg",
        ])
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
