import QtQuick
import QtQuick.Controls
import com.lune.player

// A ListView with the app's scroll bar, wheel scrolling, and fling speed.
ListView {
    id: list
    readonly property ListScrollBar scrollBar: bar
    // How far rows keep their text from the scroll bar.
    readonly property real gutter: bar.gutter

    clip: true
    boundsBehavior: Flickable.StopAtBounds
    maximumFlickVelocity: Theme.maxFlickVelocity
    ScrollBar.vertical: ListScrollBar {
        id: bar
    }
    WheelScroller {
        view: list
    }
}
