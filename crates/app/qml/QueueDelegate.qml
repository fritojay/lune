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
    readonly property bool current: index === currentRow

    signal playRequested()
    signal menuRequested()
    // `y` is in this row's coordinates.
    signal dragMoved(real y)
    signal dragEnded()

    height: Theme.rowHeight + 14
    color: index % 2 ? Theme.stripeColor(palette) : "transparent"

    Rectangle {
        width: parent.width
        height: 2
        y: queueRow.dropRow === queueRow.index + 1 ? parent.height - 1 : -1
        visible: queueRow.dropRow === queueRow.index
                 || (queueRow.dropRow === queueRow.index + 1 && queueRow.index === queueRow.rowCount - 1)
        color: queueRow.palette.highlight
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 8
        anchors.rightMargin: 8 + queueRow.gutter
        spacing: 8
        Text {
            Layout.preferredWidth: 12
            text: queueRow.current ? "▶" : ""
            color: queueRow.palette.text
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
                color: queueRow.palette.text
            }
            Text {
                width: parent.width
                text: queueRow.artist
                textFormat: Text.PlainText
                elide: Text.ElideRight
                opacity: 0.7
                font.pointSize: 9
                color: queueRow.palette.text
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
            opacity: 0.7
            color: queueRow.palette.text
        }
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onDoubleClicked: mouse => {
            if (mouse.button === Qt.LeftButton)
                queueRow.playRequested()
        }
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton)
                queueRow.menuRequested()
        }
    }

    DragHandler {
        target: null
        xAxis.enabled: false
        onCentroidChanged: if (active) queueRow.dragMoved(centroid.position.y)
        onActiveChanged: if (!active) queueRow.dragEnded()
    }
}
