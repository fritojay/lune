pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

// The selected value's tracks, grouped by album: a list, or a grid of covers
// where clicking one opens that album's tracks.
ColumnLayout {
    id: trackPane
    required property var model
    // `model`'s albums a row of the grid at a time.
    required property AlbumGridModel gridModel
    // `model` narrowed to the open album by its `albumId`.
    required property AlbumListModel openAlbumModel
    required property int albumCount
    required property int trackCount
    required property string title
    required property bool loading
    required property string sortColumn
    required property bool sortDescending
    required property bool albumDirectional
    required property bool hasFolders
    required property string scanStatus
    required property real currentTrackId
    required property var hiddenColumns
    required property var columnWidths
    required property bool albumGrid
    required property PlayerController controller
    required property PlaylistsModel playlists
    // A track list, not albums with covers: a column sort or a playlist.
    required property bool flat
    required property bool playlistShown
    required property bool reorderable
    // Shown in place of an empty track list.
    required property string emptyHint
    readonly property bool hinted: !loading && trackCount === 0 && emptyHint !== ""
    // The tracks' cells: under the headings in the list, inside the open
    // album's panel in the grid.
    readonly property real rowWidth: grid ? albumRows.panelWidth - tracksLeft - tracksRight
                                            - 2 * Theme.cellInset
                                          : headingCells.width
    readonly property bool grid: albumGrid && !flat
    property real openAlbumId: -1
    // The album `openAlbumModel` holds. It outlasts `openAlbumId` while the
    // closed album's panel shuts, so its tracks don't vanish first.
    property real shownAlbumId: -1
    onOpenAlbumIdChanged: {
        if (openAlbumId >= 0)
            shownAlbumId = openAlbumId
    }
    readonly property int tracksLeft: flat ? 0 : 2 * Theme.albumPadding + Theme.coverSize
    readonly property int tracksRight: flat ? 0 : Theme.albumPadding
    readonly property TrackList list: albumList

    signal sortClicked(string column)
    signal sortChosen(string column, bool descending)
    signal columnToggled(string column, bool shown)
    signal playRequested(int start)
    signal albumRequested(real trackId)
    signal removeFromPlaylistRequested(list<int> rows)
    // `rows` keep their order and go before the row now at `to`.
    signal moveRequested(list<int> rows, int to)
    signal addFolderRequested()
    signal albumGridChosen(bool grid)
    signal newPlaylistRequested(list<real> trackIds)

    // The first grid row in view and the columns it had, so the album that
    // starts it stays in view when the grid is relaid.
    property int keptRow
    property int keptColumns: 1

    function resetView(reset: bool) {
        trackSelection.clear()
        if (reset)
            albumList.positionViewAtBeginning()
        else
            albumList.restorePosition()
    }

    function keepGridPosition() {
        keptRow = Math.max(0, albumRows.indexAt(0, albumRows.contentY))
        keptColumns = gridModel.columns
    }

    function resetGrid(reset: bool) {
        if (reset) {
            dropAlbum()
            albumRows.positionViewAtBeginning()
        } else {
            albumRows.positionViewAtIndex(gridModel.relaidRow(keptRow, keptColumns), ListView.Beginning)
        }
    }

    function toggleAlbum(albumId: real) {
        trackSelection.clear()
        openAlbumId = openAlbumId === albumId ? -1 : albumId
    }

    function closeAlbum() {
        trackSelection.clear()
        openAlbumId = -1
    }

    // Closes the album without waiting for its panel to shut, for when the
    // panel isn't shown to animate.
    function dropAlbum() {
        closeAlbum()
        shownAlbumId = -1
    }

    onGridChanged: dropAlbum()

    // A rescan took the open album out of the view. Only `openAlbumId` is
    // cleared: changing the model's album from its own signal would reload
    // it while it is still emitting, and it is empty anyway.
    Connections {
        target: trackPane.openAlbumModel
        function onRefreshed() {
            if (trackPane.openAlbumId >= 0 && trackPane.openAlbumModel.albumCount === 0)
                trackPane.closeAlbum()
        }
    }

    function heading(text: string, column: string): string {
        return text + (sortColumn === column ? (sortDescending ? " ▼" : " ▲") : "")
    }

    component SortHeading: Label {
        id: sortHeading
        property bool active
        signal clicked()
        font.pointSize: Application.font.pointSize - 1
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

    // The selected tracks, as rows of the view.
    TrackSelection {
        id: trackSelection
    }

    TrackMenu {
        id: trackMenu
        controller: trackPane.controller
        playlists: trackPane.playlists
        source: "view"
        onNewPlaylistRequested: trackIds => trackPane.newPlaylistRequested(trackIds)
        onPlayRequested: start => trackPane.playRequested(start)
        onAlbumRequested: trackId => trackPane.albumRequested(trackId)

        CollapsingMenuSeparator {
            visible: trackPane.playlistShown
        }
        CollapsingMenuItem {
            text: qsTr("Remove from Playlist")
            visible: trackPane.playlistShown
            onTriggered: trackPane.removeFromPlaylistRequested(trackMenu.rows)
        }
        CollapsingMenuItem {
            text: qsTr("Move to Top")
            visible: trackPane.playlistShown
            enabled: trackPane.reorderable
            onTriggered: trackPane.moveRequested(trackMenu.rows, 0)
        }
        CollapsingMenuItem {
            text: qsTr("Move to Bottom")
            visible: trackPane.playlistShown
            enabled: trackPane.reorderable
            onTriggered: trackPane.moveRequested(trackMenu.rows, trackPane.trackCount)
        }
    }

    SortMenu {
        id: sortMenu
        sortColumn: trackPane.sortColumn
        sortDescending: trackPane.sortDescending
        albumDirectional: trackPane.albumDirectional
        onSortChosen: (column, descending) => trackPane.sortChosen(column, descending)

        MenuSeparator {}
        ColumnsMenu {
            title: qsTr("Columns")
            hiddenColumns: trackPane.hiddenColumns
            onColumnToggled: (column, shown) => trackPane.columnToggled(column, shown)
        }
    }

    component ViewButton: KeyToolButton {
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
            font.pointSize: 16
            font.bold: true
            elide: Text.ElideRight
            textFormat: Text.PlainText
            text: trackPane.loading ? qsTr("Loading…") : trackPane.title
        }
        ViewButton {
            icon.source: "icons/list.svg"
            checked: !trackPane.albumGrid
            onClicked: trackPane.albumGridChosen(false)
            ToolTip.text: qsTr("Show albums as a list")
        }
        ViewButton {
            icon.source: "icons/grid.svg"
            checked: trackPane.albumGrid
            onClicked: trackPane.albumGridChosen(true)
            ToolTip.text: qsTr("Show albums as a grid")
        }
    }
    Label {
        Layout.fillWidth: true
        leftPadding: 10
        bottomPadding: 6
        opacity: 0.7
        visible: !trackPane.loading
        elide: Text.ElideMiddle
        textFormat: Text.PlainText
        text: (trackPane.flat ? qsTr("%L1 tracks").arg(trackPane.trackCount)
                              : qsTr("%L1 albums, %L2 tracks").arg(trackPane.albumCount).arg(trackPane.trackCount))
              + (trackPane.playlistShown && !trackPane.reorderable && trackPane.trackCount > 1
                 ? " · " + qsTr("Clear the sort and search to reorder") : "")
              + (trackPane.scanStatus ? " · " + trackPane.scanStatus : "")
    }

    // An empty library: adding a folder is the first thing to do, or with
    // folders, the scan (in the line above) hasn't found music yet.
    Item {
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: !trackPane.loading && !trackPane.hinted && trackPane.albumCount === 0
        Column {
            anchors.centerIn: parent
            spacing: 12
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: trackPane.hasFolders ? qsTr("No music found yet") : qsTr("Your library is empty")
                font.pointSize: 14
            }
            Button {
                anchors.horizontalCenter: parent.horizontalCenter
                visible: !trackPane.hasFolders
                text: qsTr("Add a Music Folder…")
                highlighted: true
                onClicked: trackPane.addFolderRequested()
            }
        }
    }

    Label {
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: trackPane.hinted
        padding: 16
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        wrapMode: Text.Wrap
        opacity: 0.6
        text: trackPane.emptyHint
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
                model: Columns.shown(trackPane.hiddenColumns)
                SortHeading {
                    required property var modelData
                    required property int index
                    readonly property real columnWidth: trackPane.columnWidths[index] ?? 0
                    visible: columnWidth > 0
                    Layout.preferredWidth: columnWidth
                    horizontalAlignment: modelData.alignRight ? Text.AlignRight : Text.AlignLeft
                    text: trackPane.heading(modelData.title, modelData.id)
                    active: trackPane.sortColumn === modelData.id
                    onClicked: trackPane.sortClicked(modelData.id)
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
        visible: !trackPane.grid && trackPane.albumCount > 0
        model: trackPane.model
        onMoveRequested: (rows, to) => trackPane.moveRequested(rows, to)

        delegate: AlbumDelegate {
            id: listAlbum
            width: ListView.view.width
            flat: trackPane.flat
            reorderable: trackPane.reorderable
            dropRow: albumList.dropRow
            lastRow: trackPane.trackCount - 1
            tracksLeft: trackPane.tracksLeft
            tracksRight: trackPane.tracksRight
            selection: trackSelection
            currentTrackId: trackPane.currentTrackId
            hiddenColumns: trackPane.hiddenColumns
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
        visible: trackPane.grid && trackPane.albumCount > 0
        model: trackPane.gridModel
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        maximumFlickVelocity: Theme.maxFlickVelocity
        ScrollBar.vertical: ListScrollBar {
            id: gridScroll
        }
        WheelScroller {
            view: albumRows
        }
        Binding {
            target: trackPane.gridModel
            property: "columns"
            value: albumRows.columns
        }
        Binding {
            target: trackPane.gridModel
            property: "active"
            value: trackPane.grid
        }

        delegate: Column {
            id: gridRow
            required property var albums
            required property int index
            readonly property int openTile: albums.findIndex(a => a.albumId === trackPane.openAlbumId)
            width: ListView.view.width

            Row {
                Repeater {
                    model: gridRow.albums
                    AlbumTile {
                        required property var modelData
                        width: albumRows.cellWidth
                        height: albumRows.tileHeight
                        albumTitle: modelData.albumTitle
                        albumArtist: modelData.albumArtist
                        cover: modelData.cover ?? ""
                        albumId: modelData.albumId
                        firstIndex: modelData.firstIndex
                        trackCount: modelData.trackCount
                        trackIds: modelData.trackIds
                        open: albumId === trackPane.openAlbumId
                        currentTrackId: trackPane.currentTrackId
                        onOpened: albumId => trackPane.toggleAlbum(albumId)
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
                    enabled: panel.open || trackPane.openAlbumId < 0
                    NumberAnimation {
                        duration: 180
                        easing.type: Easing.OutCubic
                        onFinished: {
                            if (panel.open)
                                albumRows.positionViewAtIndex(gridRow.index, ListView.Contain)
                            else if (trackPane.openAlbumId < 0)
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
                            model: panel.height > 0 || panel.open ? trackPane.openAlbumModel : 0
                            AlbumDelegate {
                                width: background.width
                                flat: false
                                reorderable: false
                                dropRow: -1
                                lastRow: -1
                                tracksLeft: trackPane.tracksLeft
                                tracksRight: trackPane.tracksRight
                                selection: trackSelection
                                currentTrackId: trackPane.currentTrackId
                                hiddenColumns: trackPane.hiddenColumns
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
                        onClicked: trackPane.closeAlbum()
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Close the album")
                    }
                }
            }
        }
    }
}
