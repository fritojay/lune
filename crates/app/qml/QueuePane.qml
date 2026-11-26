pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

ColumnLayout {
    id: queuePane

    function showCurrent() {
        queueList.positionViewAtIndex(queueModel.currentRow, ListView.Contain)
    }

    spacing: 0

    QueueModel {
        id: queueModel
        currentRow: Player.queueIndex
        // Rows move on every change.
        onModelAboutToBeReset: queueList.keepPosition()
        onRefreshed: {
            queueSelection.clear()
            queueList.restorePosition()
        }
    }

    TrackSelection {
        id: queueSelection
    }

    TrackMenu {
        id: queueMenu
        source: "queue"
        onPlayRequested: row => Player.play("queue", row)
        MenuSeparator {}
        MenuItem {
            text: qsTr("Remove")
            onTriggered: Player.removeQueueRows(queueMenu.rows)
        }
        MenuItem {
            text: qsTr("Clear Queue")
            onTriggered: Player.clearQueue()
        }
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.leftMargin: 10
        Layout.rightMargin: 6
        Layout.topMargin: 6
        Layout.bottomMargin: 6
        spacing: 2
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 0
            Label {
                Layout.fillWidth: true
                font.bold: true
                text: qsTr("Queue")
            }
            Label {
                Layout.fillWidth: true
                visible: queueModel.count > 0
                opacity: 0.7
                font.pointSize: Theme.smallPointSize
                elide: Text.ElideRight
                text: qsTr("%L1 tracks · %2").arg(queueModel.count).arg(queueModel.totalDuration)
            }
        }
        ToggleToolButton {
            icon.source: "icons/shuffle.svg"
            icon.color: checked ? palette.highlight : palette.buttonText
            toggledOn: Player.shuffle
            onToggleRequested: on => Player.setShuffle(on)
            ToolTip.visible: hovered
            ToolTip.text: checked ? qsTr("Shuffle is on") : qsTr("Shuffle is off")
        }
        KeyToolButton {
            text: qsTr("Clear")
            enabled: queueModel.count > 0
            onClicked: Player.clearQueue()
        }
    }

    Divider {
        Layout.fillWidth: true
    }

    EmptyHint {
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: queueModel.count === 0
        text: qsTr("The queue is empty.\nDouble-click a track to play it, or right-click to add it here.")
    }

    TrackList {
        id: queueList
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: queueModel.count > 0
        // No model while hidden: delegates would still be made, and ask for covers.
        model: queuePane.visible ? queueModel : null
        onModelChanged: if (model) queuePane.showCurrent()
        onMoveRequested: (rows, to) => Player.moveQueueRows(rows, to)

        delegate: QueueDelegate {
            id: queueRow
            width: ListView.view.width
            currentRow: queueModel.currentRow
            rowCount: queueModel.count
            dropRow: queueList.dropRow
            gutter: queueList.gutter
            selection: queueSelection
            onPlayRequested: Player.play("queue", queueRow.index)
            onMenuRequested: queueMenu.show(queueSelection.rows())
            onDragMoved: y => queueList.dropRow = queueList.dropTarget(queueRow, y)
            onDragEnded: queueList.endDrag(queueSelection, queueRow.index)
        }
    }
}
