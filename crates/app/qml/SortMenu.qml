pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls

Menu {
    id: sortMenu
    required property string sortColumn
    required property bool sortDescending
    // Whether album order has a direction in this grouping.
    required property bool albumDirectional
    readonly property bool sorted: sortColumn !== ""
    readonly property bool directional: sorted || albumDirectional
    signal sortChosen(string column, bool descending)

    component SortMenuItem: MenuItem {
        id: sortItem
        required property string column
        required property bool descending
        required property bool selected
        checkable: true
        checked: selected
        onTriggered: {
            (menu as SortMenu).sortChosen(sortItem.column, sortItem.descending)
            // A click toggles `checked`, which breaks its binding.
            checked = Qt.binding(() => sortItem.selected)
        }
    }

    SortMenuItem {
        text: qsTr("Album Order")
        column: ""
        descending: sortMenu.sortDescending
        selected: !sortMenu.sorted
    }
    MenuSeparator {}
    Repeater {
        model: Columns.all
        SortMenuItem {
            required property var modelData
            text: modelData.menuTitle ?? modelData.title
            column: modelData.id
            // Album order's direction is about years, not columns.
            descending: sortMenu.sorted && sortMenu.sortDescending
            selected: sortMenu.sortColumn === column
        }
    }
    MenuSeparator {}
    SortMenuItem {
        text: qsTr("Ascending")
        enabled: sortMenu.directional
        column: sortMenu.sortColumn
        descending: false
        selected: sortMenu.directional && !sortMenu.sortDescending
    }
    SortMenuItem {
        text: qsTr("Descending")
        enabled: sortMenu.directional
        column: sortMenu.sortColumn
        descending: true
        selected: sortMenu.directional && sortMenu.sortDescending
    }
}
