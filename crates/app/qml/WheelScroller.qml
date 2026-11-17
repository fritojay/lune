import QtQuick

// Flickable moves a fixed wheelScrollLines × 24 px per wheel notch, and Qt 6.11
// has no way to change that, so mouse wheels scroll here instead, `speed` times
// as far. Touchpads aren't PointerDevice.Mouse, so they keep Flickable's scrolling.
WheelHandler {
    id: wheel
    required property Flickable view
    required property int speed

    acceptedDevices: PointerDevice.Mouse
    target: null

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
        const view = wheel.view
        const step = -event.angleDelta.y / 120 * Application.styleHints.wheelScrollLines * 24 * wheel.speed
        const from = scroll.running ? scroll.to : view.contentY
        const top = view.originY
        const bottom = Math.max(top, view.originY + view.contentHeight - view.height)
        scroll.to = Math.min(Math.max(from + step, top), bottom)
        scroll.restart()
    }
}
