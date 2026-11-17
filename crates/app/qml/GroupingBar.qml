pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

RowLayout {
    id: groupingBar
    required property int grouping
    signal groupingChosen(int grouping)

    spacing: 4

    Repeater {
        model: [qsTr("Album Artists"), qsTr("Artists"), qsTr("Albums"), qsTr("Years"),
            qsTr("Recently Added"), qsTr("Recently Played")]
        Button {
            required property int index
            required property string modelData
            text: modelData
            flat: !checked
            checkable: true
            checked: groupingBar.grouping === index
            onClicked: {
                checked = Qt.binding(() => groupingBar.grouping === index)
                groupingBar.groupingChosen(index)
            }
        }
    }
}
