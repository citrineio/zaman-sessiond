import QtQuick

Rectangle {
    id:card
    property var record
    property real uiScale:1
    property bool entered:false
    property bool closing:false
    readonly property bool connectedSuccess: ["input", "network", "bluetooth"].indexOf(record.category) >= 0
        && (record.state === "connected" || (record.category === "bluetooth" && record.state === "enabled"))
    readonly property bool installSuccess: record.category === "install" && record.state === "installed"
    readonly property string accent: record.urgency === 2 || record.state === "error" || record.state === "disconnected" ? "#FF9A90" : connectedSuccess || installSuccess ? "#8AB9A1" : record.category === "bluetooth" && record.state === "disabled" ? "#A4B5C8" : "#EAC28D"
    height:Math.round((record.body.length ? 92 : 68) * uiScale)
    radius:12 * uiScale
    color:"#F7171D28"
    border.color:"#405D718C"
    opacity:entered && !closing ? 1 : 0
    Behavior on opacity { NumberAnimation { duration:120 } }
    transform:Translate {
        x:card.entered && !card.closing ? 0 : 24 * card.uiScale
        Behavior on x { NumberAnimation { duration:120; easing.type:Easing.OutCubic } }
    }
    Component.onCompleted: entered = true
    Rectangle { x:0; y:12 * card.uiScale; width:3 * card.uiScale; height:parent.height - 24 * card.uiScale; radius:2; color:card.accent }
    MenuIcon { x:16 * card.uiScale; y:20 * card.uiScale; width:28 * card.uiScale; height:width; name:card.record.category; ink:card.accent }
    Column {
        x:58 * card.uiScale; y:14 * card.uiScale
        width:card.width - 74 * card.uiScale
        spacing:4 * card.uiScale
        MenuText { width:parent.width; text:card.record.title; font.pixelSize:22 * card.uiScale; elide:Text.ElideRight; color:"#F0E5D6" }
        MenuText { visible:card.record.body.length > 0; width:parent.width; text:card.record.body.replace(/\s+/g," "); font.pixelSize:16 * card.uiScale; maximumLineCount:1; elide:Text.ElideRight; color:"#BBC6D3" }
    }
}
