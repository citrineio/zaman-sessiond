import QtQuick

Rectangle {
    id: card
    property bool chosen: false
    property real uiScale: 1
    property color idleStart: "#00121820"
    property color idleEnd: "#00121820"
    property real emphasis: chosen ? 1 : 0

    // Follow the row's shared focus animation. The row owns its layout growth.
    x: Math.round(8 * uiScale * (1 - emphasis))
    y: Math.round(2 * uiScale * (1 - emphasis))
    width: Math.max(0, parent.width - 2 * x)
    height: Math.max(0, parent.height - 2 * y)
    radius: Math.round(13 * uiScale)
    border.width: chosen ? 1 : 0
    border.color: "#F4D2A4"
    gradient: Gradient {
        orientation: Gradient.Horizontal
        GradientStop { position: 0; color: card.chosen ? "#EAC28D" : card.idleStart }
        GradientStop { position: 1; color: card.chosen ? "#CB986C" : card.idleEnd }
    }
}
