pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: preferences
    required property list<string> folders
    required property bool busy
    required property string status

    signal addFolderRequested()
    signal removeFolderRequested(int index)
    signal rescanRequested()

    title: qsTr("Preferences")
    modal: true
    standardButtons: Dialog.Close

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        Label {
            text: qsTr("Library Folders")
            font.bold: true
        }

        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            opacity: 0.7
            text: qsTr("Music in these folders is in your library. Changes on disk are picked up automatically. Files are never modified.")
        }

        Frame {
            Layout.fillWidth: true
            Layout.preferredHeight: 200
            padding: 0

            ListView {
                id: folderList
                anchors.fill: parent
                clip: true
                model: preferences.folders
                ScrollBar.vertical: ScrollBar {}

                Label {
                    anchors.centerIn: parent
                    visible: folderList.count === 0
                    opacity: 0.6
                    text: qsTr("No folders yet")
                }

                delegate: ItemDelegate {
                    id: folderRow
                    required property int index
                    required property string modelData
                    width: ListView.view.width
                    contentItem: RowLayout {
                        Label {
                            Layout.fillWidth: true
                            text: folderRow.modelData
                            textFormat: Text.PlainText
                            elide: Text.ElideMiddle
                        }
                        Button {
                            text: qsTr("Remove")
                            flat: true
                            onClicked: preferences.removeFolderRequested(folderRow.index)
                        }
                    }
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Button {
                text: qsTr("Add Folder…")
                onClicked: preferences.addFolderRequested()
            }
            Button {
                text: qsTr("Rescan Now")
                enabled: preferences.folders.length > 0 && !preferences.busy
                onClicked: preferences.rescanRequested()
            }
            Label {
                Layout.fillWidth: true
                visible: preferences.busy
                text: preferences.status
                textFormat: Text.PlainText
                elide: Text.ElideMiddle
                opacity: 0.7
            }
        }
    }
}
