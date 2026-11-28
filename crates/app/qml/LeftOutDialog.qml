pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

Dialog {
    title: qsTr("Files Not Added")
    modal: true
    standardButtons: Dialog.Close

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            textFormat: Text.PlainText
            opacity: 0.7
            text: qsTr("These are in your library folders but not in your library. Files that couldn't be opened or whose tags couldn't be read are tried again at every scan. Your files are not touched.")
        }

        Frame {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: 360
            padding: 0

            ScrollingList {
                anchors.fill: parent
                model: LeftOutModel {}

                Label {
                    anchors.centerIn: parent
                    visible: Library.leftOutCount === 0
                    opacity: 0.6
                    text: qsTr("All files were added")
                }

                delegate: ItemDelegate {
                    id: row
                    required property string path
                    required property string reason
                    width: ListView.view.width
                    contentItem: ColumnLayout {
                        spacing: 2
                        Label {
                            Layout.fillWidth: true
                            text: row.path
                            textFormat: Text.PlainText
                            elide: Text.ElideMiddle
                        }
                        Label {
                            Layout.fillWidth: true
                            text: row.reason
                            textFormat: Text.PlainText
                            wrapMode: Text.WordWrap
                            opacity: 0.7
                        }
                    }
                }
            }
        }
    }
}
