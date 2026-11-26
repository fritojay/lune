import QtQuick
import com.lune.player

// What a track row does with the mouse: select, play, open the menu, and
// drag to reorder. Fills the row; declare it after the row's content.
Item {
    id: input
    required property int row
    required property int lastRow
    required property TrackSelection selection
    // The row a dragged track would land before, or -1.
    required property int dropRow
    property bool draggable: true
    readonly property bool hovered: area.containsMouse

    signal playRequested()
    // For `selection`'s rows.
    signal menuRequested()
    // `y` is in this item's coordinates.
    signal dragMoved(real y)
    signal dragEnded()

    anchors.fill: parent

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton) {
                input.selection.keep(input.row)
                input.menuRequested()
            } else {
                input.selection.click(input.row, (mouse.modifiers & Qt.ShiftModifier) !== 0,
                                      (mouse.modifiers & Qt.ControlModifier) !== 0)
            }
        }
        onDoubleClicked: mouse => {
            if (mouse.button === Qt.LeftButton)
                input.playRequested()
        }
    }

    Rectangle {
        readonly property string line: input.selection.dropLine(input.row, input.lastRow, input.dropRow)
        width: parent.width
        height: 2
        y: line === "below" ? parent.height - 1 : -1
        visible: line !== ""
        color: input.palette.highlight
    }

    DragHandler {
        enabled: input.draggable
        target: null
        xAxis.enabled: false
        onCentroidChanged: if (active) input.dragMoved(centroid.position.y)
        onActiveChanged: if (!active) input.dragEnded()
    }
}
