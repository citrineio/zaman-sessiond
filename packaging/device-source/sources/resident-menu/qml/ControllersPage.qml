import QtQuick

Item {
    id: controllers
    property real uiScale: 1
    property var devices: []
    property int selected: 0
    property string selectedId: ""
    readonly property var rows: devices.concat([{ id: "back", name: "Back" }])
    readonly property var current: selected < devices.length ? devices[selected] : null
    signal backRequested()

    // The page owns deferred layout work; destroyed delegates are never captured.
    Timer {
        id: focusSettle
        interval: 1
        repeat: false
        onTriggered: controllers.settleFocus()
    }
    function requestFocusSettle() { focusSettle.restart(); }
    function settleFocus() {
        if (!(visible) || selected < 0 || selected >= controllerList.count) return;
        controllerList.forceLayout();
        controllerList.positionViewAtIndex(selected, ListView.Contain);
    }

    function px(value) { return Math.round(value * uiScale); }
    function clean(value) { return String(value || "").replace(/[\x00-\x1f\x7f]/g, "").slice(0, 100); }
    function targetLabel(path) {
        var token = String(path).split("/").pop().toLowerCase();
        var names = { xbox: "Xbox", xbox360: "Xbox 360", "xbox-360": "Xbox 360",
            "xbox-elite": "Xbox Elite", xboxone: "Xbox One", dualsense: "DualSense",
            dualshock4: "DualShock 4", gamepad: "Gamepad", keyboard: "Keyboard", mouse: "Mouse" };
        var keys = Object.keys(names).sort((a, b) => b.length - a.length);
        for (var i = 0; i < keys.length; ++i) {
            var key = keys[i];
            if (token === key || new RegExp("^" + key + "[_-]?[0-9]+$").test(token)) return names[key];
        }
        return "Other input device";
    }
    function targetSummary(device) {
        if (!device || !device.targets || device.targets.length === 0) return "No game input output";
        var labels = device.targets.filter(path => !/dbus[0-9]*$/.test(path)).map(targetLabel);
        return labels.filter((label, index) => labels.indexOf(label) === index).join(" · ") || "Menu input only";
    }
    function select(index) {
        selected = Math.max(0, Math.min(rows.length - 1, index));
        selectedId = rows[selected].id;
        if (controllerList) controllerList.positionViewAtIndex(selected, ListView.Contain);
    }
    function move(direction) { select((selected + direction + rows.length) % rows.length); }
    function activate() { if (rows[selected].id === "back") backRequested(); }
    onDevicesChanged: {
        var index = rows.findIndex(row => row.id === selectedId);
        select(index >= 0 ? index : Math.min(selected, rows.length - 1));
    }
    Component.onCompleted: select(0)

    Column {
        id: status
        width: parent.width
        spacing: controllers.px(8)
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: controllers.devices.length === 0 ? "No managed controllers connected."
                : controllers.devices.length + (controllers.devices.length === 1 ? " controller available" : " controllers available")
            font.pixelSize: controllers.px(21); color: "#ECEEF1"
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: controllers.devices.length === 0
                ? "Connect a controller by USB, its receiver, or an existing Bluetooth pairing."
                : "Connect or disconnect a controller to update this list."
            font.pixelSize: controllers.px(15); color: "#BBC6D3"
        }
    }
    Column {
        id: detail
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        spacing: controllers.px(8)
        Rectangle { width: parent.width; height: 1; color: "#265C6E85" }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: controllers.current
                ? "Game input: " + controllers.targetSummary(controllers.current)
                : "Pair new devices in Settings → Bluetooth."
            font.pixelSize: controllers.px(15); color: "#C7CFD8"
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: controllers.current
                ? "Profile: " + controllers.clean(controllers.current.profile || "Not reported")
                : "Guide continues to open the fallback menu."
            font.pixelSize: controllers.px(14); color: "#BBC6D3"
        }
    }
    ListView {
        id: controllerList
        property point lastPointerPosition: Qt.point(-100000, -100000)
        anchors { left: parent.left; right: parent.right; top: status.bottom; bottom: detail.top
            topMargin: controllers.px(20); bottomMargin: controllers.px(22) }
        model: controllers.rows
        currentIndex: controllers.selected
        spacing: controllers.px(8)
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        delegate: MenuFocusRow {
            id: row
            required property var modelData
            required property int index
            chosen: controllers.selected === index
            uiScale: controllers.uiScale
            readonly property bool backRow: modelData.id === "back"
            width: controllerList.width
            baseHeight: backRow ? 66 : 100
            onFocusSettled: if (row.chosen) controllers.requestFocusSettle()
            MenuFocusBackground {
                chosen: row.chosen
                uiScale: controllers.uiScale
                emphasis: row.focusAmount
                idleStart: "#162F425C"
                idleEnd: "#10121820"
            }
            MenuIcon {
                x: controllers.px(16); y: controllers.px(19) + row.contentOffset
                name: row.backRow ? "back" : "controllers"
                width: controllers.px(28 + 2 * row.focusAmount); height: width
                ink: row.chosen ? "#30271E" : "#DEC09A"
            }
            Column {
                x: controllers.px(58); y: controllers.px(15) + row.contentOffset
                width: parent.width - controllers.px(78)
                spacing: controllers.px(6)
                MenuText {
                    width: parent.width; elide: Text.ElideRight
                    text: controllers.clean(row.modelData.name || "Controller")
                    font.pixelSize: controllers.px(21 + 2 * row.focusAmount)
                    color: row.chosen ? "#211E1B" : "#ECEEF1"
                }
                MenuText {
                    visible: !row.backRow
                    width: parent.width; elide: Text.ElideRight
                    text: row.modelData.menuInput ? "Menu input available" : "Menu input unavailable"
                    font.pixelSize: controllers.px(14)
                    color: row.chosen ? "#473627" : (row.modelData.menuInput ? "#91BBA7" : "#BBC6D3")
                }
                MenuText {
                    visible: !row.backRow
                    width: parent.width; elide: Text.ElideRight
                    text: controllers.targetSummary(row.modelData)
                    font.pixelSize: controllers.px(13)
                    color: row.chosen ? "#473627" : "#BBC6D3"
                }
            }
            MouseArea {
                id: controllerPointer
                anchors.fill: parent; hoverEnabled: true
                onEntered: if (row.pointerMoved(controllerPointer)) controllers.select(index)
                onPositionChanged: if (row.pointerMoved(controllerPointer)) controllers.select(index)
                onClicked: { controllers.select(index); controllers.activate(); }
                onWheel: wheel => { controllers.move(wheel.angleDelta.y > 0 ? -1 : 1); wheel.accepted = true; }
            }
        }
    }
}
