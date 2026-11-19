pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls

Menu {
    id: sortMenu
    required property string sortColumn
    required property bool sortDescending
    required property var hiddenColumns
    readonly property bool sorted: sortColumn !== ""
    signal sortChosen(string column, bool descending)
    signal columnToggled(string column, bool shown)

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

    component ColumnMenuItem: MenuItem {
        id: columnItem
        required property string column
        required property bool shown
        checkable: true
        checked: shown
        onTriggered: {
            (menu as SortMenu).columnToggled(columnItem.column, !columnItem.shown)
            checked = Qt.binding(() => columnItem.shown)
        }
    }

    SortMenuItem {
        text: qsTr("Album Order")
        column: ""
        descending: false
        selected: !sortMenu.sorted
    }
    MenuSeparator {}
    Repeater {
        model: Columns.all
        SortMenuItem {
            required property var modelData
            text: modelData.menuTitle ?? modelData.title
            column: modelData.id
            descending: sortMenu.sortDescending
            selected: sortMenu.sortColumn === column
        }
    }
    MenuSeparator {}
    SortMenuItem {
        text: qsTr("Ascending")
        enabled: sortMenu.sorted
        column: sortMenu.sortColumn
        descending: false
        selected: sortMenu.sorted && !sortMenu.sortDescending
    }
    SortMenuItem {
        text: qsTr("Descending")
        enabled: sortMenu.sorted
        column: sortMenu.sortColumn
        descending: true
        selected: sortMenu.sorted && sortMenu.sortDescending
    }
    MenuSeparator {}
    Repeater {
        model: Columns.hideable
        ColumnMenuItem {
            required property var modelData
            text: qsTr("Show %1").arg(modelData.title)
            column: modelData.id
            shown: !sortMenu.hiddenColumns.includes(modelData.id)
        }
    }
}
