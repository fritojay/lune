pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import com.lune.player

// What can be done with some rows of a track list. Items declared inside a
// TrackMenu come after these.
Menu {
    id: trackMenu
    required property PlayerController controller
    // The list the rows are in: "view" (the main pane) or "queue", as
    // Selection::new in player.rs reads it.
    required property string source
    property int start: 0
    property int tracks: 1
    property real trackId: -1

    signal playRequested(int row)
    signal albumRequested(real trackId)

    // Asks for a name, then makes a playlist of the rows the menu was for.
    function newPlaylist() {
        const source = trackMenu.source
        const start = trackMenu.start
        const tracks = trackMenu.tracks
        const dialog = nameDialog.createObject(trackMenu.parent, {
            title: qsTr("New Playlist"),
            name: playlists.suggestedName(qsTr("New Playlist"))
        })
        dialog.chosen.connect(name => trackMenu.controller.addToPlaylist(source, start, tracks, playlists.create(name)))
        dialog.open()
    }

    Component {
        id: nameDialog
        PlaylistNameDialog {}
    }

    function show(start: int, tracks: int) {
        trackMenu.start = start
        trackMenu.tracks = tracks
        trackId = controller.trackId(source, start)
        popup()
    }

    MenuItem {
        text: qsTr("Play")
        onTriggered: trackMenu.playRequested(trackMenu.start)
    }
    MenuItem {
        text: qsTr("Play Next")
        onTriggered: trackMenu.controller.playNext(trackMenu.source, trackMenu.start, trackMenu.tracks)
    }
    MenuItem {
        text: qsTr("Add to Queue")
        onTriggered: trackMenu.controller.addToQueue(trackMenu.source, trackMenu.start, trackMenu.tracks)
    }
    Menu {
        id: playlistMenu
        title: qsTr("Add to Playlist")
        MenuItem {
            text: qsTr("New Playlist…")
            onTriggered: trackMenu.newPlaylist()
        }
        MenuSeparator {
            visible: playlistItems.count > 0
            height: visible ? implicitHeight : 0
        }
        Instantiator {
            id: playlistItems
            model: PlaylistsModel {
                id: playlists
            }
            delegate: MenuItem {
                required property string playlistId
                required property string label
                // Not a mnemonic.
                text: label.replace(/&/g, "&&")
                onTriggered: trackMenu.controller.addToPlaylist(trackMenu.source, trackMenu.start,
                                                                trackMenu.tracks, playlistId)
            }
            // After New Playlist… and the separator.
            onObjectAdded: (index, object) => playlistMenu.insertItem(index + 2, object)
            onObjectRemoved: (index, object) => playlistMenu.removeItem(object)
        }
    }
    MenuSeparator {}
    MenuItem {
        text: qsTr("Go to Album")
        enabled: trackMenu.trackId >= 0
        onTriggered: trackMenu.albumRequested(trackMenu.trackId)
    }
}
