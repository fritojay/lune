pragma Singleton
import QtQuick

// The main pane's columns, in display order. `id` is the column's
// SortColumn::name, which FilterModel and WindowLayout take and
// AlbumListModel's trackColumns is keyed by. A test in album_model_test.rs
// checks it against Rust.
QtObject {
    readonly property var all: [
        { id: "track_number", title: qsTr("#"), menuTitle: qsTr("Track Number"),
          alignRight: true, marksPlaying: true },
        { id: "title", title: qsTr("Title"), primary: true },
        { id: "artist", title: qsTr("Artist"), hideable: true },
        { id: "file_type", title: qsTr("File Type"), hideable: true },
        { id: "bit_rate", title: qsTr("Bit Rate"), alignRight: true, hideable: true },
        { id: "duration", title: qsTr("Time"), alignRight: true },
    ]
    readonly property var hideable: all.filter(column => column.hideable)

    // Rows build only these, so a hidden column costs nothing per row.
    function shown(hidden: var): var {
        return all.filter(column => !hidden.includes(column.id))
    }

    readonly property int spacing: 10
}
