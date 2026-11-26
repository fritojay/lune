import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Shown instead of the main window when another Lune has the library open.
ApplicationWindow {
    visible: true
    title: qsTr("Lune")
    width: 420
    height: content.implicitHeight + 40
    flags: Qt.Dialog

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: 20
        spacing: 12

        Label {
            Layout.fillWidth: true
            text: qsTr("Lune is already running")
            font.bold: true
        }
        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            text: qsTr("Another Lune window is using this library. Switch to it, or quit it before opening Lune again.")
        }
        Button {
            Layout.alignment: Qt.AlignRight
            text: qsTr("Quit")
            highlighted: true
            onClicked: Qt.quit()
        }
    }
}
