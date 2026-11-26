pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// The values of the chosen grouping, or another list with the same roles
// (`label`, `detail`, `count`, `all`).
ColumnLayout {
    id: filterPane
    required property var model
    required property int currentRow
    // Shown under the search field while `headerShown`.
    property alias headerItems: header.data
    property bool headerShown: false
    required property url cover
    required property bool hasTrack
    required property string album
    signal searchEdited(string text)
    signal rowChosen(int row)
    signal rowMenuRequested(int row)
    signal albumRequested()

    function focusSearch() {
        search.forceActiveFocus()
    }

    function clearSearch() {
        search.clear()
        filterList.forceActiveFocus()
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

    ColumnLayout {
        id: header
        Layout.fillWidth: true
        Layout.leftMargin: 6
        Layout.rightMargin: 6
        visible: filterPane.headerShown
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
            required property bool all
            width: ListView.view.width
            height: detail ? 40 : 28
            color: ListView.isCurrentItem ? palette.highlight
                 : rowArea.containsMouse ? Theme.hoverColor(palette) : "transparent"
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
                        font.bold: filterRow.all
                    }
                    Text {
                        text: filterRow.count
                        color: filterRow.textColor
                        opacity: 0.6
                        font.features: { "tnum": 1 }
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
                id: rowArea
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                onClicked: mouse => {
                    if (mouse.button === Qt.RightButton) {
                        filterPane.rowMenuRequested(filterRow.index)
                        return
                    }
                    filterPane.rowChosen(filterRow.index)
                    filterList.forceActiveFocus()
                }
            }
        }
    }

    // The playing track's cover, as large as the pane allows while leaving
    // the filter list at least half the height.
    CoverArt {
        visible: filterPane.hasTrack
        readonly property real side: Math.min(parent.width - 12, parent.height / 2)
        Layout.preferredWidth: side
        Layout.preferredHeight: side
        Layout.alignment: Qt.AlignHCenter
        Layout.bottomMargin: 6
        source: filterPane.cover
        // The thumbnail is already small, and decoding at the width would decode it
        // again at every step of a resize.
        decodeAtSize: false

        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: filterPane.albumRequested()
            ToolTip.visible: containsMouse
            ToolTip.text: qsTr("Go to the album “%1”").arg(filterPane.album)
        }
    }
}
