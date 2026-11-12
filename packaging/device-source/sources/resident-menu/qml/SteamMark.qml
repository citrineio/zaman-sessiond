import QtQuick
import "SteamPaths.js" as SteamPaths

Item {
    id: mark
    objectName: "steamMark"
    property color ink: "#DEC09A"
    implicitWidth: 32
    implicitHeight: 32
    readonly property int paintCount: drawing.paintCount
    onInkChanged: if (drawing) drawing.requestPaint()
    Canvas {
        id: drawing
        objectName: "steamVectorCanvas"
        property int paintCount: 0
        // Draw vector geometry at twice the displayed dimensions; Qt also
        // accounts for the screen DPR. Repaint when the focus geometry changes.
        anchors.centerIn: parent
        width: Math.max(1, Math.ceil(mark.width * 2))
        height: Math.max(1, Math.ceil(mark.height * 2))
        scale: .5
        transformOrigin: Item.Center
        smooth: true
        antialiasing: true
        contextType: "2d"
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        onAvailableChanged: if (available) requestPaint()
        onVisibleChanged: if (visible && available) requestPaint()
        onPainted: paintCount++
        onPaint: {
            var ctx = getContext("2d");
            if (!ctx) return;
            ctx.reset();
            ctx.clearRect(0, 0, width, height);
            ctx.scale(width / 24, height / 24);
            ctx.fillStyle = mark.ink;
            SteamPaths.draw(ctx);
        }
    }
}
