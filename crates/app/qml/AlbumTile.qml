pragma ComponentBehavior: Bound
import QtQuick
import com.lune.player

// One album in the grid: its cover, title, and album artist. A click opens
// or closes its tracks.
Item {
    id: tile
    required property string albumTitle
    required property string albumArtist
    required property url cover
    required property real albumId
    required property int firstIndex
    required property int trackCount
    required property var trackIds

    required property real currentTrackId
    required property bool open

    signal opened(real albumId)
    signal playRequested(int viewIndex)
    signal menuRequested(int start, int tracks)

    readonly property bool nowPlaying: trackIds.indexOf(currentTrackId) >= 0

    Rectangle {
        anchors.centerIn: content
        width: content.width + Theme.albumPadding
        height: content.height + Theme.albumPadding
        radius: 4
        color: tile.open ? Theme.tint(tile.palette, 0.12)
             : tileArea.containsMouse ? Theme.hoverColor(tile.palette) : "transparent"
    }

    Column {
        id: content
        anchors.horizontalCenter: parent.horizontalCenter
        y: Theme.albumPadding
        width: Theme.coverSize
        spacing: 2

        CoverArt {
            width: Theme.coverSize
            height: Theme.coverSize
            source: tile.cover

            Loader {
                active: tile.nowPlaying
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: 6
                sourceComponent: Rectangle {
                    width: 24
                    height: 24
                    radius: 12
                    color: tile.palette.base
                    PathIcon {
                        anchors.centerIn: parent
                        name: "now-playing"
                        color: Theme.accentColor(tile.palette)
                    }
                }
            }
        }

        Item {
            width: 1
            height: 4
        }

        Text {
            width: parent.width
            text: tile.albumTitle
            textFormat: Text.PlainText
            elide: Text.ElideRight
            font.bold: true
            color: tile.palette.text
        }
        Text {
            width: parent.width
            text: tile.albumArtist
            textFormat: Text.PlainText
            elide: Text.ElideRight
            color: tile.palette.text
            opacity: 0.7
        }
    }

    MouseArea {
        id: tileArea
        anchors.fill: content
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton)
                tile.menuRequested(tile.firstIndex, tile.trackCount)
            else
                tile.opened(tile.albumId)
        }
    }

    // A click opens the album, so playing gets its own button rather than a
    // double-click, which would open and close it.
    Rectangle {
        x: content.x + 8
        y: content.y + Theme.coverSize - height - 8
        width: 32
        height: 32
        radius: 16
        visible: tileArea.containsMouse || playArea.containsMouse
        color: playArea.containsMouse ? Theme.accentColor(tile.palette) : tile.palette.base
        PathIcon {
            anchors.centerIn: parent
            name: "play"
            color: playArea.containsMouse ? tile.palette.base : tile.palette.text
        }
        MouseArea {
            id: playArea
            anchors.fill: parent
            hoverEnabled: true
            onClicked: tile.playRequested(tile.firstIndex)
        }
    }
}
