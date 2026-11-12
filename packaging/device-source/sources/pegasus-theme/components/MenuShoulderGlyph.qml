import QtQuick 2.12

// Same positional language as the face-button diamond: highlight the bumper.
Item {
    id: glyph
    property real uiScale: 1
    property string side: "left"
    property color ink: "#EAC28D"
    property color outline: "#8494A8"
    implicitWidth: Math.round(40 * uiScale)
    implicitHeight: Math.round(30 * uiScale)
    Accessible.name: side === "left" ? "Left shoulder" : "Right shoulder"
    onSideChanged: if (drawing) drawing.requestPaint()
    onInkChanged: if (drawing) drawing.requestPaint()
    onOutlineChanged: if (drawing) drawing.requestPaint()
    Canvas {
        id: drawing
        anchors.fill: parent
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        onPaint: {
            var c = getContext("2d");
            c.reset(); c.scale(width / 44, height / 32);
            c.lineWidth = 1.5; c.lineJoin = "round"; c.lineCap = "round";
            c.strokeStyle = glyph.outline;
            c.beginPath(); c.moveTo(12,10); c.lineTo(32,10);
            c.bezierCurveTo(37,10,38,16,40,25);
            c.bezierCurveTo(41,30,35,30,31,24);
            c.lineTo(13,24); c.bezierCurveTo(9,30,3,30,4,25);
            c.bezierCurveTo(6,16,7,10,12,10); c.closePath(); c.stroke();
            for (var i = 0; i < 2; ++i) {
                var active = (i === 0) === (glyph.side === "left");
                c.strokeStyle = active ? glyph.ink : glyph.outline;
                c.fillStyle = active ? glyph.ink : "transparent";
                c.beginPath();
                c.rect(i === 0 ? 9 : 26, 3, 9, 5);
                if (active) c.fill(); c.stroke();
            }
            c.strokeStyle = glyph.outline;
            c.beginPath(); c.moveTo(11,17); c.lineTo(17,17); c.moveTo(14,14); c.lineTo(14,20); c.stroke();
            c.fillStyle = glyph.outline;
            var points = [[30,15],[33,18],[27,18],[30,21]];
            for (var p = 0; p < points.length; ++p) {
                var point = points[p];
                c.beginPath(); c.arc(point[0],point[1],1,0,Math.PI*2); c.fill();
            }
        }
    }
}
