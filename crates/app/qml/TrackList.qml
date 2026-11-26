import QtQuick
import QtQuick.Controls
import com.lune.player

// A scrolling list of track rows that can be dragged to reorder.
ListView {
    id: list
    // How far rows keep their text from the scroll bar.
    readonly property real gutter: scrollBar.gutter
    // The row a dragged track would land before, or -1.
    property int dropRow: -1

    property real keptY

    // A model reset puts the list back at its start; these keep its place.
    function keepPosition() {
        keptY = contentY
    }

    function restorePosition() {
        contentY = Math.min(keptY, Math.max(0, contentHeight - height))
    }

    // `rows` keep their order and go before the row now at `to`.
    signal moveRequested(list<int> rows, int to)

    function endDrag(selection: TrackSelection, row: int) {
        if (dropRow >= 0)
            moveRequested(selection.dragged(row), dropRow)
        dropRow = -1
    }

    // The row a drag at `y` in `item` lands before: the row under it, or the
    // next one from its lower half on.
    function dropTarget(item: Item, y: real): int {
        const p = item.mapToItem(contentItem, 0, y)
        const row = indexAt(0, p.y)
        if (row < 0)
            return p.y < 0 ? 0 : count
        const under = itemAtIndex(row)
        return p.y - under.y > under.height / 2 ? row + 1 : row
    }

    clip: true
    reuseItems: true
    boundsBehavior: Flickable.StopAtBounds
    maximumFlickVelocity: Theme.maxFlickVelocity
    ScrollBar.vertical: ListScrollBar {
        id: scrollBar
    }
    WheelScroller {
        view: list
    }
}
