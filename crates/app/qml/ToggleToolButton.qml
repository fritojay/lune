import QtQuick

// A toggle for state kept elsewhere (`toggledOn`). It stays checkable, so
// assistive tech announces it as a toggle; a click asks for the change
// instead of owning it.
KeyToolButton {
    id: toggle
    required property bool toggledOn
    signal toggleRequested(bool on)

    checkable: true
    checked: toggledOn
    onClicked: {
        toggle.toggleRequested(checked)
        // A click toggles `checked`, which breaks its binding.
        checked = Qt.binding(() => toggle.toggledOn)
    }
}
