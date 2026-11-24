pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

ColumnLayout {
    id: queuePane
    required property var model
    required property int count
    required property int currentRow
    required property string totalDuration
    required property bool shuffle

    signal shuffleToggled(bool shuffle)
    signal clearRequested()
    signal playRequested(int row)
    signal removeRequested(int row)
    signal moveRequested(int from, int to)
    signal albumRequested(real trackId)

    function showCurrent() {
        queueList.positionViewAtIndex(currentRow, ListView.Contain)
    }

    spacing: 0

    Menu {
        id: queueMenu
        property int row: -1
        property real trackId: -1
        MenuItem {
            text: qsTr("Play")
            onTriggered: queuePane.playRequested(queueMenu.row)
        }
        MenuItem {
            text: qsTr("Remove")
            onTriggered: queuePane.removeRequested(queueMenu.row)
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("Go to Album")
            enabled: queueMenu.trackId >= 0
            onTriggered: queuePane.albumRequested(queueMenu.trackId)
        }
        MenuSeparator {}
        MenuItem {
            text: qsTr("Clear Queue")
            onTriggered: queuePane.clearRequested()
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
                visible: queuePane.count > 0
                opacity: 0.7
                font.pointSize: Application.font.pointSize - 1
                elide: Text.ElideRight
                text: qsTr("%L1 tracks · %2").arg(queuePane.count).arg(queuePane.totalDuration)
            }
        }
        ToolButton {
            icon.source: "icons/shuffle.svg"
            icon.color: checked ? palette.highlight : palette.buttonText
            checkable: true
            checked: queuePane.shuffle
            onClicked: queuePane.shuffleToggled(checked)
            Keys.onReturnPressed: click()
            Keys.onEnterPressed: click()
            ToolTip.visible: hovered
            ToolTip.text: checked ? qsTr("Shuffle is on") : qsTr("Shuffle is off")
        }
        ToolButton {
            text: qsTr("Clear")
            enabled: queuePane.count > 0
            onClicked: queuePane.clearRequested()
            Keys.onReturnPressed: click()
            Keys.onEnterPressed: click()
        }
    }

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: 1
        color: queuePane.palette.mid
    }

    Label {
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: queuePane.count === 0
        padding: 16
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.Wrap
        opacity: 0.6
        text: qsTr("The queue is empty.\nDouble-click a track to play it, or right-click to add it here.")
    }

    ListView {
        id: queueList
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: queuePane.count > 0
        // No model while hidden: delegates would still be made, and ask for covers.
        model: queuePane.visible ? queuePane.model : null
        onModelChanged: if (model) queuePane.showCurrent()
        clip: true
        reuseItems: true
        boundsBehavior: Flickable.StopAtBounds
        maximumFlickVelocity: Theme.maxFlickVelocity
        ScrollBar.vertical: ListScrollBar {
            id: queueScroll
        }
        WheelScroller {
            view: queueList
        }
        property int dropRow: -1

        function dropTarget(item: Item, y: real): int {
            const p = item.mapToItem(queueList.contentItem, 0, y)
            const row = queueList.indexAt(0, p.y)
            if (row < 0)
                return p.y < 0 ? 0 : queuePane.count
            const top = queueList.itemAtIndex(row).y
            return p.y - top > Theme.rowHeight / 2 ? row + 1 : row
        }

        delegate: QueueDelegate {
            id: queueRow
            width: ListView.view.width
            currentRow: queuePane.currentRow
            rowCount: queuePane.count
            dropRow: queueList.dropRow
            gutter: queueScroll.gutter
            onPlayRequested: queuePane.playRequested(queueRow.index)
            onMenuRequested: {
                queueMenu.row = queueRow.index
                queueMenu.trackId = queueRow.trackId
                queueMenu.popup()
            }
            onDragMoved: y => queueList.dropRow = queueList.dropTarget(queueRow, y)
            onDragEnded: if (queueList.dropRow >= 0) {
                queuePane.moveRequested(queueRow.index, queueList.dropRow)
                queueList.dropRow = -1
            }
        }
    }
}
