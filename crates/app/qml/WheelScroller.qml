import QtQuick

// Flickable moves a fixed wheelScrollLines × 24 px per wheel notch, and Qt 6.11
// has no way to change that, so mouse wheels scroll here instead, `speed` times
// as far. Touchpad events are passed on, so they keep Flickable's scrolling.
MouseArea {
    id: wheel
    required property Flickable view
    required property int speed
    readonly property bool wayland: Qt.platform.pluginName.startsWith("wayland")

    parent: view
    anchors.fill: parent
    acceptedButtons: Qt.NoButton

    readonly property NumberAnimation scroll: NumberAnimation {
        target: wheel.view
        property: "contentY"
        duration: 120
        easing.type: Easing.OutCubic
        // contentHeight is an estimate while rows are created, so the target may
        // end up past the end.
        onFinished: wheel.view.returnToBounds()
    }

    onWheel: event => {
        const fromWheel = wheel.wayland ? event.pixelDelta.x === 0 && event.pixelDelta.y === 0
                                        : event.device.type === PointerDevice.Mouse
        if (!fromWheel || event.angleDelta.y === 0) {
            event.accepted = false
            return
        }
        const view = wheel.view
        const step = -event.angleDelta.y / 120 * Application.styleHints.wheelScrollLines * 24 * wheel.speed
        const from = scroll.running ? scroll.to : view.contentY
        const top = view.originY
        const bottom = Math.max(top, view.originY + view.contentHeight - view.height)
        scroll.to = Math.min(Math.max(from + step, top), bottom)
        scroll.restart()
    }
}
