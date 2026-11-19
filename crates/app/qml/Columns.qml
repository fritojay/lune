pragma Singleton
import QtQuick

// The main pane's columns, in display order. `id` is the column's
// SortColumn::name, which FilterModel and WindowLayout take. `role` is the
// AlbumListModel role with the column's text for each track. A test in
// album_model_test.rs checks both against Rust.
QtObject {
    readonly property var all: [
        { id: "track_number", title: qsTr("#"), menuTitle: qsTr("Track Number"), role: "trackNumbers",
          width: 32, alignRight: true, marksPlaying: true },
        { id: "title", title: qsTr("Title"), role: "trackTitles", fill: true, primary: true },
        { id: "artist", title: qsTr("Artist"), role: "trackArtists", width: 220, maxShare: 0.25, hideable: true },
        { id: "file_type", title: qsTr("File Type"), role: "trackFileTypes", width: 76, hideable: true },
        { id: "bit_rate", title: qsTr("Bit Rate"), role: "trackBitRates", width: 72, alignRight: true, hideable: true },
        { id: "duration", title: qsTr("Time"), role: "trackDurations", width: 56, alignRight: true },
    ]
    readonly property var hideable: all.filter(column => column.hideable)

    // Rows build only these, so a hidden column costs nothing per row.
    function shown(hidden: var): var {
        return all.filter(column => !hidden.includes(column.id))
    }

    // -1 leaves a filling column at its implicit width.
    function width(column: var, available: real): real {
        if (column.fill)
            return -1
        return column.maxShare ? Math.min(column.width, available * column.maxShare) : column.width
    }
}
