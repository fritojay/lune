pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

// The values of the chosen grouping, or the playlists, which have the same
// roles (`label`, `detail`, `count`, `all`).
ColumnLayout {
    id: filterPane
    readonly property bool listsPlaylists: Filters.sourceKind === "playlists"
    readonly property var rows: listsPlaylists ? Playlists : Filters

    function focusSearch() {
        search.forceActiveFocus()
    }

    spacing: 6

    Connections {
        target: Filters
        function onSearchCleared() {
            search.clear()
            filterList.forceActiveFocus()
        }
    }

    Connections {
        target: Playlists
        function onPlaylistChosen(id: string) {
            Filters.showPlaylist(id)
        }
    }

    PlaylistMenu {
        id: playlistMenu
    }

    Component {
        id: newPlaylist
        NewPlaylistDialog {
            showMade: true
        }
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.margins: 6
        Layout.bottomMargin: 0
        TextField {
            id: search
            Layout.fillWidth: true
            placeholderText: qsTr("Search")
            rightPadding: clearSearch.width
            onTextChanged: Filters.setSearch(text)
            Keys.onEscapePressed: {
                text = ""
                filterList.forceActiveFocus()
            }

            ToolButton {
                id: clearSearch
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                height: parent.height
                width: height
                visible: search.text !== ""
                // Esc already clears from the keyboard, and taking focus would end the typing.
                focusPolicy: Qt.NoFocus
                icon.source: "icons/clear.svg"
                icon.color: palette.buttonText
                icon.width: 12
                icon.height: 12
                onClicked: search.clear()
            }
        }
    }

    Button {
        Layout.fillWidth: true
        Layout.leftMargin: 6
        Layout.rightMargin: 6
        visible: filterPane.listsPlaylists
        enabled: Playlists.loaded
        text: qsTr("New Playlist…")
        onClicked: newPlaylist.createObject(filterPane).open()
    }

    ListView {
        id: filterList
        Layout.fillWidth: true
        Layout.fillHeight: true
        model: filterPane.rows
        clip: true
        reuseItems: true
        boundsBehavior: Flickable.StopAtBounds
        maximumFlickVelocity: Theme.maxFlickVelocity
        currentIndex: filterPane.rows.currentRow
        // Only the keys and clicks choose a row: the current index also moves
        // when the model resets or is swapped, which is no choice.
        keyNavigationEnabled: false
        Keys.onUpPressed: {
            decrementCurrentIndex()
            filterPane.rows.select(currentIndex)
        }
        Keys.onDownPressed: {
            incrementCurrentIndex()
            filterPane.rows.select(currentIndex)
        }
        ScrollBar.vertical: ListScrollBar {
            id: filterScroll
        }
        WheelScroller {
            view: filterList
        }

        delegate: Rectangle {
            id: filterRow
            required property int index
            required property string label
            required property string detail
            required property int count
            required property bool all
            width: ListView.view.width
            height: detail ? 40 : 28
            color: ListView.isCurrentItem ? palette.highlight
                 : rowArea.containsMouse ? Theme.hoverColor(palette) : "transparent"
            readonly property color textColor: ListView.isCurrentItem ? palette.highlightedText : palette.text

            ColumnLayout {
                anchors.fill: parent
                anchors.leftMargin: 10
                anchors.rightMargin: 10 + filterScroll.gutter
                spacing: 0
                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        Layout.fillWidth: true
                        text: filterRow.label
                        textFormat: Text.PlainText
                        elide: Text.ElideRight
                        color: filterRow.textColor
                        font.bold: filterRow.all
                    }
                    Text {
                        text: filterRow.count
                        color: filterRow.textColor
                        opacity: 0.6
                        font.features: { "tnum": 1 }
                    }
                }
                Text {
                    Layout.fillWidth: true
                    visible: filterRow.detail !== ""
                    text: filterRow.detail
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    color: filterRow.textColor
                    opacity: 0.7
                    font.pointSize: Theme.smallPointSize
                }
            }

            MouseArea {
                id: rowArea
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                onClicked: mouse => {
                    if (mouse.button === Qt.RightButton) {
                        if (filterPane.listsPlaylists)
                            playlistMenu.show(filterRow.index)
                        return
                    }
                    filterPane.rows.select(filterRow.index)
                    filterList.forceActiveFocus()
                }
            }
        }
    }

    // The playing track's cover, as large as the pane allows while leaving
    // the filter list at least half the height.
    CoverArt {
        visible: Player.hasTrack
        readonly property real side: Math.min(parent.width - 12, parent.height / 2)
        Layout.preferredWidth: side
        Layout.preferredHeight: side
        Layout.alignment: Qt.AlignHCenter
        Layout.bottomMargin: 6
        source: Player.cover
        // The thumbnail is already small, and decoding at the width would decode it
        // again at every step of a resize.
        decodeAtSize: false

        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: Filters.showAlbum(Player.currentTrackId)
            ToolTip.visible: containsMouse
            ToolTip.text: qsTr("Go to the album “%1”").arg(Player.album)
        }
    }
}
