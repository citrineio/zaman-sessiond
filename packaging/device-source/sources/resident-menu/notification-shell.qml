//@ pragma ShellId zaman-notifications
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import "qml"
import "notifications"

ShellRoot {
    id: root
    readonly property string outputName: Quickshell.env("ZAMAN_TOAST_OUTPUT") || "HDMI-A-1"
    readonly property var toastScreen: Quickshell.screens.find(s => s.name === outputName) || null
    function reportRenderer() {
        if (notifications.available) notifications.send({action:"renderer-status", output:outputName, outputPresent:toastScreen !== null});
    }
    NotificationClient {
        id: notifications
        onAvailableChanged: if (available) root.reportRenderer()
    }
    Timer { interval:1000; repeat:true; running:notifications.available; onTriggered:root.reportRenderer() }
    IpcHandler {
        target: "notificationRenderer"
        function status(): string { return JSON.stringify({output:root.outputName, outputPresent:root.toastScreen !== null, connected:notifications.available, visible:notifications.popups.length}); }
    }
    Loader {
        source: Quickshell.env("ZAMAN_AUDIO_EVENT_QML") ? "file://" + Quickshell.env("ZAMAN_AUDIO_EVENT_QML") : ""
        onLoaded: item.client = notifications
    }
    Component.onCompleted: Quickshell.watchFiles = false
    PanelWindow {
        id: panel
        screen: root.toastScreen
        visible: root.toastScreen !== null && !notifications.menuOpen && stack.hasCards
        anchors { top:true; right:true }
        readonly property real uiScale: Math.max(0.65, Math.min(1.6, root.toastScreen ? root.toastScreen.height / 1080 : 1))
        margins { top:Math.round(32 * panel.uiScale); right:Math.round(32 * panel.uiScale) }
        implicitWidth: root.toastScreen ? Math.min(Math.round(420 * uiScale), root.toastScreen.width - 2 * margins.right) : 420
        implicitHeight: Math.max(1, stack.implicitHeight)
        color:"transparent"
        exclusionMode:ExclusionMode.Ignore
        WlrLayershell.layer:WlrLayer.Overlay
        WlrLayershell.keyboardFocus:WlrKeyboardFocus.None
        WlrLayershell.namespace:"zaman-notifications"
        mask: Region {}
        NotificationToastStack {
            id: stack
            width: panel.width
            uiScale: panel.uiScale
            popups: notifications.popups
        }
    }
}
