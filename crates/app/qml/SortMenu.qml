pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls

Menu {
    id: sortMenu
    required property int sortColumn
    required property bool sortDescending
    readonly property bool sorted: sortColumn >= 0
    signal sortChosen(int column, bool descending)

    component SortMenuItem: MenuItem {
        id: sortItem
        required property int column
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
        column: -1
        descending: false
        selected: !sortMenu.sorted
    }
    MenuSeparator {}
    SortMenuItem {
        text: qsTr("Track Number")
        column: 0
        descending: sortMenu.sortDescending
        selected: sortMenu.sortColumn === column
    }
    SortMenuItem {
        text: qsTr("Title")
        column: 1
        descending: sortMenu.sortDescending
        selected: sortMenu.sortColumn === column
    }
    SortMenuItem {
        text: qsTr("Artist")
        column: 2
        descending: sortMenu.sortDescending
        selected: sortMenu.sortColumn === column
    }
    SortMenuItem {
        text: qsTr("Time")
        column: 3
        descending: sortMenu.sortDescending
        selected: sortMenu.sortColumn === column
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
}
