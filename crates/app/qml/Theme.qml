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
        return tint(palette, 0.05)
    }

    function hoverColor(palette: Palette): color {
        return tint(palette, 0.1)
    }

    function tint(palette: Palette, alpha: real): color {
        return Qt.rgba(palette.text.r, palette.text.g, palette.text.b, alpha)
    }

    // The highlight color as a foreground on the list. Many dark schemes (Nord among
    // them) pick a highlight meant only as a selection background, too close to the
    // base to see as an icon, so it is blended toward the text color until it stands out.
    function accentColor(palette: Palette): color {
        let accent = palette.highlight
        for (let alpha = 0.1; contrast(accent, palette.base) < 4.5 && alpha <= 1; alpha += 0.1)
            accent = Qt.tint(palette.highlight, tint(palette, alpha))
        return accent
    }

    function contrast(a: color, b: color): real {
        const lighter = Math.max(luminance(a), luminance(b))
        const darker = Math.min(luminance(a), luminance(b))
        return (lighter + 0.05) / (darker + 0.05)
    }

    function luminance(c: color): real {
        const linear = v => v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
        return 0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }
}
