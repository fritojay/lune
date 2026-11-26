import QtQuick
import QtQuick.Controls

// Rows inset their text by `gutter` so it never sits under the scroll bar; row
// backgrounds still run under it.
ScrollBar {
    readonly property real gutter: size < 1 ? width : 0

    // The app runs Fusion (see main.rs), whose handle uses palette.mid; a tint of the
    // text color, like Theme's stripes, shows in light and dark mode. High contrast
    // gets the full text color.
    Component.onCompleted: {
        (contentItem as Rectangle).color = Qt.binding(() => {
            if (Application.styleHints.accessibility.contrastPreference === Qt.HighContrast)
                return pressed ? palette.highlight : palette.text
            return Qt.rgba(palette.text.r, palette.text.g, palette.text.b, pressed ? 0.65 : 0.45)
        })
    }
}
