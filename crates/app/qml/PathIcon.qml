pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Shapes

Shape {
    id: pathIcon
    required property string name
    property color color: palette.text

    readonly property var paths: ({
        "now-playing": { fill: "M5 9h3.5L13 5v14l-4.5-4H5z", stroke: "M16.5 9a4 4 0 0 1 0 6" },
        "note": { fill: "M4 17.5a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0-5 0zM15 15a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0-5 0z",
                  stroke: "M9 17.5V6l11-2.5v11.5" },
        "play": { fill: "M8 5.5v13a1 1 0 0 0 1.5.86l10.5-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5z", stroke: "" },
        "pause": { fill: "M7 5h2a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1H7a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1zM15 5h2a1 1 0 0 1 1 1v12a1 1 0 0 1-1 1h-2a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1z",
                   stroke: "" },
    })
    readonly property var path: paths[name] ?? { fill: "", stroke: "" }

    implicitWidth: 16
    implicitHeight: 16
    preferredRendererType: Shape.CurveRenderer
    transform: Scale {
        xScale: pathIcon.width / 24
        yScale: pathIcon.height / 24
    }

    ShapePath {
        fillColor: pathIcon.color
        strokeColor: "transparent"
        PathSvg {
            path: pathIcon.path.fill
        }
    }
    ShapePath {
        fillColor: "transparent"
        strokeColor: pathIcon.path.stroke ? pathIcon.color : "transparent"
        strokeWidth: 2
        capStyle: ShapePath.RoundCap
        joinStyle: ShapePath.RoundJoin
        PathSvg {
            path: pathIcon.path.stroke
        }
    }
}
