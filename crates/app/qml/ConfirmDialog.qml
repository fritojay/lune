pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls

// Asks before an action. Made with createObject, and gone once closed.
Dialog {
    id: confirm
    required property string text
    required property string acceptText

    parent: Overlay.overlay
    anchors.centerIn: parent
    width: Math.min(440, parent.width - 40)
    modal: true
    standardButtons: Dialog.Ok | Dialog.Cancel
    Component.onCompleted: standardButton(Dialog.Ok).text = acceptText
    onClosed: destroy()

    Label {
        width: parent.width
        wrapMode: Text.WordWrap
        textFormat: Text.PlainText
        text: confirm.text
    }
}
