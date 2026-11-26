pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import com.lune.player

Menu {
    Repeater {
        model: Columns.hideable
        MenuItem {
            id: columnItem
            required property var modelData
            readonly property bool shown: !WindowLayout.hiddenColumns.includes(modelData.id)
            text: modelData.title
            checkable: true
            checked: shown
            onTriggered: {
                WindowLayout.setColumnShown(modelData.id, !shown)
                // A click toggles `checked`, which breaks its binding.
                checked = Qt.binding(() => columnItem.shown)
            }
        }
    }
}
