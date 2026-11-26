import QtQuick.Dialogs
import com.lune.player

FolderDialog {
    title: qsTr("Add a music folder")
    options: Library.nativeFolderDialog ? 0 : FolderDialog.DontUseNativeDialog
    onAccepted: Library.addFolder(selectedFolder)
}
