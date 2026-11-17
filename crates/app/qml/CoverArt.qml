import QtQuick

Rectangle {
    id: coverArt
    required property url source
    property bool placeholder: true
    property bool decodeAtSize: true

    color: palette.mid

    Image {
        id: image
        anchors.fill: parent
        source: coverArt.source
        asynchronous: true
        fillMode: Image.PreserveAspectFit
        sourceSize.width: coverArt.decodeAtSize ? width * Screen.devicePixelRatio : -1
        sourceSize.height: coverArt.decodeAtSize ? height * Screen.devicePixelRatio : -1
    }

    Text {
        anchors.centerIn: parent
        visible: coverArt.placeholder && image.status !== Image.Ready
        text: "♪"
        font.pointSize: 40
        color: coverArt.palette.base
    }
}
