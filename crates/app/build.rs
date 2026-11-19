use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() -> std::io::Result<()> {
    // Every QML file in `qml/` is in the module, so a new component can't be left out.
    println!("cargo::rerun-if-changed=qml");
    let mut components = Vec::new();
    for entry in std::fs::read_dir("qml")? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "qml") && !path.ends_with("Theme.qml") {
            components.push(path);
        }
    }
    components.sort();
    let module = QmlModule::new("com.lune.player")
        .qml_files(components)
        .qml_file(QmlFile::from("qml/Theme.qml").singleton(true));
    // Qt is found via `qmake`; the QMAKE env var picks which one (DEVELOPMENT.md).
    CxxQtBuilder::new_qml_module(module)
        .files([
            "src/album_model.rs",
            "src/bench.rs",
            "src/desktop_entry.rs",
            "src/filter_model.rs",
            "src/library_manager.rs",
            "src/list_model.rs",
            "src/missing_model.rs",
            "src/player.rs",
            "src/queue_model.rs",
            "src/window_layout.rs",
        ])
        .qrc_resources([
            "qml/icons/clear.svg",
            "qml/icons/missing.svg",
            "qml/icons/queue.svg",
            "qml/icons/volume-high.svg",
            "qml/icons/volume-low.svg",
            "qml/icons/volume-muted.svg",
        ])
        .qt_module("Quick")
        .build();
    Ok(())
}
