pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls

// Asks for a playlist's name. Made with createObject, and gone once closed.
Dialog {
    id: nameDialog
    property alias name: field.text
    signal chosen(string name)

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(360, parent.width - 40)
    modal: true
    standardButtons: Dialog.Ok | Dialog.Cancel

    Component.onCompleted: standardButton(Dialog.Ok).enabled = Qt.binding(() => field.text.trim() !== "")
    onOpened: {
        field.selectAll()
        field.forceActiveFocus()
    }
    onAccepted: chosen(field.text)
    onClosed: destroy()

    TextField {
        id: field
        width: parent.width
        onAccepted: if (text.trim() !== "") nameDialog.accept()
    }
}
