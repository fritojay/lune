import QtQuick.Controls

// A hidden menu item still takes its space, so this one also has no height.
MenuItem {
    height: visible ? implicitHeight : 0
}
