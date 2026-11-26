pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import com.lune.player

// What can be done with some rows of a track list. Items declared inside a
// TrackMenu come after these. Play and Go to Album use the first row.
Menu {
    id: trackMenu
    required property PlayerController controller
    // The list the rows are in: "view" (the main pane) or "queue", as
    // Selection::new in player.rs reads it.
    required property string source
    // Ascending.
    property list<int> rows
    property real trackId: -1

    signal playRequested(int row)
    signal albumRequested(real trackId)

    // Asks for a name, then makes a playlist of the rows the menu was for.
    function newPlaylist() {
        const source = trackMenu.source
        const rows = trackMenu.rows
        const dialog = nameDialog.createObject(trackMenu.parent, {
            title: qsTr("New Playlist"),
            name: playlists.suggestedName(qsTr("New Playlist"))
        })
        dialog.chosen.connect(name => trackMenu.controller.addToPlaylist(source, rows, playlists.create(name)))
        dialog.open()
    }

    Component {
        id: nameDialog
        PlaylistNameDialog {}
    }

    function show(rows: list<int>) {
        if (rows.length === 0)
            return
        trackMenu.rows = rows
        trackId = controller.trackId(source, rows[0])
        popup()
    }

    MenuItem {
        text: qsTr("Play")
        onTriggered: trackMenu.playRequested(trackMenu.rows[0])
    }
    MenuItem {
        text: qsTr("Play Next")
        onTriggered: trackMenu.controller.playNext(trackMenu.source, trackMenu.rows)
    }
    MenuItem {
        text: qsTr("Add to Queue")
        onTriggered: trackMenu.controller.addToQueue(trackMenu.source, trackMenu.rows)
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
                onTriggered: trackMenu.controller.addToPlaylist(trackMenu.source, trackMenu.rows, playlistId)
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
