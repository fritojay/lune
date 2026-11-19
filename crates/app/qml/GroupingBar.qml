pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

RowLayout {
    id: groupingBar
    required property string grouping
    signal groupingChosen(string grouping)

    spacing: 4

    Repeater {
        model: Groupings.all
        Button {
            required property var modelData
            text: modelData.title
            flat: !checked
            checkable: true
            checked: groupingBar.grouping === modelData.id
            onClicked: {
                checked = Qt.binding(() => groupingBar.grouping === modelData.id)
                groupingBar.groupingChosen(modelData.id)
            }
        }
    }
}
