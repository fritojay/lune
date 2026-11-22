import QtQuick
import com.lune.player

Rectangle {
    id: coverArt
    required property url source
    property bool placeholder: true
    property bool decodeAtSize: true

    color: Theme.tint(palette, 0.08)

    Image {
        id: image
        anchors.fill: parent
        source: coverArt.source
        asynchronous: true
        fillMode: Image.PreserveAspectFit
        sourceSize.width: coverArt.decodeAtSize ? width * Screen.devicePixelRatio : -1
        sourceSize.height: coverArt.decodeAtSize ? height * Screen.devicePixelRatio : -1
    }

    Loader {
        anchors.centerIn: parent
        active: coverArt.placeholder && image.status !== Image.Ready
        sourceComponent: PathIcon {
            name: "note"
            width: Math.round(coverArt.height / 3)
            height: width
            color: coverArt.palette.text
            opacity: 0.25
        }
    }

    Rectangle {
        anchors.fill: parent
        color: "transparent"
        border.width: 1
        border.color: Theme.tint(coverArt.palette, 0.12)
    }
}
