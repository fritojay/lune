pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Dialogs
import QtQuick.Layouts
import com.lune.player

ApplicationWindow {
    id: window
    width: layout.width
    height: layout.height
    visibility: layout.maximized ? Window.Maximized : Window.Windowed
    title: player.hasTrack ? qsTr("%1 – %2").arg(player.title).arg(player.artist) : qsTr("Lune")

    property bool queueShown: layout.queueShown
    // The size while not maximized, which is what gets saved.
    property real normalWidth: layout.width
    property real normalHeight: layout.height

    onWidthChanged: if (visibility === Window.Windowed) normalWidth = width
    onHeightChanged: if (visibility === Window.Windowed) normalHeight = height

    Component.onCompleted: layout.fit(Screen.desktopAvailableWidth, Screen.desktopAvailableHeight,
                                      mainPane.SplitView.minimumWidth)

    WindowLayout {
        id: layout
    }

    Connections {
        target: Qt.application
        function onAboutToQuit() {
            layout.save(window.normalWidth, window.normalHeight, window.visibility === Window.Maximized,
                        filterPane.SplitView.preferredWidth, queuePane.SplitView.preferredWidth, window.queueShown)
        }
    }

    // Native on macOS, where Qt moves "About…", "Preferences…", and "Quit" into the application menu by their text.
    menuBar: MenuBar {
        Menu {
            title: qsTr("&File")
            Action {
                text: qsTr("&Preferences…")
                shortcut: StandardKey.Preferences
                onTriggered: settings.open()
            }
            Action {
                text: qsTr("&Unavailable Tracks (%1)…").arg(library.missingCount)
                onTriggered: unavailableList.open()
            }
            MenuSeparator {}
            Action {
                text: Qt.platform.os === "windows" ? qsTr("E&xit") : qsTr("&Quit")
                shortcut: StandardKey.Quit
                onTriggered: Qt.quit()
            }
        }
        Menu {
            title: qsTr("&View")
            Action {
                text: qsTr("Albums as &List")
                shortcut: listKey.sequence
                checkable: true
                checked: !layout.albumGrid
                onTriggered: {
                    layout.albumGrid = false
                    // Triggering toggles `checked`, which breaks its binding.
                    checked = Qt.binding(() => !layout.albumGrid)
                }
            }
            Action {
                text: qsTr("Albums as &Grid")
                shortcut: gridKey.sequence
                checkable: true
                checked: layout.albumGrid
                onTriggered: {
                    layout.albumGrid = true
                    checked = Qt.binding(() => layout.albumGrid)
                }
            }
            MenuSeparator {}
            SortMenu {
                title: qsTr("&Sort")
                sortColumn: filters.sortColumn
                sortDescending: filters.sortDescending
                albumDirectional: filters.albumDirectional
                onSortChosen: (column, descending) => filters.setSort(column, descending)
            }
            ColumnsMenu {
                title: qsTr("&Columns")
                hiddenColumns: layout.hiddenColumns
                onColumnToggled: (column, shown) => layout.setColumnShown(column, shown)
            }
        }
        Menu {
            title: qsTr("&Help")
            Action {
                text: qsTr("&Keyboard Shortcuts")
                onTriggered: shortcutList.open()
            }
            MenuSeparator {}
            Action {
                text: qsTr("&About Lune…")
                onTriggered: about.open()
            }
        }
    }

    LibraryManager {
        id: library
    }

    MissingModel {
        id: missingTracks
    }

    FilterModel {
        id: filters
        onSearchCleared: filterPane.clearSearch()
    }

    PlaylistsModel {
        id: playlistsModel
        onPlaylistChosen: id => filters.showPlaylist(id)
    }

    AlbumListModel {
        id: albums
        // A rescan keeps the list where it was.
        onModelAboutToBeReset: trackPane.list.keepPosition()
        onRefreshed: reset => trackPane.resetView(reset)
    }

    AlbumGridModel {
        id: albumGrid
        onModelAboutToBeReset: trackPane.keepGridPosition()
        onRefreshed: reset => trackPane.resetGrid(reset)
    }

    AlbumListModel {
        id: openAlbum
        oneAlbum: true
        albumId: trackPane.shownAlbumId
    }

    QueueModel {
        id: queueModel
        currentRow: player.queueIndex
    }

    PlayerController {
        id: player
    }

    BenchProbe {
        id: bench
        onListFilled: benchRun.start()
    }

    onFrameSwapped: bench.firstFrame()

    // Also reconnects a lost audio output, so it runs while stopped too.
    Timer {
        interval: player.playing ? 100 : 250
        running: true
        repeat: true
        onTriggered: player.poll()
    }

    Shortcut {
        id: playPauseKey
        sequence: "Space"
        onActivated: player.togglePause()
    }
    Shortcut {
        id: seekBackKey
        sequence: "Left"
        onActivated: player.seekBy(-5)
    }
    Shortcut {
        id: seekForwardKey
        sequence: "Right"
        onActivated: player.seekBy(5)
    }
    Shortcut {
        id: previousKey
        sequence: "Ctrl+Left"
        onActivated: player.previous()
    }
    Shortcut {
        id: nextKey
        sequence: "Ctrl+Right"
        onActivated: player.next()
    }
    Shortcut {
        id: findKey
        sequences: [StandardKey.Find]
        onActivated: filterPane.focusSearch()
    }
    // Only name the menu items' keys for the shortcut list, which reads
    // each key from its Shortcut so it shows what this platform binds.
    Shortcut {
        id: preferencesKey
        sequences: [StandardKey.Preferences]
        enabled: false
    }
    Shortcut {
        id: listKey
        sequence: "Ctrl+1"
        enabled: false
    }
    Shortcut {
        id: gridKey
        sequence: "Ctrl+2"
        enabled: false
    }

    ShortcutsDialog {
        id: shortcutList
        anchors.centerIn: parent
        width: Math.min(480, window.width - 40)
        shortcuts: [
            [playPauseKey.nativeText, qsTr("Play or pause")],
            [seekBackKey.nativeText, qsTr("Back 5 seconds")],
            [seekForwardKey.nativeText, qsTr("Forward 5 seconds")],
            [previousKey.nativeText, qsTr("Previous track")],
            [nextKey.nativeText, qsTr("Next track")],
            [findKey.nativeText, qsTr("Search")],
            [qsTr("Esc"), qsTr("Clear the search (while typing in it)")],
            [qsTr("Up / Down"), qsTr("Previous or next entry in the filter list")],
            [qsTr("Return"), qsTr("Press the focused speaker or queue button")],
            [qsTr("Up / Down"), qsTr("Change the volume (while its slider is open)")],
            [qsTr("Esc"), qsTr("Close the volume slider")],
            [listKey.nativeText, qsTr("Show albums as a list")],
            [gridKey.nativeText, qsTr("Show albums as a grid")],
            [preferencesKey.nativeText, qsTr("Preferences")],
        ]
    }

    AboutDialog {
        id: about
        anchors.centerIn: parent
        width: Math.min(400, window.width - 40)
    }

    FolderDialog {
        id: folderDialog
        title: qsTr("Add a music folder")
        options: library.nativeFolderDialog ? 0 : FolderDialog.DontUseNativeDialog
        onAccepted: library.addFolder(selectedFolder)
    }

    PreferencesDialog {
        id: settings
        anchors.centerIn: parent
        width: Math.min(560, window.width - 40)
        folders: library.folders
        busy: library.busy
        status: library.status
        onAddFolderRequested: folderDialog.open()
        onRemoveFolderRequested: index => library.removeFolder(index)
        onRescanRequested: library.rescan()
    }

    UnavailableDialog {
        id: unavailableList
        anchors.centerIn: parent
        width: Math.min(720, window.width - 40)
        height: Math.min(560, window.height - 40)
        model: missingTracks
        count: library.missingCount
        onForgetRequested: trackId => library.forgetMissing(trackId)
        onForgetAllRequested: library.forgetAllMissing()
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Errors from the library thread (settings file, database).
        ErrorBanner {
            Layout.fillWidth: true
            message: library.error
            onDismissed: library.clearError()
        }

        // The settings could not be saved.
        ErrorBanner {
            Layout.fillWidth: true
            message: Preferences.error
            onDismissed: Preferences.clearError()
        }

        // Quick filters: picks which grouping the filter pane lists.
        GroupingBar {
            Layout.fillWidth: true
            Layout.leftMargin: 6
            Layout.topMargin: 4
            onSourceChosen: (kind, sourceId) => {
                if (kind === "grouping")
                    filters.setGrouping(sourceId)
                else if (kind === "playlists")
                    filters.showPlaylists()
            }
        }

        Rectangle {
            Layout.fillWidth: true
            implicitHeight: 1
            color: window.palette.mid
        }

        SplitView {
            id: panes
            Layout.fillWidth: true
            Layout.fillHeight: true

            // A thin visible line, with a wider grab area so it stays easy to drag.
            handle: Rectangle {
                id: paneHandle
                implicitWidth: 1
                color: SplitHandle.pressed || SplitHandle.hovered ? window.palette.highlight : window.palette.mid
                containmentMask: Item {
                    x: (paneHandle.width - width) / 2
                    width: 9
                    height: panes.height
                }
            }

            FilterPane {
                id: filterPane
                SplitView.preferredWidth: layout.filterPaneWidth
                SplitView.minimumWidth: 160
                // The playlists stand in for a grouping's values.
                readonly property bool listsPlaylists: filters.sourceKind === "playlists"
                readonly property var rows: listsPlaylists ? playlistsModel : filters
                model: rows
                currentRow: rows.currentRow
                cover: player.cover
                hasTrack: player.hasTrack
                album: player.album
                onSearchEdited: text => filters.setSearch(text)
                onRowChosen: row => rows.select(row)
                onRowMenuRequested: row => {
                    if (listsPlaylists)
                        playlistMenu.show(row)
                }
                headerShown: listsPlaylists
                headerItems: Button {
                    Layout.fillWidth: true
                    enabled: playlistsModel.loaded
                    text: qsTr("New Playlist…")
                    onClicked: window.newPlaylist([], true)
                }
                onAlbumRequested: filters.showAlbum(player.currentTrackId)
            }

            ColumnLayout {
                id: mainPane
                SplitView.fillWidth: true
                SplitView.minimumWidth: Math.max(400, nowPlayingBar.minimumWidth)
                spacing: 0

                TrackPane {
                    id: trackPane
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    model: albums
                    gridModel: albumGrid
                    openAlbumModel: openAlbum
                    albumCount: albums.albumCount
                    trackCount: filters.trackCount
                    title: filters.title
                    loading: filters.loading
                    sortColumn: filters.sortColumn
                    sortDescending: filters.sortDescending
                    albumDirectional: filters.albumDirectional
                    hasFolders: library.folders.length > 0
                    scanStatus: library.busy ? library.status : ""
                    currentTrackId: player.currentTrackId
                    hiddenColumns: layout.hiddenColumns
                    columnWidths: layout.columnWidths(trackPane.rowWidth, layout.hiddenColumns)
                    albumGrid: layout.albumGrid
                    controller: player
                    playlists: playlistsModel
                    onNewPlaylistRequested: trackIds => window.newPlaylist(trackIds, false)
                    flat: filters.flat
                    playlistShown: filterPane.listsPlaylists
                    reorderable: filters.reorderable
                    onRemoveFromPlaylistRequested: rows => playlistsModel.removeTracks(rows)
                    onMoveRequested: (rows, to) => playlistsModel.moveTracks(rows, to)
                    emptyHint: filters.emptyReason
                    onAlbumGridChosen: grid => layout.albumGrid = grid
                    onSortClicked: column => filters.sortBy(column)
                    onSortChosen: (column, descending) => filters.setSort(column, descending)
                    onColumnToggled: (column, shown) => layout.setColumnShown(column, shown)
                    onPlayRequested: start => window.playView(start)
                    onAlbumRequested: trackId => filters.showAlbum(trackId)
                    onAddFolderRequested: folderDialog.open()
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 1
                    color: window.palette.mid
                }

                NowPlayingBar {
                    id: nowPlayingBar
                    Layout.fillWidth: true
                    controller: player
                    available: player.available
                    hasTrack: player.hasTrack
                    playing: player.playing
                    title: player.title
                    artist: player.artist
                    album: player.album
                    position: player.position
                    duration: player.duration
                    volume: player.volume
                    muted: player.muted
                    queueShown: window.queueShown
                    onPrevious: player.previous()
                    onTogglePause: player.togglePause()
                    onNext: player.next()
                    onSeek: seconds => player.seek(seconds)
                    onVolumeMoved: volume => player.setVolume(volume)
                    onMuteToggled: player.toggleMute()
                    onQueueToggled: shown => window.queueShown = shown
                }
            }

            QueuePane {
                id: queuePane
                visible: window.queueShown
                SplitView.preferredWidth: layout.queuePaneWidth
                SplitView.minimumWidth: 200
                model: queueModel
                count: queueModel.count
                currentRow: queueModel.currentRow
                totalDuration: queueModel.totalDuration
                shuffle: player.shuffle
                controller: player
                playlists: playlistsModel
                onNewPlaylistRequested: trackIds => window.newPlaylist(trackIds, false)
                onShuffleToggled: shuffle => player.setShuffle(shuffle)
                onClearRequested: player.clearQueue()
                onPlayRequested: row => player.play("queue", row)
                onRemoveRequested: rows => player.removeQueueRows(rows)
                onMoveRequested: (rows, to) => player.moveQueueRows(rows, to)
                onAlbumRequested: trackId => filters.showAlbum(trackId)
            }
        }
    }

    PlaylistMenu {
        id: playlistMenu
        playlists: playlistsModel
    }

    Component {
        id: playlistNameDialog
        PlaylistNameDialog {}
    }

    // Asks for a name, then makes a playlist of `trackIds`, and with `show`
    // shows it.
    function newPlaylist(trackIds: list<real>, show: bool) {
        const dialog = playlistNameDialog.createObject(window.contentItem, {
            title: qsTr("New Playlist"),
            name: playlistsModel.suggestedName(qsTr("New Playlist"))
        })
        dialog.chosen.connect(name => {
            const id = playlistsModel.create(name, trackIds)
            if (show)
                filters.showPlaylist(id)
        })
        dialog.open()
    }

    function playView(index) {
        player.play("view", index)
        queuePane.showCurrent()
    }

    BenchRun {
        id: benchRun
        list: trackPane.list
        grid: trackPane.gridList
        onGridRequested: layout.albumGrid = true
        onPhaseStarted: name => bench.beginPhase(name)
        onFrame: seconds => bench.frame(seconds)
        onFinished: {
            bench.finish(window.screen.refreshRate)
            Qt.quit()
        }
    }
}
