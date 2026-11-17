import QtQuick.Controls

// Rows inset their text by `gutter` so it never sits under the scroll bar; row
// backgrounds still run under it.
ScrollBar {
    readonly property real gutter: size < 1 ? width : 0
}
