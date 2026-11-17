pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: shortcutsDialog
    // [key, description] pairs. An entry with no key text isn't listed, so a
    // platform without a binding for it leaves it out.
    required property var shortcuts

    title: qsTr("Keyboard Shortcuts")
    modal: true
    standardButtons: Dialog.Close

    GridLayout {
        anchors.fill: parent
        columns: 2
        columnSpacing: 24
        rowSpacing: 6

        Repeater {
            model: shortcutsDialog.shortcuts.filter(entry => entry[0] !== "").reduce((cells, entry) => cells.concat(entry), [])

            Label {
                required property int index
                required property string modelData
                Layout.fillWidth: index % 2 === 1
                text: modelData
                font.bold: index % 2 === 0
                wrapMode: Text.WordWrap
            }
        }
    }
}
