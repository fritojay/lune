import QtQuick
import QtQuick.Controls

// Space plays and pauses wherever focus is (main.qml), so Return and Enter
// press a focused button instead.
ToolButton {
    Keys.onReturnPressed: click()
    Keys.onEnterPressed: click()
}
