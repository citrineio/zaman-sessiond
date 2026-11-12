import QtQuick 2.12

Item {
    id: diamond
    property real uiScale: 1
    property var activeFaces: []
    implicitWidth: Math.round(28 * uiScale)
    implicitHeight: Math.round(28 * uiScale)
    Repeater {
        model: [
            {face: "north", cx: 14, cy: 5},
            {face: "west", cx: 5, cy: 14},
            {face: "east", cx: 23, cy: 14},
            {face: "south", cx: 14, cy: 23}
        ]
        delegate: Rectangle {
            readonly property bool highlighted: diamond.activeFaces.indexOf(modelData.face) >= 0
            width: Math.round(7 * diamond.uiScale); height: width; radius: width / 2
            x: Math.round(modelData.cx * diamond.uiScale) - width / 2
            y: Math.round(modelData.cy * diamond.uiScale) - height / 2
            color: highlighted ? "#EAC28D" : "transparent"
            border.width: 1
            border.color: highlighted ? "#EAC28D" : "#8494A8"
        }
    }
}
