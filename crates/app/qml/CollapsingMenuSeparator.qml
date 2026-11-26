import QtQuick.Controls

// A hidden separator still takes its space, so this one also has no height.
MenuSeparator {
    height: visible ? implicitHeight : 0
}
