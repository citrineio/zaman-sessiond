import QtQuick

Item {
    id: page
    property real uiScale: 1
    property var snapshot: ({available:false, devices:[], error:""})
    property bool loading: false
    property string selectedId: "refresh"
    readonly property var devices: snapshot.devices || []
    readonly property var rows: devices.concat([{id:"refresh", name:"Refresh devices"}, {id:"back", name:"Back"}])
    signal refreshRequested()
    signal backRequested()
    function px(n) { return Math.round(n * uiScale); }
    function bytes(value) {
        var n = Number(value);
        if (!isFinite(n) || n < 0) return "Unknown";
        if (n >= 1099511627776) return (n / 1099511627776).toFixed(2) + " TiB";
        if (n >= 1073741824) return (n / 1073741824).toFixed(1) + " GiB";
        return (n / 1048576).toFixed(0) + " MiB";
    }
    function usage(volume) {
        if (volume.total === null) return bytes(volume.capacity) + " partition · " + (volume.mountpoint ? "Usage unavailable" : "Not mounted");
        return bytes(volume.total) + " total · " + bytes(volume.used) + " used · " + bytes(volume.available) + " available";
    }
    function select(index) {
        index = (index + rows.length) % rows.length;
        selectedId = rows[index].id;
        list.positionViewAtIndex(index, ListView.Contain);
    }
    function move(direction) { select(rows.findIndex(row => row.id === selectedId) + direction); }
    function activate() {
        if (selectedId === "back") backRequested();
        else refreshRequested();
    }
    onRowsChanged: if (!rows.some(row => row.id === selectedId)) selectedId = "refresh"
    Column {
        id: heading
        width: parent.width; spacing: page.px(6)
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: page.snapshot.error || (page.loading && !page.devices.length ? "Checking storage…" : page.devices.length ? "Connected storage · " + page.devices.length + " device" + (page.devices.length === 1 ? "" : "s") : "No usable storage devices detected.")
            font.pixelSize: page.px(20); color: page.snapshot.error ? "#EAC28D" : "#ECEEF1"
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: "Usage is shown for mounted volumes. Available space excludes filesystem reservations."
            font.pixelSize: page.px(14); color: "#BBC6D3"
        }
    }
    ListView {
        id: list
        anchors { left:parent.left; right:parent.right; top:heading.bottom; bottom:parent.bottom; topMargin:page.px(18) }
        model: page.rows; spacing: page.px(9); clip:true; boundsBehavior:Flickable.StopAtBounds
        delegate: Rectangle {
            id: card
            required property var modelData
            required property int index
            readonly property bool chosen: page.selectedId === modelData.id
            readonly property bool device: modelData.volumes !== undefined
            width: list.width
            height: device ? content.height + page.px(30) : page.px(58)
            radius:page.px(12); color:chosen ? "#EAC28D" : "#1D2634"
            border.width:1; border.color:chosen ? "#F4D2A4" : "#3A495C"
            Column {
                id: content
                x:page.px(16); y:page.px(15); width:parent.width - page.px(32); spacing:page.px(6)
                MenuText { width:parent.width; text:card.modelData.name; elide:Text.ElideRight; font.pixelSize:page.px(22); color:card.chosen ? "#211E1B" : "#ECEEF1" }
                MenuText {
                    visible:card.device; width:parent.width; wrapMode:Text.WordWrap
                    text:card.device ? card.modelData.type + " · " + card.modelData.connection + " · " + page.bytes(card.modelData.physicalSize) + " device" : ""
                    font.pixelSize:page.px(14); color:card.chosen ? "#473627" : "#BBC6D3"
                }
                Repeater {
                    model:card.device ? card.modelData.volumes : []
                    delegate: Column {
                        required property var modelData
                        width:content.width; spacing:page.px(4)
                        MenuText {
                            width:parent.width; wrapMode:Text.WordWrap
                            text:modelData.name + " · " + modelData.filesystem + (modelData.readOnly ? " · Read-only" : "")
                            font.pixelSize:page.px(14); color:card.chosen ? "#473627" : "#DEC09A"
                        }
                        MenuText {
                            width:parent.width; wrapMode:Text.WordWrap; text:page.usage(modelData)
                            font.pixelSize:page.px(14); color:card.chosen ? "#473627" : "#BBC6D3"
                        }
                        Rectangle {
                            visible:modelData.total !== null; width:parent.width; height:page.px(5); radius:height/2
                            color:card.chosen ? "#50614930" : "#35465C"
                            Rectangle {
                                width:parent.width * (modelData.total > 0 ? Math.max(0,Math.min(1,modelData.used/modelData.total)) : 0)
                                height:parent.height; radius:height/2; color:card.chosen ? "#614930" : "#88A8C9"
                            }
                        }
                    }
                }
            }
            MouseArea { anchors.fill:parent; onClicked:{page.select(index);page.activate();} }
        }
    }
}
