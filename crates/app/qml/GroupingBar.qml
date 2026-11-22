pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

RowLayout {
    id: groupingBar
    required property string grouping
    signal groupingChosen(string grouping)

    spacing: 2

    Repeater {
        model: Groupings.all
        // Not ToolButton: the macOS native style can't be restyled.
        AbstractButton {
            id: tab
            required property var modelData
            readonly property bool chosen: groupingBar.grouping === modelData.id
            implicitHeight: 34
            leftPadding: 12
            rightPadding: 12
            hoverEnabled: true
            text: modelData.title
            font.bold: chosen
            checkable: true
            checked: chosen
            onClicked: {
                checked = Qt.binding(() => tab.chosen)
                groupingBar.groupingChosen(modelData.id)
            }
            Keys.onReturnPressed: click()
            Keys.onEnterPressed: click()
            contentItem: Item {
                implicitWidth: widest.implicitWidth
                implicitHeight: label.implicitHeight
                // Sized for bold text, so choosing a tab doesn't shift the others.
                Text {
                    id: widest
                    visible: false
                    text: tab.text
                    font.family: tab.font.family
                    font.pointSize: tab.font.pointSize
                    font.bold: true
                }
                Text {
                    id: label
                    anchors.centerIn: parent
                    text: tab.text
                    font: tab.font
                    color: tab.palette.buttonText
                    opacity: tab.chosen || tab.hovered ? 1 : 0.7
                }
            }
            background: Rectangle {
                color: tab.down || tab.hovered ? Theme.tint(tab.palette, 0.07)
                                               : "transparent"
                radius: 4
                border.width: tab.visualFocus ? 1 : 0
                border.color: tab.palette.highlight
                Rectangle {
                    anchors.bottom: parent.bottom
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: parent.width - 16
                    height: 2
                    radius: 1
                    visible: tab.chosen
                    color: tab.palette.highlight
                }
            }
        }
    }
}
