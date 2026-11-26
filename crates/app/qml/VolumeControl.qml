import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import com.lune.player

RowLayout {
    spacing: 4

    KeyToolButton {
        id: speaker
        icon.source: Player.muted ? "icons/volume-muted.svg"
                   : Player.volume < 0.5 ? "icons/volume-low.svg"
                   : "icons/volume-high.svg"
        icon.color: palette.buttonText
        // The first click shows the slider; clicking again while it shows mutes.
        onClicked: volumePopup.opened ? Player.toggleMute() : volumePopup.open()

        Popup {
            id: volumePopup
            x: (speaker.width - width) / 2
            y: -height - 4
            padding: 0
            focus: true
            closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent

            contentItem: Item {
                implicitWidth: volumeSlider.implicitWidth + 16
                implicitHeight: 136

                Slider {
                    id: volumeSlider
                    anchors.fill: parent
                    anchors.margins: 8
                    focus: true
                    orientation: Qt.Vertical
                    from: 0
                    to: 1
                    value: Player.volume
                    onMoved: Player.setVolume(value)
                }
            }
        }
    }
    Label {
        id: volumeLabel
        // Wide enough for "100%", so the speaker doesn't shift.
        Layout.preferredWidth: fullVolume.width
        text: qsTr("%1%").arg(Math.round(Player.volume * 100))
        opacity: 0.7
        TextMetrics {
            id: fullVolume
            font: volumeLabel.font
            text: qsTr("%1%").arg(100)
        }
    }
}
