import QtQuick 2.12
import QtQuick.Window 2.12
import Qt.labs.folderlistmodel 2.15
import "components"

FocusScope {
    id: root
    readonly property string romRefreshMemoryKey: "zaman.romLibraryRefreshToken"
    function inspectRomRefreshSignal() {
        if (romRefreshSignals.status !== FolderListModel.Ready || romRefreshSignals.count !== 1) return;
        var token = romRefreshSignals.get(0,"fileName");
        if (api.memory.has(romRefreshMemoryKey) && String(api.memory.get(romRefreshMemoryKey)) === token) return;
        if (!romRefreshDebounce.running) romRefreshDebounce.start();
    }
    FolderListModel {
        id: romRefreshSignals
        folder: Qt.resolvedUrl("rom-library-refresh")
        nameFilters:["*.signal"]; showDirs:false; showDotAndDotDot:false
        onStatusChanged:root.inspectRomRefreshSignal()
        onCountChanged:root.inspectRomRefreshSignal()
    }
    Timer {
        id: romRefreshDebounce
        interval:500; repeat:false
        onTriggered: {
            // A paused/background Pegasus must not rescan over active gameplay.
            if (!root.Window.window || !root.Window.window.active || mainShell.launching) { restart(); return; }
            if (romRefreshSignals.status !== FolderListModel.Ready || romRefreshSignals.count !== 1) return;
            api.memory.set(root.romRefreshMemoryKey,romRefreshSignals.get(0,"fileName"));
            Internal.settings.reloadProviders();
        }
    }
    readonly property string refreshMemoryKey: "zaman.libraryRefreshToken"
    property bool refreshInitialized: false
    property string lastRefreshToken: ""
    readonly property string steamRefreshMemoryKey: "zaman.steamLibraryRefreshToken"
    property bool steamRefreshInitialized: false
    property string lastSteamRefreshToken: ""

    function inspectRefreshSignal() {
        // The sync helper briefly publishes two names while replacing a
        // signal. Wait for the single, stable entry before requesting a scan.
        if (refreshSignals.status !== FolderListModel.Ready || refreshSignals.count !== 1)
            return;

        var token = refreshSignals.get(0, "fileName");
        if (!refreshInitialized) {
            lastRefreshToken = api.memory.has(refreshMemoryKey)
                ? String(api.memory.get(refreshMemoryKey)) : "";
            refreshInitialized = true;
        }
        if (token === lastRefreshToken)
            return;

        lastRefreshToken = token;
        api.memory.set(refreshMemoryKey, token);
        refreshDebounce.restart();
    }

    function inspectSteamRefreshSignal() {
        // The sync helper briefly publishes two names while replacing a
        // signal. Wait for the single, stable entry before requesting a scan.
        if (steamRefreshSignals.status !== FolderListModel.Ready || steamRefreshSignals.count !== 1)
            return;

        var token = steamRefreshSignals.get(0, "fileName");
        if (!steamRefreshInitialized) {
            lastSteamRefreshToken = api.memory.has(steamRefreshMemoryKey)
                ? String(api.memory.get(steamRefreshMemoryKey)) : "";
            steamRefreshInitialized = true;
        }
        if (token === lastSteamRefreshToken)
            return;

        lastSteamRefreshToken = token;
        api.memory.set(steamRefreshMemoryKey, token);
        refreshDebounce.restart();
    }

    FolderListModel {
        id: refreshSignals
        folder: Qt.resolvedUrl("library-refresh")
        nameFilters: ["*.signal"]
        showDirs: false
        showDotAndDotDot: false
        onStatusChanged: root.inspectRefreshSignal()
        onCountChanged: root.inspectRefreshSignal()
    }

    FolderListModel {
        id: steamRefreshSignals
        folder: Qt.resolvedUrl("steam-library-refresh")
        nameFilters: ["*.signal"]
        showDirs: false
        showDotAndDotDot: false
        onStatusChanged: root.inspectSteamRefreshSignal()
        onCountChanged: root.inspectSteamRefreshSignal()
    }

    Timer {
        id: refreshPoll
        interval: 500
        repeat: true
        running: true
        onTriggered: { root.inspectRefreshSignal(); root.inspectSteamRefreshSignal(); root.inspectRomRefreshSignal(); }
    }

    Timer {
        id: refreshDebounce
        interval: 350
        repeat: false
        onTriggered: Internal.settings.reloadProviders()
    }

    Shell { id: mainShell; anchors.fill: parent; backend: api; focus: true }
}
