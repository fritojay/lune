import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Rectangle {
    id: banner
    required property string message
    signal dismissed()

    visible: message !== ""
    implicitHeight: errorRow.implicitHeight + 12
    color: "#fde7e7"

    RowLayout {
        id: errorRow
        anchors.fill: parent
        anchors.margins: 6
        Label {
            Layout.fillWidth: true
            text: banner.message
            textFormat: Text.PlainText
            wrapMode: Text.WordWrap
            color: "#7a1010"
        }
        Button {
            text: qsTr("Dismiss")
            flat: true
            onClicked: banner.dismissed()
        }
    }
}
