import QtQuick
import QtQml.Models
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Notifications

ShellRoot {
    id: root

    readonly property string outputName: Quickshell.env("ZAMAN_TOAST_OUTPUT") || "HDMI-A-1"
    readonly property var toastScreen: Quickshell.screens.find(s => s.name === outputName) || null
    property var retained: ({})

    FontLoader { id: primary; source: Qt.resolvedUrl("../../../assets/fonts/saira-condensed/SairaCondensed-Regular.ttf") }
    FontLoader { id: secondary; source: Qt.resolvedUrl("../../../assets/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf") }
    ListModel { id: toasts }

    function indexOf(id) {
        for (let i = 0; i < toasts.count; ++i)
            if (toasts.get(i).toastId === id) return i;
        return -1;
    }

    function deadline(notification) {
        // The v0.3.0 C++ implementation exposes the D-Bus timeout in ms,
        // despite its generated type documentation describing seconds.
        const timeoutMs = notification.expireTimeout;
        if (timeoutMs === 0 || (timeoutMs < 0 && notification.urgency === NotificationUrgency.Critical)) return 0;
        return Date.now() + (timeoutMs > 0 ? timeoutMs : 6000);
    }

    function snapshot(notification) {
        const hints = notification.hints || {};
        const kind = String(hints["x-zaman-kind"] || "system");
        const state = String(hints["x-zaman-state"] || "");
        const bad = state === "disconnected" || state === "muted" || state === "error"
            || notification.urgency === NotificationUrgency.Critical;
        const good = state === "connected" || state === "ready";
        return {
            toastId: notification.id,
            heading: String(notification.summary || "Notification").slice(0, 160),
            message: String(notification.body || "").slice(0, 480),
            sourceName: String(notification.appName || "Zaman").slice(0, 48),
            kindLabel: kind === "network" ? "NET" : kind === "audio" ? "AUD" : kind === "input" ? "PAD" : "SYS",
            accent: bad ? "#ff8b8b" : good ? "#73d595" : "#e6b25a",
            expiresAt: deadline(notification),
            closing: false,
            removeAt: 0
        };
    }

    function refresh(notification) {
        if (retained[notification.id] !== notification) return;
        const i = indexOf(notification.id);
        if (i >= 0) toasts.set(i, snapshot(notification));
    }

    function beginExit(id) {
        const i = indexOf(id);
        if (i < 0 || toasts.get(i).closing) return;
        toasts.setProperty(i, "closing", true);
        toasts.setProperty(i, "removeAt", Date.now() + 210);
    }

    function finish(i) {
        const id = toasts.get(i).toastId;
        const notification = retained[id];
        delete retained[id];
        toasts.remove(i);
        // A client-closed object was released in its closed callback. Only
        // live, server-owned notifications receive the expiry reply here.
        if (notification) notification.expire();
    }

    NotificationServer {
        id: server
        keepOnReload: false
        bodySupported: true
        bodyMarkupSupported: false
        actionsSupported: false
        persistenceSupported: false
        imageSupported: false

        onNotification: notification => {
            notification.tracked = true;
            const id = notification.id;
            const existing = root.indexOf(id);
            if (root.retained[id] !== notification) {
                root.retained[id] = notification;
                notification.closed.connect(() => {
                    if (root.retained[id] !== notification) return;
                    delete root.retained[id];
                    root.beginExit(id);
                });
                notification.summaryChanged.connect(() => root.refresh(notification));
                notification.bodyChanged.connect(() => root.refresh(notification));
                notification.appNameChanged.connect(() => root.refresh(notification));
                notification.expireTimeoutChanged.connect(() => root.refresh(notification));
                notification.urgencyChanged.connect(() => root.refresh(notification));
                notification.hintsChanged.connect(() => root.refresh(notification));
            }
            if (existing >= 0) {
                toasts.set(existing, root.snapshot(notification));
            } else {
                // Bound the trial's memory and visible stack. A fourth toast
                // expires the oldest; this is deliberately not a history view.
                while (toasts.count >= 3) root.finish(0);
                toasts.append(root.snapshot(notification));
            }
        }
    }

    Timer {
        interval: 100
        repeat: true
        running: toasts.count > 0
        onTriggered: {
            const now = Date.now();
            for (let i = toasts.count - 1; i >= 0; --i) {
                const entry = toasts.get(i);
                if (entry.closing && entry.removeAt <= now) root.finish(i);
                else if (!entry.closing && entry.expiresAt > 0 && entry.expiresAt <= now) root.beginExit(entry.toastId);
            }
        }
    }

    PanelWindow {
        id: panel
        screen: root.toastScreen
        visible: root.toastScreen !== null && toasts.count > 0
        anchors { top: true; right: true }
        readonly property real uiScale: Math.max(0.65, Math.min(2, root.toastScreen ? root.toastScreen.height / 1080 : 1))
        margins { top: Math.round(36 * panel.uiScale); right: Math.round(36 * panel.uiScale) }
        implicitWidth: root.toastScreen ? Math.min(Math.round(560 * uiScale), root.toastScreen.width - 2 * margins.right) : 560
        implicitHeight: Math.max(1, Math.ceil(stack.implicitHeight))
        color: "transparent"
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
        WlrLayershell.namespace: "zaman-toast-trial"
        // Empty input region: even clicks on a toast reach the game beneath.
        mask: Region {}

        Column {
            id: stack
            width: panel.width
            spacing: 12 * panel.uiScale
            move: Transition { NumberAnimation { properties: "y"; duration: 180; easing.type: Easing.OutCubic } }
            Repeater {
                model: toasts
                delegate: ToastCard {
                    width: stack.width
                    uiScale: panel.uiScale
                    titleFont: primary.name || "Sans Serif"
                    detailFont: secondary.name || "monospace"
                }
            }
        }
    }
}
