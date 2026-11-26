pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

ApplicationWindow {
    id: window
    width: WindowLayout.width
    height: WindowLayout.height
    visibility: WindowLayout.maximized ? Window.Maximized : Window.Windowed
    title: Player.hasTrack ? qsTr("%1 – %2").arg(Player.title).arg(Player.artist) : qsTr("Lune")

    property bool queueShown: WindowLayout.queueShown
    // The size while not maximized, which is what gets saved.
    property real normalWidth: WindowLayout.width
    property real normalHeight: WindowLayout.height

    onWidthChanged: if (visibility === Window.Windowed) normalWidth = width
    onHeightChanged: if (visibility === Window.Windowed) normalHeight = height

    Component.onCompleted: WindowLayout.fit(Screen.desktopAvailableWidth, Screen.desktopAvailableHeight,
                                            mainPane.SplitView.minimumWidth)

    Connections {
        target: Qt.application
        function onAboutToQuit() {
            WindowLayout.save(window.normalWidth, window.normalHeight, window.visibility === Window.Maximized,
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
                text: qsTr("&Unavailable Tracks (%1)…").arg(Library.missingCount)
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
                checked: !WindowLayout.albumGrid
                onTriggered: {
                    WindowLayout.albumGrid = false
                    // Triggering toggles `checked`, which breaks its binding.
                    checked = Qt.binding(() => !WindowLayout.albumGrid)
                }
            }
            Action {
                text: qsTr("Albums as &Grid")
                shortcut: gridKey.sequence
                checkable: true
                checked: WindowLayout.albumGrid
                onTriggered: {
                    WindowLayout.albumGrid = true
                    checked = Qt.binding(() => WindowLayout.albumGrid)
                }
            }
            MenuSeparator {}
            SortMenu {
                title: qsTr("&Sort")
            }
            ColumnsMenu {
                title: qsTr("&Columns")
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

    BenchProbe {
        id: bench
        onListFilled: benchRun.start()
    }

    onFrameSwapped: bench.firstFrame()

    Shortcut {
        id: playPauseKey
        sequence: "Space"
        onActivated: Player.togglePause()
    }
    Shortcut {
        id: seekBackKey
        sequence: "Left"
        onActivated: Player.seekBy(-5)
    }
    Shortcut {
        id: seekForwardKey
        sequence: "Right"
        onActivated: Player.seekBy(5)
    }
    Shortcut {
        id: previousKey
        sequence: "Ctrl+Left"
        onActivated: Player.previous()
    }
    Shortcut {
        id: nextKey
        sequence: "Ctrl+Right"
        onActivated: Player.next()
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

    PreferencesDialog {
        id: settings
        anchors.centerIn: parent
        width: Math.min(560, window.width - 40)
    }

    UnavailableDialog {
        id: unavailableList
        anchors.centerIn: parent
        width: Math.min(720, window.width - 40)
        height: Math.min(560, window.height - 40)
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Errors from the library thread (settings file, database).
        ErrorBanner {
            Layout.fillWidth: true
            message: Library.error
            onDismissed: Library.clearError()
        }

        // The settings could not be saved.
        ErrorBanner {
            Layout.fillWidth: true
            message: Preferences.error
            onDismissed: Preferences.clearError()
        }

        // Quick filters: picks which grouping the filter pane lists. Scrolls
        // sideways when the window is too narrow for every tab.
        Flickable {
            Layout.fillWidth: true
            Layout.leftMargin: 6
            Layout.topMargin: 4
            implicitHeight: groupingBar.implicitHeight
            contentWidth: groupingBar.implicitWidth
            clip: true
            interactive: contentWidth > width
            flickableDirection: Flickable.HorizontalFlick
            boundsBehavior: Flickable.StopAtBounds

            GroupingBar {
                id: groupingBar
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
                SplitView.preferredWidth: WindowLayout.filterPaneWidth
                SplitView.minimumWidth: 160
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
                    onPlayRequested: start => window.playView(start)
                }

                Rectangle {
                    Layout.fillWidth: true
                    implicitHeight: 1
                    color: window.palette.mid
                }

                NowPlayingBar {
                    id: nowPlayingBar
                    Layout.fillWidth: true
                    queueShown: window.queueShown
                    onQueueToggled: shown => window.queueShown = shown
                }
            }

            QueuePane {
                id: queuePane
                visible: window.queueShown
                SplitView.preferredWidth: WindowLayout.queuePaneWidth
                SplitView.minimumWidth: 200
            }
        }
    }

    function playView(index) {
        Player.play("view", index)
        queuePane.showCurrent()
    }

    BenchRun {
        id: benchRun
        list: trackPane.list
        grid: trackPane.gridList
        onGridRequested: WindowLayout.albumGrid = true
        onPhaseStarted: name => bench.beginPhase(name)
        onFrame: seconds => bench.frame(seconds)
        onFinished: {
            bench.finish(window.screen.refreshRate)
            Qt.quit()
        }
    }
}
