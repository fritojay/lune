pragma Singleton
import QtQuick

// The quick-filter buttons, in display order. `id` is the grouping's
// Grouping::name, which FilterModel takes. A test in filter_model_test.rs
// checks it against Rust.
QtObject {
    readonly property var all: [
        { id: "album_artist", title: qsTr("Album Artists") },
        { id: "artist", title: qsTr("Artists") },
        { id: "album", title: qsTr("Albums") },
        { id: "year", title: qsTr("Years") },
        { id: "genre", title: qsTr("Genres") },
        { id: "recently_added", title: qsTr("Recently Added") },
        { id: "recently_played", title: qsTr("Recently Played") },
    ]
}
