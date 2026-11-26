import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

Rectangle {
    id: queueRow
    required property int index
    required property string title
    required property string artist
    required property string duration
    required property url cover
    required property bool missing

    required property int currentRow
    required property int rowCount
    // Where a dragged row would land, or -1.
    required property int dropRow
    required property real gutter
    required property TrackSelection selection
    readonly property bool current: index === currentRow
    readonly property bool selected: selection.revision >= 0 && selection.contains(index)
    readonly property color textColor: selected ? palette.highlightedText : palette.text

    signal playRequested()
    // For `selection`'s rows.
    signal menuRequested()
    // `y` is in this row's coordinates.
    signal dragMoved(real y)
    signal dragEnded()

    height: Theme.rowHeight + 14
    color: Theme.rowColor(palette, selected, rowInput.hovered, index % 2 === 1)

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 8
        anchors.rightMargin: 8 + queueRow.gutter
        spacing: 8
        Loader {
            Layout.preferredWidth: 14
            Layout.preferredHeight: 14
            active: queueRow.current
            sourceComponent: PathIcon {
                name: "now-playing"
                width: 14
                height: 14
                color: Theme.accentColor(queueRow.palette)
            }
        }
        CoverArt {
            Layout.preferredWidth: Theme.rowHeight + 6
            Layout.preferredHeight: Theme.rowHeight + 6
            source: queueRow.cover
            placeholder: false
        }
        Column {
            Layout.fillWidth: true
            Text {
                width: parent.width
                text: queueRow.title
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.bold: queueRow.current
                color: queueRow.textColor
            }
            Text {
                width: parent.width
                text: queueRow.artist
                textFormat: Text.PlainText
                elide: Text.ElideRight
                opacity: 0.7
                font.pointSize: 9
                color: queueRow.textColor
            }
        }
        Image {
            visible: queueRow.missing
            source: "icons/missing.svg"
            sourceSize: Qt.size(16, 16)
            Layout.preferredWidth: 16
            Layout.preferredHeight: 16

            HoverHandler {
                id: missingHover
            }
            ToolTip.visible: missingHover.hovered
            ToolTip.text: qsTr("File not found; skipped when playing")
        }
        Text {
            text: queueRow.duration
            font.features: { "tnum": 1 }
            opacity: 0.7
            color: queueRow.textColor
        }
    }

    TrackRowInput {
        id: rowInput
        row: queueRow.index
        lastRow: queueRow.rowCount - 1
        selection: queueRow.selection
        dropRow: queueRow.dropRow
        onPlayRequested: queueRow.playRequested()
        onMenuRequested: queueRow.menuRequested()
        onDragMoved: y => queueRow.dragMoved(y)
        onDragEnded: queueRow.dragEnded()
    }
}
