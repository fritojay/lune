pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import com.lune.player

// What can be done with one playlist in the Playlists pane.
Menu {
    id: playlistMenu
    property string playlistId
    property string name

    function show(row: int) {
        playlistId = Playlists.idAt(row)
        name = Playlists.nameAt(row)
        if (playlistId !== "")
            popup()
    }

    MenuItem {
        text: qsTr("Rename…")
        onTriggered: {
            const id = playlistMenu.playlistId
            const dialog = nameDialog.createObject(playlistMenu.parent,
                                                   { title: qsTr("Rename Playlist"), name: playlistMenu.name })
            dialog.chosen.connect(name => Playlists.rename(id, name))
            dialog.open()
        }
    }
    MenuItem {
        text: qsTr("Delete…")
        onTriggered: confirmDelete.createObject(playlistMenu.parent,
                                                { playlistId: playlistMenu.playlistId, name: playlistMenu.name }).open()
    }

    Component {
        id: nameDialog
        PlaylistNameDialog {}
    }

    Component {
        id: confirmDelete
        Dialog {
            id: confirm
            required property string playlistId
            required property string name
            parent: Overlay.overlay
            anchors.centerIn: parent
            width: Math.min(400, parent.width - 40)
            modal: true
            title: qsTr("Delete Playlist")
            standardButtons: Dialog.Ok | Dialog.Cancel
            Component.onCompleted: standardButton(Dialog.Ok).text = qsTr("Delete")
            onAccepted: Playlists.remove(playlistId)
            onClosed: destroy()
            Label {
                width: parent.width
                wrapMode: Text.WordWrap
                textFormat: Text.PlainText
                text: qsTr("Delete “%1”? Its tracks stay in your library.").arg(confirm.name)
            }
        }
    }
}
