import QtQuick

// --bench: steady scrolling, fast flicks, then jumps across the list.
Item {
    id: benchRun
    required property ListView list
    signal phaseStarted(string name)
    signal frame(real seconds)
    signal finished()

    function start() {
        run.start()
    }

    FrameAnimation {
        id: frames
        onTriggered: benchRun.frame(frameTime)
    }

    SequentialAnimation {
        id: run

        PauseAnimation { duration: 500 }
        ScriptAction { script: { benchRun.phaseStarted("scroll"); frames.start() } }
        NumberAnimation {
            target: benchRun.list
            property: "contentY"
            from: 0
            to: Math.min(30000, benchRun.list.contentHeight - benchRun.list.height)
            duration: 10000
        }
        ScriptAction { script: benchRun.phaseStarted("flick") }
        ScriptAction { script: benchRun.list.flick(0, -benchRun.list.maximumFlickVelocity) }
        PauseAnimation { duration: 1500 }
        ScriptAction { script: benchRun.list.flick(0, -benchRun.list.maximumFlickVelocity) }
        PauseAnimation { duration: 1500 }
        ScriptAction { script: benchRun.list.flick(0, benchRun.list.maximumFlickVelocity) }
        PauseAnimation { duration: 1500 }
        ScriptAction { script: benchRun.phaseStarted("jump") }
        ScriptAction { script: jumps.start() }
        PauseAnimation { duration: 4000 }
        ScriptAction {
            script: {
                frames.stop()
                benchRun.finished()
            }
        }
    }

    // Jumps like dragging the scroll bar: every visible delegate is rebound at once.
    Timer {
        id: jumps
        interval: 200
        repeat: true
        property int done: 0
        onTriggered: {
            benchRun.list.positionViewAtIndex(Math.floor(Math.random() * benchRun.list.count), ListView.Beginning)
            if (++done === 20)
                stop()
        }
    }
}
