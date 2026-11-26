pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import com.lune.player

// One album: its cover beside its tracks. A flat view shows one track per
// delegate, with no cover or header.
Item {
    id: album
    required property string albumTitle
    required property string albumArtist
    required property string year
    required property string genre
    required property string duration
    required property url cover
    required property int firstIndex
    required property int trackCount
    required property var trackColumns
    required property var trackIds

    required property bool flat
    // Tracks can be dragged to another row (only when `flat`).
    required property bool reorderable
    // The row a dragged track would land before, or -1.
    required property int dropRow
    required property int lastRow
    required property TrackSelection selection
    required property real currentTrackId
    required property var hiddenColumns
    required property var columnWidths
    required property real gutter
    required property int tracksLeft
    required property int tracksRight

    signal playRequested(int viewIndex)
    // For `selection`'s rows.
    signal menuRequested()
    // `y` is in this delegate's coordinates.
    signal dragMoved(real y)
    signal dragEnded(int viewIndex)

    height: flat ? Theme.rowHeight
                   : Math.max(Theme.coverSize, header.height + trackCount * Theme.rowHeight)
                     + 2 * Theme.albumPadding

    CoverArt {
        x: Theme.albumPadding
        y: Theme.albumPadding
        width: Theme.coverSize
        height: Theme.coverSize
        visible: !album.flat
        source: album.cover

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onDoubleClicked: mouse => {
                if (mouse.button === Qt.LeftButton)
                    album.playRequested(album.firstIndex)
            }
            onClicked: mouse => {
                if (mouse.button === Qt.RightButton) {
                    album.selection.selectRange(album.firstIndex, album.trackCount)
                    album.menuRequested()
                }
            }
        }
    }

    Column {
        id: tracks
        x: album.tracksLeft
        y: album.flat ? 0 : Theme.albumPadding
        width: album.width - x - album.tracksRight

        Column {
            id: header
            width: parent.width
            visible: !album.flat
            bottomPadding: 6
            Text {
                width: parent.width - album.gutter
                text: album.albumTitle
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.bold: true
                font.pointSize: 12
                color: album.palette.text
            }
            Text {
                width: parent.width - album.gutter
                text: album.albumArtist + " · " + album.year + (album.genre ? " · " + album.genre : "")
                      + (album.duration ? " · " + album.duration : "")
                textFormat: Text.PlainText
                elide: Text.ElideRight
                color: album.palette.text
                opacity: 0.7
            }
        }

        Repeater {
            model: album.trackCount

            Rectangle {
                id: trackRow
                required property int index
                readonly property int viewIndex: album.firstIndex + index
                readonly property bool selected: album.selection.revision >= 0 && album.selection.contains(viewIndex)
                readonly property bool nowPlaying: album.trackIds[index] === album.currentTrackId
                // With reuseItems, a recycled album can get its new trackCount
                // before its new lists, so an index may briefly be past their end.
                function field(list) { return list[index] ?? "" }
                readonly property color textColor: selected ? album.palette.highlightedText : album.palette.text
                width: tracks.width
                height: Theme.rowHeight
                color: Theme.rowColor(album.palette, selected, rowInput.hovered,
                                      (album.flat ? viewIndex : index) % 2 === 1)

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Theme.cellInset
                    anchors.rightMargin: Theme.cellInset + album.gutter
                    spacing: Columns.spacing
                    Repeater {
                        model: Columns.shown(album.hiddenColumns)
                        Text {
                            id: cell
                            required property var modelData
                            required property int index
                            readonly property bool marker: (modelData.marksPlaying ?? false) && trackRow.nowPlaying
                            readonly property real columnWidth: album.columnWidths[index] ?? 0
                            visible: columnWidth > 0
                            Layout.preferredWidth: columnWidth
                            horizontalAlignment: modelData.alignRight ? Text.AlignRight : Text.AlignLeft
                            text: marker ? "" : trackRow.field(album.trackColumns[modelData.id])
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            color: trackRow.textColor
                            opacity: modelData.primary || trackRow.nowPlaying ? 1 : 0.7
                            font.bold: (modelData.primary ?? false) && trackRow.nowPlaying
                            font.features: modelData.alignRight ? { "tnum": 1 } : {}

                            Loader {
                                active: cell.marker
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                sourceComponent: PathIcon {
                                    name: "now-playing"
                                    color: trackRow.selected ? album.palette.highlightedText : Theme.accentColor(album.palette)
                                }
                            }
                        }
                    }
                }

                TrackRowInput {
                    id: rowInput
                    row: trackRow.viewIndex
                    lastRow: album.lastRow
                    selection: album.selection
                    dropRow: album.dropRow
                    draggable: album.reorderable
                    onPlayRequested: album.playRequested(trackRow.viewIndex)
                    onMenuRequested: album.menuRequested()
                    onDragMoved: y => album.dragMoved(rowInput.mapToItem(album, 0, y).y)
                    onDragEnded: album.dragEnded(trackRow.viewIndex)
                }
            }
        }
    }
}
