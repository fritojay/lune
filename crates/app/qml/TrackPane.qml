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
    // Shown in place of an empty track list.
    required property string emptyHint
    readonly property bool hinted: !loading && trackCount === 0 && emptyHint !== ""
    // The tracks' cells: under the headings in the list, inside the open
    // album's panel in the grid.
    readonly property real rowWidth: grid ? albumRows.panelWidth - tracksLeft - tracksRight
                                            - 2 * Theme.cellInset
                                          : headingCells.width
    // A column sort shows a flat track list instead of albums with covers.
    readonly property bool sorted: sortColumn !== ""
    readonly property bool grid: albumGrid && !sorted
    property real openAlbumId: -1
    // The album `openAlbumModel` holds. It outlasts `openAlbumId` while the
    // closed album's panel shuts, so its tracks don't vanish first.
    property real shownAlbumId: -1
    onOpenAlbumIdChanged: {
        if (openAlbumId >= 0)
            shownAlbumId = openAlbumId
    }
    readonly property int tracksLeft: sorted ? 0 : 2 * Theme.albumPadding + Theme.coverSize
    readonly property int tracksRight: sorted ? 0 : Theme.albumPadding
    readonly property ListView list: albumList

    signal sortClicked(string column)
    signal sortChosen(string column, bool descending)
    signal columnToggled(string column, bool shown)
    signal playRequested(int start)
    signal albumRequested(real trackId)
    signal addFolderRequested()
    signal albumGridChosen(bool grid)

    property real keptY
    // The first grid row in view and the columns it had, so the album that
    // starts it stays in view when the grid is relaid.
    property int keptRow
    property int keptColumns: 1

    function keepPosition() {
        keptY = albumList.contentY
    }

    function resetView(reset: bool) {
        albumList.selectedIndex = -1
        if (reset)
            albumList.positionViewAtBeginning()
        else
            albumList.contentY = Math.min(keptY, Math.max(0, albumList.contentHeight - albumList.height))
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
        albumRows.selectedIndex = -1
        openAlbumId = openAlbumId === albumId ? -1 : albumId
    }

    function closeAlbum() {
        albumRows.selectedIndex = -1
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

    TrackMenu {
        id: trackMenu
        controller: trackPane.controller
        source: "view"
        onPlayRequested: start => trackPane.playRequested(start)
        onAlbumRequested: trackId => trackPane.albumRequested(trackId)
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

    component ViewButton: ToolButton {
        icon.color: checked ? palette.highlight : palette.buttonText
        checkable: true
        Keys.onReturnPressed: click()
        Keys.onEnterPressed: click()
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
            onClicked: {
                checked = Qt.binding(() => !trackPane.albumGrid)
                trackPane.albumGridChosen(false)
            }
            ToolTip.text: qsTr("Show albums as a list")
        }
        ViewButton {
            icon.source: "icons/grid.svg"
            checked: trackPane.albumGrid
            onClicked: {
                checked = Qt.binding(() => trackPane.albumGrid)
                trackPane.albumGridChosen(true)
            }
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
        text: (trackPane.sorted ? qsTr("%L1 tracks").arg(trackPane.trackCount)
                                : qsTr("%L1 albums, %L2 tracks").arg(trackPane.albumCount).arg(trackPane.trackCount))
              + (trackPane.scanStatus ? " · " + trackPane.scanStatus : "")
    }

    // An empty library with no folders: the first thing to do.
    Item {
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: !trackPane.loading && !trackPane.hasFolders && trackPane.albumCount === 0
        Column {
            anchors.centerIn: parent
            spacing: 12
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: qsTr("Your library is empty")
                font.pointSize: 14
            }
            Button {
                anchors.horizontalCenter: parent.horizontalCenter
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
            anchors.rightMargin: trackPane.tracksRight + Theme.cellInset + albumScroll.gutter
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

    ListView {
        id: albumList
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: !trackPane.grid && !trackPane.hinted && (trackPane.albumCount > 0 || trackPane.hasFolders)
        model: trackPane.model
        clip: true
        reuseItems: true
        boundsBehavior: Flickable.StopAtBounds
        maximumFlickVelocity: Theme.maxFlickVelocity
        ScrollBar.vertical: ListScrollBar {
            id: albumScroll
        }
        WheelScroller {
            view: albumList
        }
        property int selectedIndex: -1

        delegate: AlbumDelegate {
            width: ListView.view.width
            sorted: trackPane.sorted
            tracksLeft: trackPane.tracksLeft
            tracksRight: trackPane.tracksRight
            selectedIndex: albumList.selectedIndex
            currentTrackId: trackPane.currentTrackId
            hiddenColumns: trackPane.hiddenColumns
            columnWidths: trackPane.columnWidths
            gutter: albumScroll.gutter
            onTrackClicked: viewIndex => albumList.selectedIndex = viewIndex
            onPlayRequested: viewIndex => trackPane.playRequested(viewIndex)
            onMenuRequested: (start, tracks) => trackMenu.show(start, tracks)
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
        property int selectedIndex: -1
        Layout.fillWidth: true
        Layout.fillHeight: true
        visible: trackPane.grid && !trackPane.hinted && (trackPane.albumCount > 0 || trackPane.hasFolders)
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
                        onMenuRequested: (start, tracks) => trackMenu.show(start, tracks)
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
                                sorted: false
                                tracksLeft: trackPane.tracksLeft
                                tracksRight: trackPane.tracksRight
                                selectedIndex: albumRows.selectedIndex
                                currentTrackId: trackPane.currentTrackId
                                hiddenColumns: trackPane.hiddenColumns
                                columnWidths: trackPane.columnWidths
                                gutter: 0
                                onTrackClicked: viewIndex => albumRows.selectedIndex = viewIndex
                                onPlayRequested: viewIndex => trackPane.playRequested(viewIndex)
                                onMenuRequested: (start, tracks) => trackMenu.show(start, tracks)
                            }
                        }
                    }

                    ToolButton {
                        anchors.top: parent.top
                        anchors.right: parent.right
                        anchors.margins: 4
                        icon.source: "icons/clear.svg"
                        onClicked: trackPane.closeAlbum()
                        Keys.onReturnPressed: click()
                        Keys.onEnterPressed: click()
                        ToolTip.visible: hovered
                        ToolTip.text: qsTr("Close the album")
                    }
                }
            }
        }
    }
}
