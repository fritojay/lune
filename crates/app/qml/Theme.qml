pragma Singleton
import QtQuick

QtObject {
    readonly property int rowHeight: 24
    readonly property int coverSize: 160
    readonly property int albumPadding: 12
    // Qt's default of 2500 px/s caps a fling well short of the end of a long
    // list. BenchRun flicks at that default, so raising this doesn't skew it.
    readonly property real maxFlickVelocity: 10000

    // A tint of the text color rather than alternateBase, which the macOS style makes
    // near-white regardless of the text color, so stripes stay readable in light and dark mode.
    function stripeColor(palette: Palette): color {
        return Qt.rgba(palette.text.r, palette.text.g, palette.text.b, 0.07)
    }
}
