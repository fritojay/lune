import QtQuick.Controls

// ToggleToolButton for menus: checkable for state kept elsewhere
// (`toggledOn`); triggering asks for the change instead of owning it.
Action {
    id: toggle
    required property bool toggledOn
    signal toggleRequested(bool on)

    checkable: true
    checked: toggledOn
    onTriggered: {
        toggle.toggleRequested(checked)
        // Triggering toggles `checked`, which breaks its binding.
        checked = Qt.binding(() => toggle.toggledOn)
    }
}
