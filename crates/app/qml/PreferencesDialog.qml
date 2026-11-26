pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

Dialog {
    id: preferences
    required property list<var> folders
    required property bool busy
    required property string status

    signal addFolderRequested()
    signal removeFolderRequested(int index)
    signal rescanRequested()

    title: qsTr("Preferences")
    modal: true
    onAboutToShow: Preferences.refreshOutputDevices()
    standardButtons: Dialog.Close

    // Removing a folder deletes its tracks from the library. Adding it back
    // gives them new ids, so playlists and play history never get them back.
    Component {
        id: confirmRemove
        Dialog {
            id: confirm
            required property int row
            required property string path
            parent: Overlay.overlay
            anchors.centerIn: parent
            width: Math.min(440, parent.width - 40)
            modal: true
            title: qsTr("Remove Folder")
            standardButtons: Dialog.Ok | Dialog.Cancel
            Component.onCompleted: standardButton(Dialog.Ok).text = qsTr("Remove")
            onAccepted: preferences.removeFolderRequested(row)
            onClosed: destroy()
            Label {
                width: parent.width
                wrapMode: Text.WordWrap
                textFormat: Text.PlainText
                text: qsTr("Remove “%1” from the library? Its tracks are also taken out of your playlists and Recently Played, even if you add the folder again. The files themselves are not touched.").arg(confirm.path)
            }
        }
    }

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
                ScrollBar.vertical: ListScrollBar {}
                WheelScroller {
                    view: folderList
                }

                Label {
                    anchors.centerIn: parent
                    visible: folderList.count === 0
                    opacity: 0.6
                    text: qsTr("No folders yet")
                }

                delegate: ItemDelegate {
                    id: folderRow
                    required property int index
                    required property var modelData
                    width: ListView.view.width
                    contentItem: RowLayout {
                        Label {
                            Layout.fillWidth: true
                            text: folderRow.modelData.path
                            textFormat: Text.PlainText
                            elide: Text.ElideMiddle
                        }
                        Label {
                            readonly property var scanned: folderRow.modelData.lastScanned
                            text: scanned === undefined ? qsTr("Not scanned yet")
                                                        : qsTr("Scanned %1").arg(new Date(scanned).toLocaleString(Qt.locale(), Locale.ShortFormat))
                            opacity: 0.7
                        }
                        Button {
                            text: qsTr("Remove")
                            flat: true
                            onClicked: confirmRemove.createObject(preferences.parent, {
                                row: folderRow.index,
                                path: folderRow.modelData.path
                            }).open()
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

        Label {
            Layout.topMargin: 12
            text: qsTr("Audio Output")
            font.bold: true
        }

        ComboBox {
            Layout.fillWidth: true
            model: Preferences.outputDevices
            currentIndex: Preferences.outputDevice
            onActivated: index => Preferences.outputDevice = index
        }

        Label {
            Layout.topMargin: 12
            text: qsTr("Scrolling")
            font.bold: true
        }

        RowLayout {
            Layout.fillWidth: true
            Label {
                text: qsTr("Mouse wheel speed")
            }
            Slider {
                id: scrollSpeedSlider
                Layout.fillWidth: true
                from: Preferences.minScrollSpeed
                to: Preferences.maxScrollSpeed
                stepSize: 1
                snapMode: Slider.SnapAlways
                value: Preferences.scrollSpeed
                // Once per drag, not per step; keyboard steps aren't pressed.
                onMoved: if (!pressed) Preferences.scrollSpeed = value
                onPressedChanged: if (!pressed) Preferences.scrollSpeed = value
            }
            Label {
                text: qsTr("%1×").arg(scrollSpeedSlider.value)
            }
        }
    }
}
