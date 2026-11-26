pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

RowLayout {
    id: groupingBar
    // `kind` and `sourceId` as SourcesModel lists them.
    signal sourceChosen(string kind, string sourceId)

    // By SourcesModel's `sourceId`. A test in sources_model_test.rs checks it against Rust.
    readonly property var titles: ({
        "album_artist": qsTr("Album Artists"),
        "artist": qsTr("Artists"),
        "album": qsTr("Albums"),
        "year": qsTr("Years"),
        "genre": qsTr("Genres"),
        "file_type": qsTr("File Types"),
        "recently_added": qsTr("Recently Added"),
        "recently_played": qsTr("Recently Played"),
        "playlists": qsTr("Playlists"),
    })

    spacing: 2

    Repeater {
        model: SourcesModel {
            id: sources
        }
        // Not ToolButton: the macOS native style can't be restyled.
        AbstractButton {
            id: tab
            required property int index
            required property string kind
            required property string sourceId
            required property bool current
            implicitHeight: 34
            leftPadding: 12
            rightPadding: 12
            hoverEnabled: true
            text: groupingBar.titles[sourceId]
            font.bold: current
            checkable: true
            checked: current
            onClicked: {
                sources.choose(index)
                checked = Qt.binding(() => tab.current)
                groupingBar.sourceChosen(kind, sourceId)
            }
            Keys.onReturnPressed: click()
            Keys.onEnterPressed: click()
            contentItem: Item {
                implicitWidth: widest.implicitWidth
                implicitHeight: label.implicitHeight
                // Sized for bold text, so choosing a tab doesn't shift the others.
                Text {
                    id: widest
                    visible: false
                    text: tab.text
                    font.family: tab.font.family
                    font.pointSize: tab.font.pointSize
                    font.bold: true
                }
                Text {
                    id: label
                    anchors.centerIn: parent
                    text: tab.text
                    font: tab.font
                    color: tab.palette.buttonText
                    opacity: tab.current || tab.hovered ? 1 : 0.7
                }
            }
            background: Rectangle {
                color: tab.down || tab.hovered ? Theme.tint(tab.palette, 0.07)
                                               : "transparent"
                radius: 4
                border.width: tab.visualFocus ? 1 : 0
                border.color: tab.palette.highlight
                Rectangle {
                    anchors.bottom: parent.bottom
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: parent.width - 16
                    height: 2
                    radius: 1
                    visible: tab.current
                    color: tab.palette.highlight
                }
            }
        }
    }
}
