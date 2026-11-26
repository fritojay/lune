pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

Pane {
    id: nowPlayingBar
    required property PlayerController controller
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

    component TransportButton: KeyToolButton {
        icon.width: 20
        icon.height: 20
        icon.color: palette.buttonText
    }

    padding: 10
    topPadding: 8

    ColumnLayout {
        anchors.fill: parent
        spacing: 4

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
                spacing: 6
                TransportButton {
                    icon.source: "icons/previous.svg"
                    onClicked: nowPlayingBar.previous()
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("Previous")
                }
                // Not RoundButton: the macOS native style can't be restyled.
                AbstractButton {
                    id: playButton
                    implicitWidth: 40
                    implicitHeight: 40
                    hoverEnabled: true
                    enabled: nowPlayingBar.available
                    contentItem: Item {
                        PathIcon {
                            anchors.centerIn: parent
                            width: 20
                            height: 20
                            name: nowPlayingBar.playing ? "pause" : "play"
                            color: playButton.palette.highlightedText
                        }
                    }
                    background: Rectangle {
                        radius: width / 2
                        color: !playButton.enabled ? playButton.palette.mid
                             : playButton.down ? Qt.darker(playButton.palette.highlight, 1.15)
                             : playButton.hovered ? Qt.lighter(playButton.palette.highlight, 1.1)
                             : playButton.palette.highlight
                        border.width: playButton.visualFocus ? 2 : 0
                        border.color: playButton.palette.highlightedText
                    }
                    onClicked: nowPlayingBar.togglePause()
                    Keys.onReturnPressed: click()
                    Keys.onEnterPressed: click()
                    ToolTip.visible: hovered
                    ToolTip.text: nowPlayingBar.playing ? qsTr("Pause") : qsTr("Play")
                }
                TransportButton {
                    icon.source: "icons/next.svg"
                    onClicked: nowPlayingBar.next()
                    ToolTip.visible: hovered
                    ToolTip.text: qsTr("Next")
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
                KeyToolButton {
                    icon.source: "icons/queue.svg"
                    icon.color: palette.buttonText
                    checked: nowPlayingBar.queueShown
                    onClicked: nowPlayingBar.queueToggled(!checked)
                    ToolTip.visible: hovered
                    ToolTip.text: checked ? qsTr("Hide Queue") : qsTr("Show Queue")
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 8
            Label {
                Layout.preferredWidth: clockWidth.width
                horizontalAlignment: Text.AlignRight
                text: nowPlayingBar.controller.clock(seekBar.pressed ? seekBar.value : nowPlayingBar.position)
                font.features: { "tnum": 1 }
                opacity: 0.7
                TextMetrics {
                    id: clockWidth
                    font.features: { "tnum": 1 }
                    text: nowPlayingBar.controller.clock(Math.max(nowPlayingBar.duration, 600))
                }
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
                Layout.preferredWidth: clockWidth.width
                text: nowPlayingBar.controller.clock(nowPlayingBar.duration)
                font.features: { "tnum": 1 }
                opacity: 0.7
            }
        }
    }
}
