pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

// The selected value's tracks, grouped by album: a list, or a grid of covers
// where clicking one opens that album's tracks.
ColumnLayout {
    id: trackPane
    // The tracks' cells: under the headings in the list, inside the open
    // album's panel in the grid.
    readonly property real rowWidth: grid ? albumRows.panelWidth - tracksLeft - tracksRight
                                            - 2 * Theme.cellInset
                                          : headingCells.width
    readonly property var columnWidths: WindowLayout.columnWidths(rowWidth, WindowLayout.hiddenColumns)
    readonly property bool grid: WindowLayout.albumGrid && !Filters.flat
    readonly property bool playlistShown: Filters.sourceKind === "playlists"
    // The album `openAlbum` holds. It outlasts the grid's open album
    // while that album's panel shuts, so its tracks don't vanish first.
    property real shownAlbumId: -1
    readonly property int tracksLeft: Filters.flat ? 0 : 2 * Theme.albumPadding + Theme.coverSize
    readonly property int tracksRight: Filters.flat ? 0 : Theme.albumPadding
    readonly property TrackList list: albumList
    readonly property ListView gridList: albumRows

    signal playRequested(int start)

    // The first grid row in view and the columns it had, so the album that
    // starts it stays in view when the grid is relaid.
    property int keptRow
    property int keptColumns: 1

    // A hidden ListView still makes delegates, so the list has no model
    // while the grid shows. It comes back where it was.
    function showList(shown: bool) {
        if (!shown)
            albumList.keepPosition()
        albumList.model = shown ? albums : null
        if (shown)
            albumList.restorePosition()
    }

    Component.onCompleted: showList(!grid)
    onGridChanged: {
        shownAlbumId = -1
        showList(!grid)
    }

    AlbumListModel {
        id: albums
        // A rescan keeps the list where it was.
        onModelAboutToBeReset: if (albumList.model) albumList.keepPosition()
        onRefreshed: reset => {
            trackSelection.clear()
            if (reset)
                albumList.keptY = 0
            if (!albumList.model)
                return
            if (reset)
                albumList.positionViewAtBeginning()
            else
                albumList.restorePosition()
        }
    }

    AlbumGridModel {
        id: gridModel
        columns: albumRows.columns
        active: trackPane.grid
        onModelAboutToBeReset: {
            trackPane.keptRow = Math.max(0, albumRows.indexAt(0, albumRows.contentY))
            trackPane.keptColumns = columns
        }
        onRefreshed: reset => {
            if (reset) {
                // The grid closed its album; no panel is left to shut.
                trackPane.shownAlbumId = -1
                albumRows.positionViewAtBeginning()
            } else {
                albumRows.positionViewAtIndex(relaidRow(trackPane.keptRow, trackPane.keptColumns), ListView.Beginning)
            }
        }
        onOpenAlbumIdChanged: {
            trackSelection.clear()
            if (openAlbumId >= 0)
                trackPane.shownAlbumId = openAlbumId
        }
    }

    // The view narrowed to the open album.
    AlbumListModel {
        id: openAlbum
        oneAlbum: true
        albumId: trackPane.shownAlbumId
    }

    component SortHeading: Label {
        id: sortHeading
        property bool active
        signal clicked()
        font.pointSize: Theme.smallPointSize
        elide: Text.ElideRight
        opacity: active || sortArea.containsMouse ? 1 : 0.6
        MouseArea {
            id: sortArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: sortHeading.clicked()
        }
    }

    spacing: 0

    AddFolderDialog {
        id: folderDialog
    }

    // The selected tracks, as rows of the view.
    TrackSelection {
        id: trackSelection
    }

    TrackMenu {
        id: trackMenu
        source: "view"
        onPlayRequested: start => trackPane.playRequested(start)

        CollapsingMenuSeparator {
            visible: trackPane.playlistShown
        }
        CollapsingMenuItem {
            text: qsTr("Remove from Playlist")
            visible: trackPane.playlistShown
            onTriggered: Playlists.removeTracks(trackMenu.rows)
        }
        CollapsingMenuItem {
            text: qsTr("Move to Top")
            visible: trackPane.playlistShown
            enabled: Filters.reorderable
            onTriggered: Playlists.moveTracks(trackMenu.rows, 0)
        }
        CollapsingMenuItem {
            text: qsTr("Move to Bottom")
            visible: trackPane.playlistShown
            enabled: Filters.reorderable
            onTriggered: Playlists.moveTracks(trackMenu.rows, Filters.trackCount)
        }
    }

    SortMenu {
        id: sortMenu

        MenuSeparator {}
        ColumnsMenu {
            title: qsTr("Columns")
        }
    }

    component ViewButton: ToggleToolButton {
        icon.color: checked ? palette.highlight : palette.buttonText
        ToolTip.visible: hovered
    }

    RowLayout {
        Layout.fillWidth: true
        Layout.rightMargin: 6
        spacing: 2
        Label {
            Layout.fillWidth: true
            padding: 10
            font.pointSize: Theme.titlePointSize
            font.bold: true
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: Filters.loading ? qsTr("Loading…") : Filters.title
        }
        ViewButton {
            icon.source: "icons/list.svg"
            toggledOn: !WindowLayout.albumGrid
            onToggleRequested: WindowLayout.albumGrid = false
            ToolTip.text: qsTr("Show albums as a list")
        }
        ViewButton {
            icon.source: "icons/grid.svg"
            toggledOn: WindowLayout.albumGrid
            onToggleRequested: WindowLayout.albumGrid = true
            ToolTip.text: qsTr("Show albums as a grid")
        }
    }
    Label {
        Layout.fillWidth: true
        leftPadding: 10
        bottomPadding: 6
        opacity: 0.7
        visible: !Filters.loading
        elide: Text.ElideMiddle
        textFormat: Text.PlainText
        text: (Filters.flat ? qsTr("%L1 tracks").arg(Filters.trackCount)
                            : qsTr("%L1 albums, %L2 tracks").arg(albums.albumCount).arg(Filters.trackCount))
              + (trackPane.playlistShown && !Filters.reorderable && Filters.trackCount > 1
                 ? " · " + qsTr("Clear the sort and search to reorder") : "")
              + (Library.busy ? " · " + Library.status : "")
    }

    // An empty library. With folders, the scan's progress is in the line above.
    Item {
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: Filters.emptyLibrary !== ""
        Column {
            anchors.centerIn: parent
            spacing: 12
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: Filters.emptyLibrary
                font.pointSize: Theme.headingPointSize
            }
            Button {
                anchors.horizontalCenter: parent.horizontalCenter
                visible: Filters.offerAddFolder
                text: qsTr("Add a Music Folder…")
                highlighted: true
                onClicked: folderDialog.open()
            }
        }
    }

    Label {
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: Filters.emptyHint !== ""
        padding: 16
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.Wrap
        opacity: 0.6
        text: Filters.emptyHint
    }

    Item {
        id: headings
        Layout.fillWidth: true
        implicitHeight: Theme.rowHeight
        visible: albumList.visible && albumList.count > 0

        // Behind the headings, which take only left clicks.
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.RightButton
            onClicked: sortMenu.popup()
        }

        RowLayout {
            id: headingCells
            anchors.fill: parent
            anchors.leftMargin: trackPane.tracksLeft + Theme.cellInset
            anchors.rightMargin: trackPane.tracksRight + Theme.cellInset + albumList.gutter
            spacing: Columns.spacing
            Repeater {
                model: Columns.shown(WindowLayout.hiddenColumns)
                SortHeading {
                    required property var modelData
                    required property int index
                    readonly property real columnWidth: trackPane.columnWidths[index] ?? 0
                    readonly property var heading: Filters.sortHeadings[modelData.id]
                    visible: columnWidth > 0
                    Layout.preferredWidth: columnWidth
                    horizontalAlignment: modelData.alignRight ? Text.AlignRight : Text.AlignLeft
                    text: heading.text
                    active: heading.active
                    onClicked: Filters.sortBy(modelData.id)
                }
            }
        }

        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: trackPane.palette.mid
        }
    }

    TrackList {
        id: albumList
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: !trackPane.grid && albums.albumCount > 0
        onMoveRequested: (rows, to) => Playlists.moveTracks(rows, to)

        delegate: AlbumDelegate {
            id: listAlbum
            width: ListView.view.width
            flat: Filters.flat
            reorderable: Filters.reorderable
            dropRow: albumList.dropRow
            lastRow: Filters.trackCount - 1
            tracksLeft: trackPane.tracksLeft
            tracksRight: trackPane.tracksRight
            selection: trackSelection
            hiddenColumns: WindowLayout.hiddenColumns
            columnWidths: trackPane.columnWidths
            gutter: albumList.gutter
            onPlayRequested: viewIndex => trackPane.playRequested(viewIndex)
            onMenuRequested: trackMenu.show(trackSelection.rows())
            onDragMoved: y => albumList.dropRow = albumList.dropTarget(listAlbum, y)
            onDragEnded: viewIndex => albumList.endDrag(trackSelection, viewIndex)
        }
    }

    // A ListView of rows of tiles rather than a GridView, so the open album
    // can sit between two rows.
    ListView {
        id: albumRows
        readonly property real tileWidth: Theme.coverSize + 2 * Theme.albumPadding
        readonly property real tileHeight: Theme.coverSize + 2 * Theme.albumPadding + 2 * Theme.rowHeight
        // The scroll bar's width is kept free even while it is hidden: the
        // column count decides whether it shows, so following its gutter
        // would be a binding loop.
        readonly property real usableWidth: width - gridScroll.width
        readonly property real panelWidth: usableWidth - 2 * Theme.albumPadding
        readonly property int columns: Math.max(1, Math.floor(usableWidth / tileWidth))
        // Spare width goes between the tiles rather than after the last one.
        readonly property real cellWidth: Math.floor(usableWidth / columns)
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: trackPane.grid && albums.albumCount > 0
        model: gridModel
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        maximumFlickVelocity: Theme.maxFlickVelocity
        ScrollBar.vertical: ListScrollBar {
            id: gridScroll
        }
        WheelScroller {
            view: albumRows
        }

        delegate: Column {
            id: gridRow
            required property var albums
            required property int index
            required property int openTile
            width: ListView.view.width

            Row {
                Repeater {
                    model: gridRow.albums
                    AlbumTile {
                        required property var modelData
                        required property int index
                        width: albumRows.cellWidth
                        height: albumRows.tileHeight
                        albumTitle: modelData.albumTitle
                        albumArtist: modelData.albumArtist
                        cover: modelData.cover ?? ""
                        albumId: modelData.albumId
                        firstIndex: modelData.firstIndex
                        trackCount: modelData.trackCount
                        open: index === gridRow.openTile
                        nowPlaying: modelData.nowPlaying
                        onOpened: albumId => gridModel.toggleAlbum(albumId)
                        onPlayRequested: viewIndex => trackPane.playRequested(viewIndex)
                        onMenuRequested: (start, tracks) => {
                            trackSelection.selectRange(start, tracks)
                            trackMenu.show(trackSelection.rows())
                        }
                    }
                }
            }

            Item {
                id: panel
                readonly property bool open: gridRow.openTile >= 0
                width: parent.width
                height: open ? tracks.height + Theme.albumPadding : 0
                clip: true
                visible: height > 0

                // Opening another album shuts this panel at once: the one track
                // model already holds the other album, so a slow close would
                // show its tracks here.
                Behavior on height {
                    enabled: panel.open || gridModel.openAlbumId < 0
                    NumberAnimation {
                        duration: 180
                        easing.type: Easing.OutCubic
                        onFinished: {
                            if (panel.open)
                                albumRows.positionViewAtIndex(gridRow.index, ListView.Contain)
                            else if (gridModel.openAlbumId < 0)
                                trackPane.shownAlbumId = -1
                        }
                    }
                }

                Rectangle {
                    id: background
                    x: Theme.albumPadding
                    y: Theme.albumPadding / 2
                    width: albumRows.panelWidth
                    height: tracks.height
                    radius: 6
                    color: Theme.tint(trackPane.palette, 0.04)
                    border.width: 1
                    border.color: Theme.tint(trackPane.palette, 0.12)

                    Column {
                        id: tracks
                        width: parent.width
                        Repeater {
                            // Kept while the panel closes, so it doesn't empty first.
                            model: panel.height > 0 || panel.open ? openAlbum : 0
                            AlbumDelegate {
                                width: background.width
                                flat: false
                                reorderable: false
                                dropRow: -1
                                lastRow: -1
                                tracksLeft: trackPane.tracksLeft
                                tracksRight: trackPane.tracksRight
                                selection: trackSelection
                                hiddenColumns: WindowLayout.hiddenColumns
                                columnWidths: trackPane.columnWidths
                                gutter: 0
                                onPlayRequested: viewIndex => trackPane.playRequested(viewIndex)
                                onMenuRequested: trackMenu.show(trackSelection.rows())
                            }
                        }
                    }

                    KeyToolButton {
                        anchors.top: parent.top
                        anchors.right: parent.right
                        anchors.margins: 4
                        icon.source: "icons/clear.svg"
                        onClicked: gridModel.closeAlbum()
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Close the album")
                    }
                }
            }
        }
    }
}
