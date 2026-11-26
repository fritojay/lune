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
            action: ToggleAction {
                text: columnItem.modelData.title
                toggledOn: !WindowLayout.hiddenColumns.includes(columnItem.modelData.id)
                onToggleRequested: on => WindowLayout.setColumnShown(columnItem.modelData.id, on)
            }
        }
    }
}
