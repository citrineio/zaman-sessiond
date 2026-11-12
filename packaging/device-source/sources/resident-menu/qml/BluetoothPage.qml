import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Bluetooth

Item {
    id: bluetooth
    property real uiScale: 1
    property bool initialized: false
    property BluetoothAdapter selectedAdapter: null
    readonly property BluetoothAdapter adapter: {
        var available = Bluetooth.adapters.values;
        if (selectedAdapter && available.indexOf(selectedAdapter) >= 0) return selectedAdapter;
        var preferred = Bluetooth.defaultAdapter;
        return preferred && available.indexOf(preferred) >= 0 ? preferred : (available.length ? available[0] : null);
    }
    property BluetoothAdapter scanAdapter: null
    property bool scanStopRequested: false
    property BluetoothDevice device: null
    property BluetoothDevice pendingDevice: null
    property string pendingPath: ""
    property string view: "list"
    property string confirmation: ""
    property int selected: 0
    property string selectedKey: "power"
    property string listKey: "power"
    property bool showNearby: false
    property string operation: ""
    property bool expected: false
    property string notice: ""
    property var prompt: null
    property bool pairSucceeded: false
    property bool pairCanceled: false
    property bool pairReported: false
    readonly property string pairHelper: Quickshell.env("ZAMAN_MENU_DEV_PAIR") || ""
    readonly property bool pairing: pairProcess.running || operation === "pair"
    readonly property bool busy: operation !== "" || pairProcess.running
    readonly property bool editingCode: prompt !== null && (prompt.kind === "pin" || prompt.kind === "passkey")
    readonly property var adapters: Bluetooth.adapters.values
    readonly property var devices: adapter ? adapter.devices.values : []
    readonly property var rows: makeRows()
    signal backRequested()
    signal navigationReset()

    // The page owns deferred layout work; destroyed delegates are never captured.
    Timer {
        id: focusSettle
        interval: 1
        repeat: false
        onTriggered: bluetooth.settleFocus()
    }
    function requestFocusSettle() { focusSettle.restart(); }
    function settleFocus() {
        if (!(visible) || selected < 0 || selected >= deviceList.count) return;
        deviceList.forceLayout();
        deviceList.positionViewAtIndex(selected, ListView.Contain);
    }

    function px(value) { return Math.round(value * uiScale); }
    function clean(value) { return String(value || "").replace(/[\x00-\x1f\x7f]/g, "").slice(0, 100); }
    function deviceName(d) { return d ? clean(d.name || d.deviceName || d.address) : "Device unavailable"; }
    function present(d) { return d !== null && devices.indexOf(d) >= 0; }
    function row(key, title, detail, target) { return {key: key, title: title, detail: detail, target: target || null}; }
    function deviceStatus(d) {
        if (!d) return "Device unavailable";
        var status = d.blocked ? "Blocked" : d.state === BluetoothDeviceState.Connecting ? "Connecting…"
            : d.state === BluetoothDeviceState.Disconnecting ? "Disconnecting…" : d.connected ? "Connected" : d.paired || d.bonded ? "Paired" : "Not paired";
        return status + (d.batteryAvailable ? " · " + Math.round(d.battery * 100) + "% battery" : "");
    }
    function makeRows() {
        if (editingCode) return [];
        if (prompt) return prompt.kind === "display" ? [row("cancel-pair", "Cancel pairing", "Return to device")]
            : [row("cancel-pair", "Cancel pairing", "Do not accept"), row("accept-pair", "Confirm", "Accept this pairing request")];
        if (view === "confirm") return [row("cancel", "Cancel", "Keep the current settings"),
            row("confirm", confirmation === "power" ? "Turn Bluetooth off" : confirmation === "forget" ? "Forget device" : "Disconnect device", "Apply this change")];
        if (pairing) return [row("cancel-pair", pairCanceled ? "Canceling…" : "Cancel pairing", "Wait for pairing to stop")];
        if (view === "detail" && device) {
            var result = [];
            if (device.blocked) result.push(row("unblock", "Unblock device", "Allow this device to connect"));
            else if (!device.paired && !device.bonded) result.push(row("pair", "Pair and connect", "Put the device in pairing mode first"));
            else result.push(row(device.connected ? "disconnect" : "connect", device.connected ? "Disconnect" : "Connect", "Use this saved device"));
            if (device.paired || device.bonded) {
                result.push(row("trust", "Automatic reconnection", device.trusted ? "Allowed · Select to disable" : "Not allowed · Select to allow"));
                result.push(row("forget", "Forget device", "Remove the saved pairing"));
            }
            result.push(row("back", "Back", "Return to Bluetooth devices"));
            return result;
        }
        var result = [];
        if (adapters.length > 1) result.push(row("adapter", "Bluetooth adapter", adapter ? clean(adapter.name) + " · Select to change" : "Select adapter"));
        if (adapter) {
            result.push(row("power", "Bluetooth", BluetoothAdapterState.toString(adapter.state)));
            if (adapter.enabled) {
                result.push(row("scan", scanAdapter ? (scanStopRequested ? "Waiting for search to stop…" : "Stop searching") : "Find devices", scanAdapter ? "Searching for up to 30 seconds" : "Put your controller or accessory in pairing mode"));
                var sorted = devices.slice().sort((a, b) => a.dbusPath.localeCompare(b.dbusPath));
                for (var i = 0; i < sorted.length; ++i) {
                    var d = sorted[i];
                    if (showNearby || d.paired || d.bonded || d.connected) result.push(row(d.dbusPath, deviceName(d), "", d));
                }
            }
        }
        result.push(row("back", "Back", "Return to Settings"));
        return result;
    }
    function select(index) {
        if (!rows.length) return;
        selected = Math.max(0, Math.min(rows.length - 1, index));
        selectedKey = rows[selected].key;
        if (initialized) deviceList.positionViewAtIndex(selected, ListView.Contain);
    }
    function move(direction) {
        if (editingCode) codeKeys.navigate(0, direction);
        else if (rows.length) select((selected + direction + rows.length) % rows.length);
    }
    function horizontal(direction) {
        if (editingCode) codeKeys.navigate(direction, 0);
        else if (direction < 0) back();
        else activate();
    }
    function cycleKeyboard() { if (editingCode) codeKeys.cycleView(); }
    function changeView(next) {
        navigationReset();
        view = next;
        selectedKey = next === "list" ? listKey : "";
        var index = rows.findIndex(r => r.key === selectedKey);
        select(index >= 0 ? index : 0);
    }
    function stopScan() {
        scanDeadline.stop();
        var held = scanAdapter;
        if (!held || adapters.indexOf(held) < 0 || !held.enabled) { scanAdapter = null; scanStopRequested = false; return; }
        // StartDiscovery may still be in flight. Keep the target until its
        // discovering signal arrives so a quick Back cannot strand that scan.
        scanStopRequested = true;
        if (held.discovering) { held.discovering = false; scanAdapter = null; scanStopRequested = false; }
    }
    function scan() {
        if (scanAdapter) { stopScan(); return; }
        if (!adapter || !adapter.enabled) return;
        showNearby = true;
        if (adapter.discovering) { notice = "A search is already running. Devices will appear below."; return; }
        scanStopRequested = false;
        scanAdapter = adapter;
        adapter.discovering = true;
        scanDeadline.restart();
        notice = "Searching… Nearby devices may take a few seconds to appear.";
    }
    function finish(message) {
        deadline.stop();
        operation = "";
        pendingDevice = null;
        pendingPath = "";
        notice = message;
    }
    function begin(kind, d) {
        if (busy || !adapter) return false;
        pendingDevice = d || null;
        pendingPath = d ? d.dbusPath : "";
        operation = kind;
        notice = "";
        deadline.restart();
        return true;
    }
    function settle() {
        if (!operation || operation === "pair") return;
        if (!adapter || adapters.indexOf(adapter) < 0) { finish("The Bluetooth adapter is unavailable."); return; }
        if (operation === "power") {
            var actual = expected ? BluetoothAdapterState.Enabled : BluetoothAdapterState.Disabled;
            if (adapter.state === actual) finish(expected ? "Bluetooth is on." : "Bluetooth is off.");
            return;
        }
        if (operation === "forget" && !devices.some(d => d.dbusPath === pendingPath)) {
            finish("Saved pairing removed."); return;
        }
        if (!present(pendingDevice)) { finish("The selected device is unavailable."); return; }
        if (operation === "connect" && pendingDevice.state === BluetoothDeviceState.Connected) finish("Connected.");
        else if (operation === "disconnect" && pendingDevice.state === BluetoothDeviceState.Disconnected) finish("Disconnected.");
        else if (operation === "trust" && pendingDevice.trusted === expected) finish("Reconnection preference updated.");
        else if (operation === "unblock" && !pendingDevice.blocked) finish("Device unblocked.");
        else if (operation === "forget" && !pendingDevice.paired && !pendingDevice.bonded) finish("Saved pairing removed.");
    }
    function startPair() {
        if (!present(device) || busy || device.pairing || !adapter.enabled) return;
        if (!pairHelper) { notice = "Pairing is unavailable in this preview. Build the supplied pairing helper first."; return; }
        stopScan();
        pendingDevice = device;
        pendingPath = device.dbusPath;
        operation = "pair";
        notice = "";
        prompt = null;
        codeKeys.reset();
        pairSucceeded = false; pairCanceled = false; pairReported = false;
        pairProcess.command = [pairHelper, device.dbusPath, device.adapter.dbusPath, device.address];
        navigationReset();
        pairProcess.running = true;
    }
    function cancelPair() {
        if (!pairing || pairCanceled) return;
        pairCanceled = true;
        prompt = null;
        codeKeys.reset();
        if (pairProcess.running) pairProcess.write(JSON.stringify({action: "cancel"}) + "\n");
    }
    function receive(line) {
        var message;
        try { message = JSON.parse(line); } catch (error) { return; }
        if (message.type === "prompt" && !pairCanceled) {
            navigationReset();
            if (!prompt || prompt.token !== message.token) codeKeys.reset();
            prompt = message;
            select(0);
        } else if (message.type === "dismiss") { prompt = null; codeKeys.reset(); }
        else if (message.type === "paired") { pairSucceeded = true; pairReported = true; }
        else if (message.type === "error") { notice = clean(message.message); pairReported = true; }
    }
    function reply(value) {
        if (!prompt || !pairProcess.running || pairCanceled) return;
        if (editingCode) {
            var valid = prompt.kind === "passkey" ? /^[0-9]{6}$/.test(value) : /^[\x20-\x7e]{1,16}$/.test(value);
            if (!valid) { notice = prompt.kind === "passkey" ? "Enter exactly six digits." : "Enter a PIN of 1–16 characters."; return; }
        }
        pairProcess.write(JSON.stringify({action: "reply", token: prompt.token, value: value}) + "\n");
        prompt = null;
        codeKeys.reset();
        notice = "Waiting for the device…";
    }
    function back() {
        if (pairing) { cancelPair(); return; }
        if (view === "confirm") changeView(device ? "detail" : "list");
        else if (view === "detail") { changeView("list"); device = null; }
        else { stopScan(); backRequested(); }
    }
    function applyConfirmation() {
        if (busy || !adapter) return;
        var action = confirmation;
        changeView(device ? "detail" : "list");
        if (action === "power") {
            stopScan(); expected = false;
            if (begin("power", null)) { adapter.enabled = false; settle(); }
        } else if (present(device)) {
            var d = device;
            if (begin(action, d)) {
                if (action === "forget") d.forget();
                else d.disconnect();
                settle();
            }
        }
    }
    function activate() {
        if (editingCode) { codeKeys.activate(); return; }
        var item = rows[selected];
        if (!item) return;
        if (item.key === "cancel-pair") { cancelPair(); return; }
        if (item.key === "accept-pair") { reply("accept"); return; }
        if (item.key === "cancel" || item.key === "back") { back(); return; }
        if (busy) return;
        if (item.key === "confirm") { applyConfirmation(); return; }
        if (item.target) { device = item.target; listKey = item.key; notice = ""; changeView("detail"); return; }
        if (item.key === "adapter") {
            stopScan(); showNearby = false;
            selectedAdapter = adapters[(adapters.indexOf(adapter) + 1) % adapters.length];
            return;
        }
        if (!adapter) return;
        if (item.key === "power") {
            if (adapter.state === BluetoothAdapterState.Blocked) { notice = "Bluetooth is blocked by hardware."; return; }
            if (adapter.enabled) { confirmation = "power"; changeView("confirm"); }
            else { expected = true; if (begin("power", null)) { adapter.enabled = true; settle(); } }
        } else if (item.key === "scan") scan();
        else if (present(device) && adapter.enabled) {
            if (device.state === BluetoothDeviceState.Connecting || device.state === BluetoothDeviceState.Disconnecting || device.pairing) {
                notice = "Wait for the current device operation to finish."; return;
            }
            if (item.key === "pair") startPair();
            else if (item.key === "disconnect" || item.key === "forget") { confirmation = item.key; changeView("confirm"); }
            else if (item.key === "connect") { if (begin("connect", device)) { device.connect(); settle(); } }
            else if (item.key === "trust") { expected = !device.trusted; if (begin("trust", device)) { device.trusted = expected; settle(); } }
            else if (item.key === "unblock") { if (begin("unblock", device)) { device.blocked = false; settle(); } }
        }
    }
    function physicalKey(event) {
        if (!editingCode) return false;
        if (event.key === Qt.Key_Escape || event.key === Qt.Key_Back) { if (!event.isAutoRepeat) back(); }
        else if (event.key === Qt.Key_Up || event.key === Qt.Key_Down || event.key === Qt.Key_Left || event.key === Qt.Key_Right) return false;
        else if (event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete) codeKeys.erase();
        else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) { if (!event.isAutoRepeat) reply(codeKeys.secret); }
        else if (!(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) codeKeys.append(event.text);
        return true;
    }
    function shutdown() { stopScan(); cancelPair(); codeKeys.reset(); }
    onRowsChanged: if (initialized) {
        var index = rows.findIndex(r => r.key === selectedKey);
        select(index >= 0 ? index : 0);
    }
    onDevicesChanged: if (initialized) {
        settle();
        if (device && !present(device)) { cancelPair(); device = null; changeView("list"); notice = "The selected device is unavailable."; }
    }
    onDeviceChanged: if (initialized && !device && view !== "list") { cancelPair(); changeView("list"); }
    onAdapterChanged: if (initialized) {
        shutdown(); finish(adapter ? "" : "No Bluetooth adapter available."); device = null;
        changeView("list");
    }
    onPendingDeviceChanged: if (initialized) {
        if (operation === "pair" && !pendingDevice) cancelPair();
        else if (operation) Qt.callLater(() => bluetooth.settle());
    }
    Component.onCompleted: { initialized = true; select(0); }
    Component.onDestruction: shutdown()
    onVisibleChanged: if (initialized && !visible) shutdown()

    Process {
        id: pairProcess
        stdinEnabled: true
        stdout: SplitParser { onRead: data => bluetooth.receive(data) }
        onRunningChanged: if (!running && bluetooth.operation === "pair") Qt.callLater(() => {
            if (!pairProcess.running && bluetooth.operation === "pair") bluetooth.finish("The pairing helper could not finish. Check its build and try again.");
        })
        onExited: (exitCode, exitStatus) => {
            var d = bluetooth.pendingDevice;
            var success = bluetooth.pairSucceeded && !bluetooth.pairCanceled && exitCode === 0;
            var message = bluetooth.pairCanceled ? "Pairing canceled." : bluetooth.notice;
            bluetooth.prompt = null;
            codeKeys.reset();
            if (!bluetooth.pairReported && !bluetooth.pairCanceled) message = "The pairing helper stopped before confirming pairing.";
            bluetooth.finish(message);
            if (success && bluetooth.present(d) && bluetooth.adapter && bluetooth.adapter.enabled && bluetooth.visible) {
                bluetooth.device = d;
                bluetooth.changeView("detail");
                if (d.connected) bluetooth.notice = "Paired and connected.";
                else if (bluetooth.begin("connect", d)) { d.connect(); bluetooth.settle(); }
            } else if (success) bluetooth.notice = "Paired. Open the device again to connect.";
        }
    }
    Timer {
        interval: 150000
        running: bluetooth.operation === "pair"
        onTriggered: {
            bluetooth.cancelPair();
            if (!pairProcess.running) bluetooth.finish("Pairing timed out.");
        }
    }
    Timer { interval: 1000; running: pairProcess.running; repeat: true; onTriggered: pairProcess.write(JSON.stringify({action: "heartbeat"}) + "\n") }
    Timer { id: scanDeadline; interval: 30000; onTriggered: { bluetooth.stopScan(); bluetooth.notice = "Search finished. Select Find devices to search again."; } }
    Timer { id: deadline; interval: 20000; onTriggered: { bluetooth.settle(); if (bluetooth.operation) bluetooth.finish("The change was not confirmed. Check the device and try again."); } }
    Connections {
        target: bluetooth.pendingDevice
        function onStateChanged() { bluetooth.settle(); }
        function onPairedChanged() { bluetooth.settle(); }
        function onBondedChanged() { bluetooth.settle(); }
        function onTrustedChanged() { bluetooth.settle(); }
        function onBlockedChanged() { bluetooth.settle(); }
    }
    Connections {
        target: bluetooth.scanAdapter
        function onDiscoveringChanged() {
            if (bluetooth.scanStopRequested) bluetooth.stopScan();
        }
        function onEnabledChanged() {
            if (bluetooth.scanAdapter && !bluetooth.scanAdapter.enabled) bluetooth.stopScan();
        }
    }
    Connections {
        target: bluetooth.adapter
        function onStateChanged() {
            bluetooth.settle();
            if (bluetooth.adapter && !bluetooth.adapter.enabled) bluetooth.cancelPair();
        }
    }
    Column {
        id: status
        width: parent.width; spacing: bluetooth.px(8)
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 2; elide: Text.ElideRight
            text: bluetooth.device ? bluetooth.deviceName(bluetooth.device) : bluetooth.adapter ? bluetooth.clean(bluetooth.adapter.name) : "No Bluetooth adapter available"
            font.pixelSize: bluetooth.px(21)
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: bluetooth.device ? bluetooth.deviceStatus(bluetooth.device) + " · " + bluetooth.device.address
                : bluetooth.adapter ? BluetoothAdapterState.toString(bluetooth.adapter.state) : "Return to Settings and try again."
            font.pixelSize: bluetooth.px(14); color: bluetooth.device && bluetooth.device.connected ? "#98D8AE" : "#BBC6D3"
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 4; elide: Text.ElideRight
            text: bluetooth.prompt ? (bluetooth.prompt.kind === "confirm" ? "Check that both devices show " + bluetooth.prompt.code + ". Confirm only if they match."
                : bluetooth.prompt.kind === "authorize" ? "Allow pairing with this device?"
                : bluetooth.prompt.kind === "display" ? "Type " + bluetooth.prompt.code + " on the other device, then press Enter."
                    + (bluetooth.prompt.entered !== null && bluetooth.prompt.entered !== undefined ? " " + bluetooth.prompt.entered + " digits entered." : "")
                : bluetooth.prompt.kind === "passkey" ? "Enter the six-digit code shown by the other device." : "Enter this device’s pairing PIN.")
                : bluetooth.view === "confirm" ? (bluetooth.confirmation === "power" ? "This disconnects all Bluetooth devices on this adapter."
                    : bluetooth.confirmation === "forget" ? "This removes the saved pairing and disconnects the device." : "This disconnects the selected device.")
                    + " If it is your controller or keyboard, use another input device to reconnect it."
                : bluetooth.notice || (bluetooth.pairing ? "Pairing… Follow any instructions on the other device."
                    : bluetooth.busy ? "Applying change…" : bluetooth.showNearby ? "Known and discovered devices. A device may no longer be nearby." : "Select Find devices to pair a new controller or accessory.")
            font.pixelSize: bluetooth.px(15); color: "#D9BC96"
        }
        MenuText {
            visible: bluetooth.editingCode && bluetooth.notice.length > 0
            width: parent.width; wrapMode: Text.WordWrap
            text: bluetooth.notice; font.pixelSize: bluetooth.px(14); color: "#D9BC96"
        }
    }
    PasswordKeyboard {
        id: codeKeys
        visible: bluetooth.editingCode
        uiScale: bluetooth.uiScale
        title: bluetooth.prompt && bluetooth.prompt.kind === "passkey" ? "Pairing code" : "Pairing PIN"
        submitLabel: "Confirm"
        maxLength: bluetooth.prompt && bluetooth.prompt.kind === "passkey" ? 6 : 16
        digitsOnly: bluetooth.prompt !== null && bluetooth.prompt.kind === "passkey"
        masked: false
        anchors { left: parent.left; right: parent.right; top: status.bottom; bottom: parent.bottom; topMargin: bluetooth.px(20) }
        onSubmitted: value => bluetooth.reply(value)
    }
    ListView {
        id: deviceList
        property point lastPointerPosition: Qt.point(-100000, -100000)
        visible: !bluetooth.editingCode
        anchors { left: parent.left; right: parent.right; top: status.bottom; bottom: parent.bottom; topMargin: bluetooth.px(20) }
        model: bluetooth.rows; currentIndex: bluetooth.selected; spacing: bluetooth.px(6)
        clip: true; boundsBehavior: Flickable.StopAtBounds
        delegate: MenuFocusRow {
            id: deviceRow
            required property var modelData
            required property int index
            width: deviceList.width; baseHeight: 78; uiScale: bluetooth.uiScale; chosen: bluetooth.selected === index
            onFocusSettled: if (deviceRow.chosen) bluetooth.requestFocusSettle()
            MenuFocusBackground { chosen: deviceRow.chosen; uiScale: bluetooth.uiScale; emphasis: deviceRow.focusAmount }
            MenuIcon {
                x: bluetooth.px(16); y: Math.round((parent.height - height) / 2)
                name: deviceRow.modelData.key === "back" || deviceRow.modelData.key === "cancel" ? "back" : "bluetooth"
                width: bluetooth.px(28 + 2 * deviceRow.focusAmount); height: width
                ink: deviceRow.chosen ? "#30271E" : "#DEC09A"
            }
            Column {
                x: bluetooth.px(58); y: Math.round((parent.height - height) / 2)
                width: parent.width - bluetooth.px(96); spacing: bluetooth.px(5)
                MenuText {
                    width: parent.width; elide: Text.ElideRight; text: deviceRow.modelData.title
                    font.pixelSize: bluetooth.px(21 + 2 * deviceRow.focusAmount); color: deviceRow.chosen ? "#211E1B" : "#ECEEF1"
                }
                MenuText {
                    width: parent.width; elide: Text.ElideRight
                    text: deviceRow.modelData.target ? bluetooth.deviceStatus(deviceRow.modelData.target) : deviceRow.modelData.detail
                    font.pixelSize: bluetooth.px(14); color: deviceRow.chosen ? "#473627" : "#BBC6D3"
                }
            }
            Rectangle {
                visible: deviceRow.modelData.target !== null && deviceRow.modelData.target.connected
                anchors { right: parent.right; rightMargin: bluetooth.px(17); verticalCenter: parent.verticalCenter }
                width: bluetooth.px(10); height: width; radius: width / 2; color: "#58BA82"; border.color: "#203C2B"; border.width: 1
            }
            MouseArea {
                id: pointer
                anchors.fill: parent; hoverEnabled: true
                onEntered: if (deviceRow.pointerMoved(pointer)) bluetooth.select(index)
                onPositionChanged: if (deviceRow.pointerMoved(pointer)) bluetooth.select(index)
                onClicked: { bluetooth.select(index); bluetooth.activate(); }
                onWheel: wheel => { bluetooth.move(wheel.angleDelta.y > 0 ? -1 : 1); wheel.accepted = true; }
            }
        }
    }
}
