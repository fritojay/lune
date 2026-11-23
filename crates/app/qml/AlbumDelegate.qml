pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import com.lune.player

// One album: its cover beside its tracks. A sorted view shows one track per
// delegate, with no cover or header.
Item {
    id: album
    required property string albumTitle
    required property string albumArtist
    required property string year
    required property string genre
    required property string duration
    required property url cover
    required property int firstIndex
    required property int trackCount
    required property var trackColumns
    required property var trackIds

    required property bool sorted
    required property int selectedIndex
    required property real currentTrackId
    required property var hiddenColumns
    required property var columnWidths
    required property real gutter
    required property int tracksLeft
    required property int tracksRight

    signal trackClicked(int viewIndex)
    signal playRequested(int viewIndex)
    signal menuRequested(int start, int tracks)

    height: sorted ? Theme.rowHeight
                   : Math.max(Theme.coverSize, header.height + trackCount * Theme.rowHeight)
                     + 2 * Theme.albumPadding

    CoverArt {
        x: Theme.albumPadding
        y: Theme.albumPadding
        width: Theme.coverSize
        height: Theme.coverSize
        visible: !album.sorted
        source: album.cover

        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton
            onDoubleClicked: mouse => {
                if (mouse.button === Qt.LeftButton)
                    album.playRequested(album.firstIndex)
            }
            onClicked: mouse => {
                if (mouse.button === Qt.RightButton)
                    album.menuRequested(album.firstIndex, album.trackCount)
            }
        }
    }

    Column {
        id: tracks
        x: album.tracksLeft
        y: album.sorted ? 0 : Theme.albumPadding
        width: album.width - x - album.tracksRight

        Column {
            id: header
            width: parent.width
            visible: !album.sorted
            bottomPadding: 6
            Text {
                width: parent.width - album.gutter
                text: album.albumTitle
                textFormat: Text.PlainText
                elide: Text.ElideRight
                font.bold: true
                font.pointSize: 12
                color: album.palette.text
            }
            Text {
                width: parent.width - album.gutter
                text: album.albumArtist + " · " + album.year + (album.genre ? " · " + album.genre : "")
                      + (album.duration ? " · " + album.duration : "")
                textFormat: Text.PlainText
                elide: Text.ElideRight
                color: album.palette.text
                opacity: 0.7
            }
        }

        Repeater {
            model: album.trackCount

            Rectangle {
                id: trackRow
                required property int index
                readonly property int viewIndex: album.firstIndex + index
                readonly property bool selected: album.selectedIndex === viewIndex
                readonly property bool nowPlaying: album.trackIds[index] === album.currentTrackId
                // With reuseItems, a recycled album can get its new trackCount
                // before its new lists, so an index may briefly be past their end.
                function field(list) { return list[index] ?? "" }
                readonly property color textColor: selected ? album.palette.highlightedText : album.palette.text
                width: tracks.width
                height: Theme.rowHeight
                color: selected ? album.palette.highlight
                     : rowArea.containsMouse ? Theme.hoverColor(album.palette)
                     : (album.sorted ? viewIndex : index) % 2 ? Theme.stripeColor(album.palette) : "transparent"

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 6
                    anchors.rightMargin: 6 + album.gutter
                    spacing: Columns.spacing
                    Repeater {
                        model: Columns.shown(album.hiddenColumns)
                        Text {
                            id: cell
                            required property var modelData
                            required property int index
                            readonly property bool marker: (modelData.marksPlaying ?? false) && trackRow.nowPlaying
                            readonly property real columnWidth: album.columnWidths[index] ?? 0
                            visible: columnWidth > 0
                            Layout.preferredWidth: columnWidth
                            horizontalAlignment: modelData.alignRight ? Text.AlignRight : Text.AlignLeft
                            text: marker ? "" : trackRow.field(album.trackColumns[modelData.id])
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            color: trackRow.textColor
                            opacity: modelData.primary || trackRow.nowPlaying ? 1 : 0.7
                            font.bold: (modelData.primary ?? false) && trackRow.nowPlaying
                            font.features: modelData.alignRight ? { "tnum": 1 } : {}

                            Loader {
                                active: cell.marker
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                sourceComponent: PathIcon {
                                    name: "now-playing"
                                    color: trackRow.selected ? album.palette.highlightedText : Theme.accentColor(album.palette)
                                }
                            }
                        }
                    }
                }

                MouseArea {
                    id: rowArea
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    onClicked: mouse => {
                        album.trackClicked(trackRow.viewIndex)
                        if (mouse.button === Qt.RightButton)
                            album.menuRequested(trackRow.viewIndex, 1)
                    }
                    onDoubleClicked: mouse => {
                        if (mouse.button === Qt.LeftButton)
                            album.playRequested(trackRow.viewIndex)
                    }
                }
            }
        }
    }
}
