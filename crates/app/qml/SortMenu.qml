pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import com.lune.player

Menu {
    id: sortMenu
    readonly property var items: Filters.sortMenu

    component SortMenuItem: MenuItem {
        id: sortItem
        required property var item
        text: item.text
        enabled: item.enabled
        checkable: true
        checked: item.checked
        onTriggered: {
            Filters.setSort(sortItem.item.column, sortItem.item.descending)
            // A click toggles `checked`, which breaks its binding.
            checked = Qt.binding(() => sortItem.item.checked)
        }
    }

    SortMenuItem {
        item: sortMenu.items.albumOrder
    }
    MenuSeparator {}
    Repeater {
        // One item per column, the same ones whatever the sort, so the
        // items aren't made again on each change.
        model: Columns.all
        SortMenuItem {
            required property int index
            item: sortMenu.items.columns[index]
        }
    }
    MenuSeparator {}
    SortMenuItem {
        item: sortMenu.items.directions[0]
    }
    SortMenuItem {
        item: sortMenu.items.directions[1]
    }
}
