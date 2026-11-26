import QtQuick
import com.lune.player

// Asks for a name, then makes a playlist of `trackIds`, and with `showMade`
// shows it.
PlaylistNameDialog {
    property list<real> trackIds
    property bool showMade

    title: qsTr("New Playlist")
    name: Playlists.suggestedName(qsTr("New Playlist"))
    onChosen: name => {
        const id = Playlists.create(name, trackIds)
        if (showMade)
            Filters.showPlaylist(id)
    }
}
