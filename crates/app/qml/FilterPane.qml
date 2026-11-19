pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The values of the chosen grouping.
ColumnLayout {
    id: filterPane
    required property var model
    required property int currentRow
    required property url cover
    signal searchEdited(string text)
    signal rowChosen(int row)

    function focusSearch() {
        search.forceActiveFocus()
    }

    spacing: 6

    RowLayout {
        Layout.fillWidth: true
        Layout.margins: 6
        Layout.bottomMargin: 0
        TextField {
            id: search
            Layout.fillWidth: true
            placeholderText: qsTr("Search")
            rightPadding: clearSearch.width
            onTextChanged: filterPane.searchEdited(text)
            Keys.onEscapePressed: {
                text = ""
                filterList.forceActiveFocus()
            }

            ToolButton {
                id: clearSearch
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                height: parent.height
                width: height
                visible: search.text !== ""
                // Esc already clears from the keyboard, and taking focus would end the typing.
                focusPolicy: Qt.NoFocus
                icon.source: "icons/clear.svg"
                icon.color: palette.buttonText
                icon.width: 12
                icon.height: 12
                onClicked: search.clear()
            }
        }
    }

    ListView {
        id: filterList
        Layout.fillWidth: true
        Layout.fillHeight: true
        model: filterPane.model
        clip: true
        reuseItems: true
        boundsBehavior: Flickable.StopAtBounds
        maximumFlickVelocity: Theme.maxFlickVelocity
        currentIndex: filterPane.currentRow
        keyNavigationEnabled: true
        onCurrentIndexChanged: filterPane.rowChosen(currentIndex)
        ScrollBar.vertical: ListScrollBar {
            id: filterScroll
        }
        WheelScroller {
            view: filterList
        }

        delegate: Rectangle {
            id: filterRow
            required property int index
            required property string label
            required property string detail
            required property int count
            width: ListView.view.width
            height: detail ? 40 : 28
            color: ListView.isCurrentItem ? palette.highlight : "transparent"
            readonly property color textColor: ListView.isCurrentItem ? palette.highlightedText : palette.text

            ColumnLayout {
                anchors.fill: parent
                anchors.leftMargin: 10
                anchors.rightMargin: 10 + filterScroll.gutter
                spacing: 0
                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        Layout.fillWidth: true
                        text: filterRow.label
                        textFormat: Text.PlainText
                        elide: Text.ElideRight
                        color: filterRow.textColor
                        font.bold: filterRow.index === 0
                    }
                    Text {
                        text: filterRow.count
                        color: filterRow.textColor
                        opacity: 0.6
                    }
                }
                Text {
                    Layout.fillWidth: true
                    visible: filterRow.detail !== ""
                    text: filterRow.detail
                    textFormat: Text.PlainText
                    elide: Text.ElideRight
                    color: filterRow.textColor
                    opacity: 0.7
                    font.pointSize: 9
                }
            }

            MouseArea {
                anchors.fill: parent
                onClicked: {
                    filterPane.rowChosen(filterRow.index)
                    filterList.forceActiveFocus()
                }
            }
        }
    }

    // The playing track's cover, as large as the pane allows while leaving
    // the filter list at least half the height.
    CoverArt {
        readonly property real side: Math.min(parent.width - 12, parent.height / 2)
        Layout.preferredWidth: side
        Layout.preferredHeight: side
        Layout.alignment: Qt.AlignHCenter
        Layout.bottomMargin: 6
        source: filterPane.cover
        // The thumbnail is already small, and decoding at the width would decode it
        // again at every step of a resize.
        decodeAtSize: false
    }
}
