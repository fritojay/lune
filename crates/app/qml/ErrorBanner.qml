import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// A warning tint over the window color with the regular text color, so it
// stays readable in light and dark mode. The icon carries the meaning too.
Rectangle {
    id: banner
    required property string message
    signal dismissed()

    readonly property color warning: "#d93025"
    function wash(base: color, alpha: real): color {
        return Qt.tint(base, Qt.rgba(warning.r, warning.g, warning.b, alpha))
    }

    visible: message !== ""
    implicitHeight: errorRow.implicitHeight + 12
    color: wash(palette.window, 0.14)

    Rectangle {
        anchors.bottom: parent.bottom
        width: parent.width
        height: 1
        color: banner.wash(banner.palette.mid, 0.3)
    }

    RowLayout {
        id: errorRow
        anchors.fill: parent
        anchors.margins: 6
        anchors.leftMargin: 10
        spacing: 8
        // Icon and text share a top edge, so the icon stays on the first line when the text wraps.
        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            Image {
                Layout.alignment: Qt.AlignTop
                Layout.topMargin: Math.max(0, (metrics.height - 16) / 2)
                source: "icons/error.svg"
                sourceSize: Qt.size(16, 16)
                Layout.preferredWidth: 16
                Layout.preferredHeight: 16
            }
            Label {
                id: label
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop
                text: banner.message
                textFormat: Text.PlainText
                wrapMode: Text.WordWrap
                FontMetrics {
                    id: metrics
                    font: label.font
                }
            }
        }
        Button {
            text: qsTr("Dismiss")
            flat: true
            onClicked: banner.dismissed()
        }
    }
}
