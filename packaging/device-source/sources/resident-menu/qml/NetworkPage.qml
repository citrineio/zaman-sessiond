import QtQuick
import Quickshell.Networking

Item {
    id: network
    property real uiScale: 1
    property bool initialized: false
    property string view: "list"
    property int selected: 0
    property string selectedKey: "radio"
    property string listKey: "radio"
    property WifiNetwork targetNetwork: null
    property WifiNetwork pendingNetwork: null
    property NetworkDevice pendingDevice: null
    property string operation: ""
    property bool pendingPasswordProvided: false
    property bool expectedRadio: false
    property string notice: ""
    readonly property bool busy: operation !== ""
    readonly property bool editingPassword: view === "password"
    readonly property bool available: Networking.backend === NetworkBackendType.NetworkManager
    readonly property var devices: Networking.devices.values
    readonly property var wifiDevices: devices.filter(device => device.type === DeviceType.Wifi)
    readonly property var networks: {
        var result = [];
        for (var i = 0; i < wifiDevices.length; ++i) {
            var values = wifiDevices[i].networks.values;
            for (var j = 0; j < values.length; ++j) if (values[j].name.length) result.push(values[j]);
        }
        return result.sort((a, b) => key(a).localeCompare(key(b)));
    }
    readonly property string connectionSummary: {
        var result = [];
        for (var i = 0; i < devices.length; ++i) {
            var device = devices[i];
            if (device.type === DeviceType.Wired) result.push("Ethernet: " + (device.connected ? "Connected" : "Not connected"));
            else if (device.type === DeviceType.Wifi && device.connected) {
                var active = device.networks.values.filter(n => n.connected);
                result.push("Wi-Fi: " + (active.length ? clean(active[0].name) : "Connected"));
            }
        }
        return result.join("\n") || "No active connection";
    }
    readonly property string internetSummary: {
        if (!Networking.connectivityCheckEnabled) return "Internet access has not been checked.";
        switch (Networking.connectivity) {
        case NetworkConnectivity.Full: return "Internet access available";
        case NetworkConnectivity.Portal: return "This network requires a browser sign-in.";
        case NetworkConnectivity.Limited: return "Connected, but internet access is limited.";
        case NetworkConnectivity.None: return "No internet connection";
        default: return "Internet access has not been checked.";
        }
    }
    readonly property var rows: makeRows()
    signal backRequested()
    signal navigationReset()

    // The page owns deferred layout work; destroyed delegates are never captured.
    Timer {
        id: focusSettle
        interval: 1
        repeat: false
        onTriggered: network.settleFocus()
    }
    function requestFocusSettle() { focusSettle.restart(); }
    function settleFocus() {
        if (!(visible) || selected < 0 || selected >= networkList.count) return;
        networkList.forceLayout();
        networkList.positionViewAtIndex(selected, ListView.Contain);
    }

    function px(value) { return Math.round(value * uiScale); }
    function clean(value) { return String(value || "").replace(/[\x00-\x1f\x7f]/g, "").slice(0, 100); }
    function key(n) { return n && n.device ? JSON.stringify([n.device.name, n.name, n.security]) : ""; }
    function personal(n) {
        return n && (n.security === WifiSecurityType.WpaPsk || n.security === WifiSecurityType.Wpa2Psk || n.security === WifiSecurityType.Sae);
    }
    function present(n) { return n && networks.indexOf(n) >= 0; }
    function row(key, title, detail, net) { return { key: key, title: title, detail: detail, net: net || null }; }
    function makeRows() {
        if (view === "password") return [];
        if (view === "forget") return [
            row("cancel", "Keep saved network", "Go back"),
            row("confirm-forget", "Forget network", "Remove all saved profiles for this network")
        ];
        if (view === "detail" && targetNetwork) {
            var n = targetNetwork;
            var details = [row(n.connected ? "disconnect" : "connect", n.connected ? "Disconnect" : "Connect",
                n.connected ? "Disconnect Wi-Fi on this adapter" : n.known ? "Use the saved connection" : "Join this network")];
            if (personal(n) && !n.connected) details.push(row("password", "Enter password", "Use a new Wi-Fi password"));
            if (n.known) details.push(row("forget", "Forget saved network", "Remove saved connection settings"));
            details.push(row("back", "Back", "Return to networks"));
            return details;
        }
        var result = [];
        if (available && wifiDevices.length) {
            result.push(row("radio", "Wi-Fi", !Networking.wifiHardwareEnabled ? "Blocked by hardware" : Networking.wifiEnabled ? "On · Select to turn off" : "Off · Select to turn on"));
            if (Networking.wifiEnabled) {
                for (var i = 0; i < networks.length; ++i) {
                    var net = networks[i];
                    // Keep the list model stable when signal/connection properties change.
                    // The delegate observes those live properties independently.
                    result.push(row(key(net), clean(net.name), "", net));
                }
            }
        }
        result.push(row("back", "Back", "Return to Settings"));
        return result;
    }
    function description(item) {
        var n = item.net;
        if (!n) return item.detail;
        var status = n.connected ? "Connected" : n.stateChanging ? "Connection changing" : n.known ? "Saved" : "Available";
        return status + " · " + Math.round(n.signalStrength * 100) + "% · " + WifiSecurityType.toString(n.security)
            + (wifiDevices.length > 1 && n.device ? " · " + clean(n.device.name) : "");
    }
    function select(index) {
        if (!rows.length) return;
        selected = Math.max(0, Math.min(rows.length - 1, index));
        selectedKey = rows[selected].key;
        if (initialized) networkList.positionViewAtIndex(selected, ListView.Contain);
    }
    function move(direction) {
        if (editingPassword) passwordKeys.navigate(0, direction);
        else if (rows.length) select((selected + direction + rows.length) % rows.length);
    }
    function horizontal(direction) {
        if (editingPassword) passwordKeys.navigate(direction, 0);
        else if (direction < 0) back();
        else activate();
    }
    function cycleKeyboard() { if (editingPassword) passwordKeys.cycleView(); }
    function changeView(next) {
        navigationReset();
        passwordKeys.reset();
        view = next;
        selectedKey = next === "list" ? listKey : "";
        var index = rows.findIndex(r => r.key === selectedKey);
        select(index >= 0 ? index : 0);
    }
    function back() {
        if (view === "password" || view === "forget") changeView(targetNetwork ? "detail" : "list");
        else if (view === "detail") { changeView("list"); targetNetwork = null; }
        else backRequested();
    }
    function finish(message) {
        deadline.stop();
        operation = "";
        pendingNetwork = null;
        pendingDevice = null;
        pendingPasswordProvided = false;
        notice = message;
    }
    function begin(kind, n, device) {
        if (busy || !available) return false;
        pendingNetwork = n || null;
        pendingDevice = device || null;
        operation = kind;
        notice = "";
        deadline.interval = kind === "connect" ? 45000 : 10000;
        deadline.restart();
        return true;
    }
    function settle() {
        if (!busy) return;
        if (!available) { finish("Network service is unavailable."); return; }
        if (operation === "radio") {
            if (Networking.wifiEnabled === expectedRadio) finish(expectedRadio ? "Wi-Fi is on." : "Wi-Fi is off.");
        } else if (operation === "disconnect") {
            if (!pendingDevice || devices.indexOf(pendingDevice) < 0) finish("The network adapter was removed.");
            else if (!pendingDevice.connected) finish("Disconnected.");
        } else if (!present(pendingNetwork)) finish("The selected network is no longer available.");
        else if (operation === "connect" && pendingNetwork.connected) finish("Connected.");
        else if (operation === "forget" && !pendingNetwork.known) finish("Saved connection removed.");
    }
    function connectNetwork(password, withPassword) {
        var n = targetNetwork;
        if (!present(n) || busy || n.stateChanging || !Networking.wifiEnabled) return;
        // New personal networks always prompt before asking NetworkManager to connect.
        // Keep NoSecrets handling below for expired/wrong saved credentials.
        if (!withPassword && !n.known && personal(n)) {
            notice = "";
            changeView("password");
            return;
        }
        if (withPassword) {
            if (!personal(n)) { notice = "This network requires a different authentication method."; return; }
            var valid = n.security === WifiSecurityType.Sae
                ? /^[\x20-\x7e]{1,63}$/.test(password)
                : /^[\x20-\x7e]{8,63}$/.test(password) || /^[0-9a-fA-F]{64}$/.test(password);
            if (!valid) { notice = n.security === WifiSecurityType.Sae ? "Enter a password of 1–63 characters." : "Use 8–63 characters, or a 64-digit hexadecimal key."; return; }
        } else if (!n.known && !personal(n) && n.security !== WifiSecurityType.Open && n.security !== WifiSecurityType.Owe) {
            notice = "This network requires setup outside this preview.";
            return;
        }
        if (!begin("connect", n, n.device)) return;
        pendingPasswordProvided = withPassword;
        changeView("detail");
        // Always try stored credentials first unless the user explicitly edits them.
        if (withPassword) n.connectWithPsk(password);
        else n.connect();
        settle();
    }
    function activate() {
        if (editingPassword) { passwordKeys.activate(); return; }
        var item = rows[selected];
        if (!item) return;
        if (item.key === "back" || item.key === "cancel") { back(); return; }
        if (item.net) {
            listKey = item.key;
            targetNetwork = item.net;
            changeView("detail");
            return;
        }
        if (busy || !available) return;
        if (item.key === "radio") {
            if (!Networking.wifiHardwareEnabled) { notice = "Wi-Fi is blocked by a hardware switch."; return; }
            expectedRadio = !Networking.wifiEnabled;
            if (begin("radio", null, null)) { Networking.wifiEnabled = expectedRadio; settle(); }
            return;
        }
        if (!present(targetNetwork)) return;
        if (!Networking.wifiEnabled) { notice = "Turn on Wi-Fi before changing this connection."; return; }
        if (targetNetwork.stateChanging) { notice = "Wait for the current connection change to finish."; return; }
        if (item.key === "connect") connectNetwork("", false);
        else if (item.key === "password") { notice = ""; changeView("password"); }
        else if (item.key === "disconnect") {
            var device = targetNetwork.device;
            if (begin("disconnect", targetNetwork, device)) { device.disconnect(); settle(); }
        } else if (item.key === "forget") changeView("forget");
        else if (item.key === "confirm-forget") {
            var n = targetNetwork;
            if (begin("forget", n, n.device)) { changeView("detail"); n.forget(); settle(); }
        }
    }
    function physicalKey(event) {
        if (!editingPassword) return false;
        if (event.key === Qt.Key_Escape || event.key === Qt.Key_Back) { if (!event.isAutoRepeat) back(); }
        else if (event.key === Qt.Key_Up || event.key === Qt.Key_Down || event.key === Qt.Key_Left || event.key === Qt.Key_Right) return false;
        else if (event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete) passwordKeys.erase();
        else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            if (!event.isAutoRepeat) connectNetwork(passwordKeys.secret, true);
        } else if (!(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) passwordKeys.append(event.text);
        return true;
    }
    onRowsChanged: if (initialized) {
        var index = rows.findIndex(r => r.key === selectedKey);
        select(index >= 0 ? index : 0);
    }
    onNetworksChanged: if (initialized) {
        settle();
        if (targetNetwork && !present(targetNetwork)) {
            targetNetwork = null;
            changeView("list");
            notice = "The selected network is no longer available.";
        }
    }
    onPendingNetworkChanged: if (initialized && busy) Qt.callLater(() => network.settle())
    onDevicesChanged: if (initialized && busy) Qt.callLater(() => network.settle())
    onTargetNetworkChanged: if (initialized) {
        if (!targetNetwork && view !== "list") { changeView("list"); notice = "The selected network is no longer available."; }
    }
    onAvailableChanged: if (initialized && !available) { finish("Network service is unavailable."); targetNetwork = null; changeView("list"); }
    Component.onCompleted: { initialized = true; select(0); }

    // Scope automatic scanning to this page and restore the previous property.
    Repeater {
        model: network.wifiDevices
        delegate: Item {
            id: scanner
            required property var modelData
            visible: false
            Binding {
                target: scanner.modelData; property: "scannerEnabled"; value: true
                when: network.visible && Networking.wifiEnabled
                restoreMode: Binding.RestoreBindingOrValue
            }
        }
    }
    Connections {
        target: Networking
        function onWifiEnabledChanged() {
            network.settle();
            if (!Networking.wifiEnabled && network.editingPassword) network.changeView("detail");
        }
    }
    Connections {
        target: network.pendingDevice
        function onConnectedChanged() { network.settle(); }
    }
    Connections {
        target: network.pendingNetwork
        function onConnectedChanged() { network.settle(); }
        function onKnownChanged() { network.settle(); }
        function onConnectionFailed(reason) {
            if (network.operation !== "connect") return;
            var n = network.pendingNetwork;
            if (reason === ConnectionFailReason.NoSecrets && network.personal(n) && network.present(n)) {
                var rejected = network.pendingPasswordProvided || n.known;
                network.finish(rejected ? "Password was not accepted. Check it and try again." : "Enter the Wi-Fi password and try again.");
                network.targetNetwork = n;
                network.changeView("password");
            } else network.finish("Connection failed: " + ConnectionFailReason.toString(reason));
        }
    }
    Timer {
        id: deadline
        onTriggered: {
            network.settle();
            if (network.busy) network.finish("The change was not confirmed. Check the connection status before retrying.");
        }
    }
    Column {
        id: status
        width: parent.width; spacing: network.px(7)
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 3; elide: Text.ElideRight
            text: !network.available ? "Network service is unavailable."
                : network.view !== "list" && network.targetNetwork ? network.clean(network.targetNetwork.name) : network.connectionSummary
            font.pixelSize: network.px(20); color: "#ECEEF1"
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 2; elide: Text.ElideRight
            text: network.view !== "list" && network.targetNetwork
                ? (network.targetNetwork.connected ? "● Connected" : network.targetNetwork.stateChanging ? "Connection changing…" : "Not connected")
                : network.internetSummary
            font.pixelSize: network.px(15)
            color: network.targetNetwork && network.targetNetwork.connected ? "#98D8AE" : "#BBC6D3"
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap; maximumLineCount: 3; elide: Text.ElideRight
            text: network.busy ? (network.operation === "connect" ? "Connecting… You can go back while it finishes." : "Applying change…")
                : network.view === "forget" ? "All saved profiles for this network will be removed. An active connection may also be interrupted."
                : network.notice || (!network.available ? "Return to Settings and try again."
                : !network.wifiDevices.length ? "No Wi-Fi adapter is available."
                : network.view === "list" && Networking.wifiEnabled && !network.networks.length ? "Looking for networks…" : "")
            visible: text.length > 0
            font.pixelSize: network.px(14); color: "#D9BC96"
        }
    }
    PasswordKeyboard {
        id: passwordKeys
        visible: network.editingPassword
        uiScale: network.uiScale
        anchors { left: parent.left; right: parent.right; top: status.bottom; bottom: parent.bottom; topMargin: network.px(20) }
        onSubmitted: password => network.connectNetwork(password, true)
    }
    ListView {
        id: networkList
        property point lastPointerPosition: Qt.point(-100000, -100000)
        visible: !network.editingPassword
        anchors { left: parent.left; right: parent.right; top: status.bottom; bottom: parent.bottom; topMargin: network.px(18) }
        model: network.rows; currentIndex: network.selected
        spacing: network.px(6); clip: true; boundsBehavior: Flickable.StopAtBounds
        delegate: MenuFocusRow {
            id: networkRow
            required property var modelData
            required property int index
            width: networkList.width; baseHeight: 78
            uiScale: network.uiScale; chosen: network.selected === index
            onFocusSettled: if (networkRow.chosen) network.requestFocusSettle()
            MenuFocusBackground { chosen: networkRow.chosen; uiScale: network.uiScale; emphasis: networkRow.focusAmount }
            MenuIcon {
                x: network.px(16); y: Math.round((parent.height - height) / 2)
                width: network.px(28 + 2 * networkRow.focusAmount); height: width
                name: networkRow.modelData.key === "back" || networkRow.modelData.key === "cancel" ? "back" : "network"
                ink: networkRow.chosen ? "#30271E" : "#DEC09A"
            }
            Column {
                x: network.px(58); y: Math.round((parent.height - height) / 2)
                width: parent.width - network.px(96); spacing: network.px(5)
                MenuText {
                    width: parent.width; elide: Text.ElideRight; text: networkRow.modelData.title
                    font.pixelSize: network.px(21 + 2 * networkRow.focusAmount)
                    color: networkRow.chosen ? "#211E1B" : "#ECEEF1"
                }
                MenuText {
                    width: parent.width; elide: Text.ElideRight; text: network.description(networkRow.modelData)
                    font.pixelSize: network.px(14)
                    color: networkRow.chosen ? "#473627" : networkRow.modelData.net && networkRow.modelData.net.connected ? "#98D8AE" : "#BBC6D3"
                }
            }
            Rectangle {
                visible: networkRow.modelData.net !== null && networkRow.modelData.net.connected
                anchors { right: parent.right; rightMargin: network.px(17); verticalCenter: parent.verticalCenter }
                width: network.px(10); height: width; radius: width / 2
                color: "#58BA82"; border.color: "#203C2B"; border.width: 1
            }
            MouseArea {
                id: networkPointer
                anchors.fill: parent; hoverEnabled: true
                onEntered: if (networkRow.pointerMoved(networkPointer)) network.select(index)
                onPositionChanged: if (networkRow.pointerMoved(networkPointer)) network.select(index)
                onClicked: { network.select(index); network.activate(); }
                onWheel: wheel => { network.move(wheel.angleDelta.y > 0 ? -1 : 1); wheel.accepted = true; }
            }
        }
    }
}
