import QtQuick

// Storefront components load only when used. Steam paints official vector paths;
// Dukkan displays the owner-supplied sign. Generic icons retain their canvas.
Item {
    id: icon
    property string name: "settings"
    property color ink: "#DDC2A0"
    readonly property bool branded: name === "store-dukkan" || name === "store-steam"
    implicitWidth: 32
    implicitHeight: 32
    onNameChanged: if (lines) lines.requestPaint()
    onInkChanged: if (lines) lines.requestPaint()
    Loader {
        anchors.fill: parent
        active: icon.branded
        sourceComponent: icon.name === "store-dukkan" ? dukkanComponent : steamComponent
    }
    Component { id: dukkanComponent; DukkanMark {} }
    Component { id: steamComponent; SteamMark { ink: icon.ink } }
    Canvas {
        id: lines
        anchors.fill: parent
        visible: !icon.branded
        contextType: "2d"
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        onAvailableChanged: if (available) requestPaint()
        onPaint: {
            var ctx = getContext("2d");
            if (!ctx) return;
            ctx.reset();
            ctx.clearRect(0, 0, width, height);
            if (icon.branded) return;
            ctx.save();
            ctx.scale(width / 24, height / 24);
            ctx.strokeStyle = icon.ink;
            ctx.fillStyle = icon.ink;
            ctx.lineWidth = 1.65;
            ctx.lineCap = "round";
            ctx.lineJoin = "round";

            function line(points, close) {
                ctx.beginPath();
                ctx.moveTo(points[0][0], points[0][1]);
                for (var i = 1; i < points.length; ++i) ctx.lineTo(points[i][0], points[i][1]);
                if (close) ctx.closePath();
                ctx.stroke();
            }
            function circle(x, y, r, filled) {
                ctx.beginPath();
                ctx.arc(x, y, r, 0, Math.PI * 2);
                if (filled) ctx.fill();
                else ctx.stroke();
            }
            function box(x, y, w, h, r) {
                ctx.beginPath();
                ctx.moveTo(x + r, y);
                ctx.lineTo(x + w - r, y);
                ctx.quadraticCurveTo(x + w, y, x + w, y + r);
                ctx.lineTo(x + w, y + h - r);
                ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
                ctx.lineTo(x + r, y + h);
                ctx.quadraticCurveTo(x, y + h, x, y + h - r);
                ctx.lineTo(x, y + r);
                ctx.quadraticCurveTo(x, y, x + r, y);
                ctx.closePath();
                ctx.stroke();
            }

            switch (icon.name) {
            default:
            case "system":
            case "notifications":
                ctx.beginPath(); ctx.arc(12,10,6,Math.PI,0); ctx.stroke();
                line([[6,10],[6,16],[3,18],[21,18],[18,16],[18,10]], false);
                ctx.beginPath(); ctx.arc(12,19,2,0,Math.PI); ctx.stroke();
                break;
            case "scrape":
            case "artwork":
                box(3,4,18,16,2); circle(8,9,1.5,false);
                line([[4,18],[10,12],[14,16],[17,12],[21,16]],false);
                break;
            case "battery":
                box(3,6,17,12,2); line([[22,10],[22,14]],false);
                line([[6,9],[6,15]],false); break;
            case "install":
            case "transactions":
            case "transfer":
                line([[12,3],[12,15],[7,10]],false); line([[12,15],[17,10]],false);
                line([[3,15],[3,21],[21,21],[21,15]],false); break;
            case "success":
                circle(12,12,9,false); line([[7,12],[11,16],[17,8]],false); break;
            case "warning":
            case "error":
                line([[12,3],[22,21],[2,21],[12,3]],true);
                line([[12,8],[12,14]],false); circle(12,18,0.8,true); break;
            case "resume":
                circle(12, 12, 9, false);
                ctx.beginPath();
                ctx.moveTo(10, 7.8); ctx.lineTo(16, 12); ctx.lineTo(10, 16.2);
                ctx.closePath(); ctx.fill();
                break;
            case "storefronts":
                box(4, 10, 16, 11, 1);
                line([[3,10],[5,3],[19,3],[21,10]], true);
                box(9, 14, 6, 7, 0);
                break;
            case "store-home":
                line([[3,11],[12,3],[21,11]], false);
                line([[5,10],[5,21],[19,21],[19,10]], false);
                break;
            case "store-close":
                line([[6,6],[18,18]], false); line([[18,6],[6,18]], false);
                break;
            case "library":
                box(3, 3, 7, 7, 1); box(14, 3, 7, 7, 1);
                box(3, 14, 7, 7, 1); box(14, 14, 7, 7, 1);
                break;
            case "audio":
                line([[3,9],[7,9],[11,5],[11,19],[7,15],[3,15]], true);
                ctx.beginPath(); ctx.arc(13, 12, 4, -0.9, 0.9); ctx.stroke();
                ctx.beginPath(); ctx.arc(13, 12, 7.5, -0.9, 0.9); ctx.stroke();
                break;
            case "bluetooth":
                line([[12,3],[18,8],[6,16]], false);
                line([[6,8],[18,16],[12,21],[12,3]], false);
                break;
            case "network":
                for (var radius = 5; radius <= 15; radius += 5) {
                    ctx.beginPath(); ctx.arc(12, 21, radius, -Math.PI * 0.75, -Math.PI * 0.25); ctx.stroke();
                }
                circle(12, 20, 1.1, true);
                break;
            case "display":
                box(3, 4, 18, 13, 1.7);
                line([[12,17],[12,21]], false); line([[8,21],[16,21]], false);
                break;
            case "input":
            case "controllers":
                ctx.beginPath();
                ctx.moveTo(8,6); ctx.lineTo(16,6);
                ctx.bezierCurveTo(20,6,20.4,10,21.2,16);
                ctx.bezierCurveTo(21.9,20,18.5,20,16,16);
                ctx.lineTo(8,16);
                ctx.bezierCurveTo(5.5,20,2.1,20,2.8,16);
                ctx.bezierCurveTo(3.6,10,4,6,8,6); ctx.closePath(); ctx.stroke();
                line([[6,11],[10,11]], false); line([[8,9],[8,13]], false);
                circle(16,10,0.8,true); circle(18,12.5,0.8,true);
                break;
            case "settings":
                var points = [];
                for (var tooth = 0; tooth < 32; ++tooth) {
                    var a = (tooth - 0.5) * Math.PI / 16;
                    var r = tooth % 4 < 2 ? 9.4 : 7.6;
                    points.push([12 + Math.cos(a) * r, 12 + Math.sin(a) * r]);
                }
                line(points, true); circle(12,12,3.1,false);
                break;
            case "appearance":
                // Palette: a distinct visual for future theme customization.
                ctx.beginPath();
                ctx.moveTo(21,11);
                ctx.bezierCurveTo(21,5,16,2,11,3);
                ctx.bezierCurveTo(5,3,2,7,3,13);
                ctx.bezierCurveTo(4,19,9,22,14,21);
                ctx.bezierCurveTo(17,20,15,17,15,16);
                ctx.bezierCurveTo(15,14,21,16,21,11);
                ctx.closePath(); ctx.stroke();
                circle(7,10,1,true); circle(10,6.5,1,true);
                circle(15,7,1,true); circle(7.5,15,1,true);
                break;
            case "reboot":
                ctx.beginPath(); ctx.arc(12,12,8,-Math.PI*0.1,Math.PI*1.5); ctx.stroke();
                line([[8.5,1.5],[12,4],[8.5,7.5]], false);
                break;
            case "power":
            case "shutdown":
                ctx.beginPath(); ctx.arc(12,13,8,-Math.PI*0.25,Math.PI*1.25); ctx.stroke();
                line([[12,2],[12,11]], false);
                break;
            case "sleep":
                line([[16,3],[10,4],[6,8],[6,15],[10,20],[17,19],[21,15],[15,16],[11,12],[11,7],[16,3]], true);
                break;
            case "exit":
                line([[10,4],[20,4],[20,20],[10,20]], false);
                line([[4,12],[14,12]], false); line([[10,8],[14,12],[10,16]], false);
                break;
            case "chevron":
                line([[9,6],[15,12],[9,18]], false);
                break;
            case "back":
                line([[15,6],[9,12],[15,18]], false);
                break;
            case "navigation":
                line([[8,20],[8,4],[4,8]], false); line([[8,4],[12,8]], false);
                line([[16,4],[16,20],[12,16]], false); line([[16,20],[20,16]], false);
                break;
            }
            ctx.restore();
        }
    }
}
