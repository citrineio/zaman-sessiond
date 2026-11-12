import QtQuick
import Quickshell
import Quickshell.Io

Item {
    id: client
    visible: false
    property var records: []
    property var popups: []
    property int unread: 0
    property var preferences: ({dnd:false, steamReminder:true, steamStartup:true, backgroundProgress:true})
    property bool viewing: false
    property bool presentingMenu: false
    property bool menuOpen: false
    property string outputName: ""
    property bool outputPresent: false
    property bool available: false
    property string error: "Notifications are connecting…"
    readonly property string helper: Quickshell.env("ZAMAN_NOTIFICATION_CLIENT") || ""
    function send(message) {
        if (available && connection.running) connection.write(JSON.stringify(message) + "\n");
    }
    function receive(line) {
        var data;
        try { data = JSON.parse(line); } catch (error) { return; }
        if (data.type !== "notifications") return;
        var wasAvailable = available;
        available = true;
        records = data.records || [];
        popups = data.visible || [];
        unread = data.unread || 0;
        preferences = data.preferences || ({dnd:false, steamReminder:true, steamStartup:true, backgroundProgress:true});
        menuOpen = data.menuOpen === true;
        if (data.renderer && typeof data.renderer.output === "string") {
            outputName = data.renderer.output;
            outputPresent = data.renderer.outputPresent === true;
        }
        error = data.error || "";
        if (!wasAvailable && viewing) send({action:"view", active:true});
        if (!wasAvailable && presentingMenu) send({action:"menu", active:true});
    }
    onViewingChanged: send({action:"view", active:viewing})
    onPresentingMenuChanged: send({action:"menu", active:presentingMenu})
    Process {
        id: connection
        command: ["/usr/bin/python3", client.helper, "watch"]
        stdinEnabled: true
        running: client.helper.length > 0
        stdout: SplitParser { onRead: line => client.receive(line) }
        stderr: SplitParser { onRead: line => console.warn(line) }
        onExited: {
            client.available = false;
            client.error = "Notifications unavailable. Check the notification service.";
            client.popups = [];
        }
    }
    Timer {
        interval: 2000
        repeat: true
        running: client.helper.length > 0 && !connection.running
        onTriggered: connection.running = true
    }
}
