pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

// The selected value's tracks, grouped by album.
ColumnLayout {
    id: trackPane
    required property var model
    required property int albumCount
    required property int trackCount
    required property string title
    required property bool loading
    required property int sortColumn
    required property bool sortDescending
    required property bool hasFolders
    required property string scanStatus
    required property int scrollSpeed
    required property real currentTrackId
    // A column sort shows a flat track list instead of albums with covers.
    readonly property bool sorted: sortColumn >= 0
    readonly property int tracksLeft: sorted ? 0 : 2 * Theme.albumPadding + Theme.coverSize
    readonly property int tracksRight: sorted ? 0 : Theme.albumPadding
    readonly property ListView list: albumList

    signal sortClicked(int column)
    signal sortChosen(int column, bool descending)
    signal playRequested(int start)
    signal playNextRequested(int start, int tracks)
    signal addToQueueRequested(int start, int tracks)
    signal addFolderRequested()

    function resetView(reset: bool, contentY: real) {
        albumList.selectedIndex = -1
        if (reset)
            albumList.positionViewAtBeginning()
        else
            albumList.contentY = Math.min(contentY, Math.max(0, albumList.contentHeight - albumList.height))
    }

    function heading(text: string, column: int): string {
        return text + (sortColumn === column ? (sortDescending ? " ▼" : " ▲") : "")
    }

    component SortHeading: Label {
        id: sortHeading
        signal clicked()
        font.bold: true
        opacity: 0.7
        MouseArea {
            anchors.fill: parent
            cursorShape: Qt.PointingHandCursor
            onClicked: sortHeading.clicked()
        }
    }

    spacing: 0

    Menu {
        id: trackMenu
        property int start: 0
        property int tracks: 1
        MenuItem {
            text: qsTr("Play")
            onTriggered: trackPane.playRequested(trackMenu.start)
        }
        MenuItem {
            text: qsTr("Play Next")
            onTriggered: trackPane.playNextRequested(trackMenu.start, trackMenu.tracks)
        }
        MenuItem {
            text: qsTr("Add to Queue")
            onTriggered: trackPane.addToQueueRequested(trackMenu.start, trackMenu.tracks)
        }
    }

    SortMenu {
        id: sortMenu
        sortColumn: trackPane.sortColumn
        sortDescending: trackPane.sortDescending
        onSortChosen: (column, descending) => trackPane.sortChosen(column, descending)
    }

    Label {
        Layout.fillWidth: true
        padding: 10
        font.pointSize: 16
        font.bold: true
        elide: Text.ElideRight
        textFormat: Text.PlainText
        text: trackPane.loading ? qsTr("Loading…") : trackPane.title
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

    Item {
        id: headings
        Layout.fillWidth: true
        implicitHeight: Theme.rowHeight
        visible: albumList.visible && trackPane.albumCount > 0

        // Behind the headings, which take only left clicks.
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.RightButton
            onClicked: sortMenu.popup()
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: trackPane.tracksLeft + 6
            anchors.rightMargin: trackPane.tracksRight + 6 + albumScroll.gutter
            spacing: 10
            SortHeading {
                Layout.preferredWidth: 32
                horizontalAlignment: Text.AlignRight
                text: trackPane.heading(qsTr("#"), 0)
                onClicked: trackPane.sortClicked(0)
            }
            SortHeading {
                Layout.fillWidth: true
                text: trackPane.heading(qsTr("Title"), 1)
                onClicked: trackPane.sortClicked(1)
            }
            SortHeading {
                Layout.preferredWidth: Math.min(220, headings.width / 4)
                // Album rows show an artist only where it differs, so there is no column to head.
                visible: trackPane.sorted
                text: trackPane.heading(qsTr("Artist"), 2)
                onClicked: trackPane.sortClicked(2)
            }
            SortHeading {
                Layout.preferredWidth: Theme.timeWidth
                horizontalAlignment: Text.AlignRight
                text: trackPane.heading(qsTr("Time"), 3)
                onClicked: trackPane.sortClicked(3)
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
        visible: trackPane.albumCount > 0 || trackPane.hasFolders
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
            speed: trackPane.scrollSpeed
        }
        property int selectedIndex: -1

        delegate: AlbumDelegate {
            width: ListView.view.width
            sorted: trackPane.sorted
            tracksLeft: trackPane.tracksLeft
            tracksRight: trackPane.tracksRight
            selectedIndex: albumList.selectedIndex
            currentTrackId: trackPane.currentTrackId
            gutter: albumScroll.gutter
            onTrackClicked: viewIndex => albumList.selectedIndex = viewIndex
            onPlayRequested: viewIndex => trackPane.playRequested(viewIndex)
            onMenuRequested: (start, tracks) => {
                trackMenu.start = start
                trackMenu.tracks = tracks
                trackMenu.popup()
            }
        }
    }
}
