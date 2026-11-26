pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

Dialog {
    title: qsTr("About Lune")
    modal: true
    standardButtons: Dialog.Close

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        Image {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 12
            source: "icons/lune.svg"
            sourceSize: Qt.size(128, 128)
        }

        Label {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Lune")
            font.bold: true
            font.pointSize: Qt.application.font.pointSize * 1.6
        }

        Label {
            Layout.alignment: Qt.AlignHCenter
            text: qsTr("Version %1").arg(Qt.application.version)
            opacity: 0.7
        }

        // Selectable, so it can be copied into a bug report.
        TextEdit {
            Layout.fillWidth: true
            visible: Preferences.logFile !== ""
            horizontalAlignment: Text.AlignHCenter
            wrapMode: TextEdit.WrapAnywhere
            readOnly: true
            selectByMouse: true
            textFormat: TextEdit.PlainText
            color: palette.text
            opacity: 0.7
            text: qsTr("Log: %1").arg(Preferences.logFile)
        }

        Label {
            Layout.alignment: Qt.AlignHCenter
            Layout.bottomMargin: 12
            text: "<a href=\"https://github.com/fritojay/lune/releases\">%1</a>".arg(qsTr("Check for updates"))
            textFormat: Text.StyledText
            onLinkActivated: link => Qt.openUrlExternally(link)

            HoverHandler {
                cursorShape: Qt.PointingHandCursor
            }
        }
    }
}
