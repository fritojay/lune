pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: unavailable
    required property var model
    required property int count

    signal forgetRequested(real trackId)
    signal forgetAllRequested()

    title: qsTr("Unavailable Tracks")
    modal: true
    standardButtons: Dialog.Close

    ColumnLayout {
        anchors.fill: parent
        spacing: 8

        Label {
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            opacity: 0.7
            text: qsTr("These files were in your library but can no longer be found, for example because a drive is disconnected or the files were moved or deleted. They are hidden from the library and come back if the files reappear. Removing one forgets it and its play history; your files are not touched.")
        }

        Frame {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: 360
            padding: 0

            ListView {
                id: trackList
                anchors.fill: parent
                clip: true
                model: unavailable.model
                ScrollBar.vertical: ListScrollBar {}

                Label {
                    anchors.centerIn: parent
                    visible: unavailable.count === 0
                    opacity: 0.6
                    text: qsTr("No unavailable tracks")
                }

                delegate: ItemDelegate {
                    id: row
                    required property string label
                    required property string path
                    required property real missingSince
                    required property real trackId
                    width: ListView.view.width
                    contentItem: RowLayout {
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            RowLayout {
                                Label {
                                    Layout.fillWidth: true
                                    text: row.label
                                    textFormat: Text.PlainText
                                    elide: Text.ElideRight
                                }
                                Label {
                                    text: qsTr("Since %1").arg(new Date(row.missingSince).toLocaleDateString(Qt.locale(), Locale.ShortFormat))
                                    opacity: 0.7
                                }
                            }
                            Label {
                                Layout.fillWidth: true
                                text: row.path
                                textFormat: Text.PlainText
                                elide: Text.ElideMiddle
                                opacity: 0.7
                            }
                        }
                        Button {
                            text: qsTr("Remove")
                            flat: true
                            onClicked: unavailable.forgetRequested(row.trackId)
                        }
                    }
                }
            }
        }

        Button {
            text: qsTr("Remove All…")
            enabled: unavailable.count > 0
            onClicked: confirmRemoveAll.open()
        }
    }

    Dialog {
        id: confirmRemoveAll
        anchors.centerIn: parent
        modal: true
        title: qsTr("Remove All Unavailable Tracks?")
        standardButtons: Dialog.Cancel | Dialog.Ok
        onAccepted: unavailable.forgetAllRequested()

        Label {
            text: qsTr("%n track(s) and their play history will be forgotten. Files that come back later are added as new tracks.", "", unavailable.count)
            wrapMode: Text.WordWrap
            width: Math.min(implicitWidth, 400)
        }
    }
}
