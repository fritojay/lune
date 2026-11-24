pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls

Menu {
    id: columnsMenu
    required property var hiddenColumns
    signal columnToggled(string column, bool shown)

    Repeater {
        model: Columns.hideable
        MenuItem {
            id: columnItem
            required property var modelData
            readonly property bool shown: !columnsMenu.hiddenColumns.includes(modelData.id)
            text: modelData.title
            checkable: true
            checked: shown
            onTriggered: {
                columnsMenu.columnToggled(modelData.id, !shown)
                // A click toggles `checked`, which breaks its binding.
                checked = Qt.binding(() => columnItem.shown)
            }
        }
    }
}
