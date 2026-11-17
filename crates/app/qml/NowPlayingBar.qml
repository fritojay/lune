pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Pane {
    id: nowPlayingBar
    required property bool available
    required property bool hasTrack
    required property bool playing
    required property string title
    required property string artist
    required property string album
    required property real position
    required property real duration
    required property real volume
    required property bool muted
    required property bool queueShown
    // Room for the centered transport with the side controls beside it.
    readonly property real minimumWidth: transport.implicitWidth + 2 * (sideControls.implicitWidth + 8) + 16

    signal previous()
    signal togglePause()
    signal next()
    signal seek(real seconds)
    signal volumeMoved(real volume)
    signal muteToggled()
    signal queueToggled(bool shown)

    function clock(seconds: real): string {
        const s = Math.max(0, Math.floor(seconds))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    // Sized by the symbol at 14 pt but drawn larger, so the symbol fills more of
    // the button without the button growing.
    component TransportButton: ToolButton {
        id: transportButton
        font.pointSize: 14
        contentItem: Item {
            implicitWidth: sizer.implicitWidth
            implicitHeight: sizer.implicitHeight
            Text {
                id: sizer
                visible: false
                text: transportButton.text
                font: transportButton.font
            }
            Text {
                anchors.centerIn: parent
                text: transportButton.text
                font.pointSize: 20
                color: transportButton.palette.buttonText
            }
        }
    }

    padding: 8

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Item {
            id: nowPlayingLine
            Layout.fillWidth: true
            implicitHeight: line.implicitHeight
            clip: true

            readonly property real gap: 48
            readonly property bool scrolls: nowPlayingBar.playing && line.width > width
            readonly property string text: nowPlayingBar.title + nowPlayingBar.artist + nowPlayingBar.album
            readonly property string heading: nowPlayingBar.hasTrack ? nowPlayingBar.title
                : (nowPlayingBar.available ? qsTr("Nothing playing") : qsTr("No audio output"))
            readonly property string detail: nowPlayingBar.hasTrack ? nowPlayingBar.artist + " — " + nowPlayingBar.album : ""

            component TrackLine: Row {
                id: trackLine
                property string heading
                property string detail
                spacing: 8
                Label {
                    text: trackLine.heading
                    textFormat: Text.PlainText
                    font.bold: true
                }
                Label {
                    visible: trackLine.detail !== ""
                    text: trackLine.detail
                    textFormat: Text.PlainText
                    opacity: 0.7
                }
            }

            // A line too long to fit scrolls like a news ticker while playing. The
            // second copy follows the first, so the loop restarts without a jump.
            Row {
                id: ticker
                spacing: nowPlayingLine.gap
                TrackLine {
                    id: line
                    heading: nowPlayingLine.heading
                    detail: nowPlayingLine.detail
                }
                TrackLine {
                    visible: nowPlayingLine.scrolls
                    heading: nowPlayingLine.heading
                    detail: nowPlayingLine.detail
                }
            }

            SequentialAnimation {
                id: tickerAnimation
                running: nowPlayingLine.scrolls
                loops: Animation.Infinite
                // Stopping leaves the line where it was; a paused or changed track reads from the start.
                onRunningChanged: if (!running) ticker.x = 0
                PauseAnimation {
                    duration: 2000
                }
                NumberAnimation {
                    target: ticker
                    property: "x"
                    from: 0
                    to: -(line.width + nowPlayingLine.gap)
                    duration: (line.width + nowPlayingLine.gap) * 1000 / 40
                }
            }

            onTextChanged: {
                ticker.x = 0
                if (tickerAnimation.running)
                    tickerAnimation.restart()
            }
        }

        Item {
            Layout.fillWidth: true
            implicitHeight: transport.implicitHeight

            RowLayout {
                id: transport
                anchors.centerIn: parent
                spacing: 0
                TransportButton {
                    text: "⏮"
                    onClicked: nowPlayingBar.previous()
                }
                TransportButton {
                    text: nowPlayingBar.playing ? "⏸" : "▶"
                    onClicked: nowPlayingBar.togglePause()
                }
                TransportButton {
                    text: "⏭"
                    onClicked: nowPlayingBar.next()
                }
            }

            RowLayout {
                id: sideControls
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 4
                VolumeControl {
                    volume: nowPlayingBar.volume
                    muted: nowPlayingBar.muted
                    onVolumeMoved: volume => nowPlayingBar.volumeMoved(volume)
                    onMuteToggled: nowPlayingBar.muteToggled()
                }
                ToolButton {
                    icon.source: "icons/queue.svg"
                    icon.color: palette.buttonText
                    checkable: true
                    checked: nowPlayingBar.queueShown
                    onClicked: {
                        nowPlayingBar.queueToggled(checked)
                        // A click toggles `checked`, which breaks its binding.
                        checked = Qt.binding(() => nowPlayingBar.queueShown)
                    }
                    ToolTip.visible: hovered
                    ToolTip.text: checked ? qsTr("Hide Queue") : qsTr("Show Queue")
                    Keys.onReturnPressed: click()
                    Keys.onEnterPressed: click()
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            Label {
                text: nowPlayingBar.clock(seekBar.pressed ? seekBar.value : nowPlayingBar.position)
            }
            Slider {
                id: seekBar
                Layout.fillWidth: true
                from: 0
                to: Math.max(nowPlayingBar.duration, 0.001)
                enabled: nowPlayingBar.hasTrack
                // Follows the player except while dragged; seeks on release.
                Binding on value {
                    when: !seekBar.pressed
                    value: nowPlayingBar.position
                }
                onPressedChanged: if (!pressed) nowPlayingBar.seek(value)
            }
            Label {
                text: nowPlayingBar.clock(nowPlayingBar.duration)
            }
        }
    }
}
