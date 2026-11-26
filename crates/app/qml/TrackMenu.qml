pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import com.lune.player

// What can be done with some rows of a track list. Items declared inside a
// TrackMenu come after these. Play and Go to Album use the first row.
Menu {
    id: trackMenu
    // The list the rows are in: "view" (the main pane) or "queue", as
    // Selection::new in player.rs reads it.
    required property string source
    // Ascending.
    property list<int> rows
    property real trackId: -1
    property list<real> trackIds

    signal playRequested(int row)

    function show(rows: list<int>) {
        if (rows.length === 0)
            return
        trackMenu.rows = rows
        trackIds = Player.trackIds(source, rows)
        trackId = Player.trackId(source, rows[0])
        popup()
    }

    MenuItem {
        text: qsTr("Play")
        onTriggered: trackMenu.playRequested(trackMenu.rows[0])
    }
    MenuItem {
        text: qsTr("Play Next")
        onTriggered: Player.playNext(trackMenu.source, trackMenu.rows)
    }
    MenuItem {
        text: qsTr("Add to Queue")
        onTriggered: Player.addToQueue(trackMenu.source, trackMenu.rows)
    }
    Menu {
        id: playlistMenu
        title: qsTr("Add to Playlist")
        MenuItem {
            text: qsTr("New Playlist…")
            enabled: Playlists.loaded
            // The tracks the menu was for, as they were when it opened.
            onTriggered: newPlaylist.createObject(trackMenu.parent, { trackIds: trackMenu.trackIds }).open()
        }
        CollapsingMenuSeparator {
            visible: playlistItems.count > 0
        }
        Instantiator {
            id: playlistItems
            model: Playlists
            delegate: MenuItem {
                required property string playlistId
                required property string menuText
                text: menuText
                onTriggered: Player.addToPlaylist(trackMenu.source, trackMenu.rows, playlistId)
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
        onTriggered: Filters.showAlbum(trackMenu.trackId)
    }

    Component {
        id: newPlaylist
        NewPlaylistDialog {}
    }
}
