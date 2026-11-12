//@ pragma ShellId zaman-menu-dev

import QtQuick
import QtQuick.Window
import Quickshell
import Quickshell.Io
import Quickshell.Wayland

ShellRoot {
    id: root
    property bool menuOpen: false
    property bool opening: false
    property bool ready: false
    property bool closing: false
    property var closeOrigin: ({page:"",tab:0})
    property bool helperIdle: false
    property double openStartedMs: 0
    property double lastReadyMs: -1
    property double lastVisibleMs: -1
    property double firstFrameMs: -1
    property double slideFinishedMs: -1
    property string entryPhase: "hidden"
    readonly property string menuInstance: Date.now().toString()
    property int menuGeneration: 0
    property string sessionAction: "exit-game"
    property bool exitBusy: false
    property bool exitResultReceived: false
    property string exitNotice: ""
    readonly property string sessionActionPath: Quickshell.env("ZAMAN_MENU_SESSION_BRIDGE") || ""
    property string foreground: "library"
    property string storefrontId: ""
    property string storeGeneration: "0"
    property bool storeGame: false
    property var pendingStoreAction: null
    property var storeRequest: ({action:"", id:"", generation:"0"})
    property string storeNotice: ""
    property bool storeResultReceived: false
    property var transactionJobs: []
    property bool transactionsAvailable: false
    readonly property int pendingTransactions: transactionJobs.filter(job => ["queued", "waiting", "running", "cancelling"].indexOf(job.phase) >= 0).length
    readonly property string transactionClient: Quickshell.env("DUKKAN_TRANSACTION_CLIENT") || ""
    readonly property string transferClient: Quickshell.env("ZAMAN_TRANSFER_CLIENT") || ""
    property var transferSnapshot: ({jobs:[],systems:[],enabled:false})
    property bool transferAvailable: false
    property string transferError: ""
    property var transferRequest: ({action:"snapshot"})
    readonly property int pendingImports: transferSnapshot.pending || 0
    property var scrapeReturnLocation: ({page:"",returnPage:"",entry:({label:"",key:"",detail:""})})
    function openTransfers() {
        stopRepeat();
        returnPage = page;
        pageEntry = ({label:"Transfer Games",key:"transfer",detail:"Receive and import games"});
        page = "transfer";
    }
    function openScraping(importId, system, jobId) {
        stopRepeat();
        scrapeReturnLocation=({page:page,returnPage:returnPage,entry:pageEntry});
        if (page === "") mainSelected=selected;
        pageEntry=({label:"Artwork & Game Info",key:"scrape",detail:"Covers, screenshots and descriptions"});
        scrapePage.open(importId || "",system || "",jobId || "");
        page="scrape";
    }
    function transferResult(line) {
        try {
            var result = JSON.parse(line);
            if (result.type === "transfer") {
                transferSnapshot = result;
                transferAvailable = true;
                transferError = "";
            } else if (result.type === "error") transferError = result.error;
        } catch (error) { transferError = "Invalid receiver response"; }
    }
    Process {
        id: transferPoll
        property bool received: false
        command:["/usr/bin/python3",root.transferClient,"snapshot"]
        onStarted: received = false
        stdout: SplitParser { onRead: line => { transferPoll.received = true; root.transferResult(line); } }
        onExited: (code, status) => { if (code !== 0 || !received) root.transferAvailable = false; }
    }
    Process {
        id: transferAction
        command:["/usr/bin/python3",root.transferClient,"request"]
        stdinEnabled:true
        onStarted:write(JSON.stringify(root.transferRequest)+"\n")
        stdout: SplitParser { onRead: line => root.transferResult(line) }
    }
    Timer {
        interval:1000; repeat:true; triggeredOnStart:true
        running:root.menuOpen && root.transferClient.length > 0
        onTriggered: if (!transferPoll.running && !transferAction.running) transferPoll.running = true
    }
    property int controllerCount: 0
    property bool displaySupported: false
    property var controllerDevices: []
    property string controllerWarning: ""
    property int activeTab: 0
    property var tabLocations: [null, null, null]
    property int navigationRevision: 0
    property bool audioLoaded: false
    property bool networkLoaded: false
    readonly property bool canSwitchTab: menuOpen && !closing && !exitBusy
    readonly property bool menuPresentsToasts: menuOpen && notifications.available && notifications.outputPresent
        && overlay.screen !== null && overlay.screen.name === notifications.outputName
    readonly property var notificationSettings: [
        {key:"dnd", preference:"dnd", label:"Do Not Disturb", detail:notifications.preferences.dnd ? "On · Critical alerts remain visible" : "Off · Show routine popups"},
        {key:"steam-startup", preference:"steamStartup", label:"Steam startup notice", detail:notifications.preferences.steamStartup ? "On · Notice while starting Steam" : "Off · Startup notice disabled"},
        {key:"steam-reminder", preference:"steamReminder", label:"Steam menu shortcut reminder", detail:notifications.preferences.steamReminder ? "On · Start + Select reminder when Steam opens" : "Off · Reminder disabled"}
    ]
    NotificationClient {
        id: notifications
        presentingMenu: root.menuPresentsToasts
        viewing: root.menuOpen && root.activeTab === 1
    }
    property int selected: 0
    property string page: ""
    property bool bluetoothLoaded: false
    property int mainSelected: 0
    property int settingsSelected: 0
    property int powerSelected: 0
    property string returnPage: ""
    property var pageEntry: ({ label: "", key: "", detail: "" })
    readonly property bool listPage: page === "" || page === "settings" || page === "power" || page === "storefronts" || page === "notification-settings"
    readonly property bool navigationPage: page === "scrape" || page === "transfer" || page === "transactions" || page === "notifications" || listPage || page === "audio" || page === "controllers" || page === "network" || page === "bluetooth" || page === "display" || page === "exit" || page === "reboot" || page === "shutdown" || page === "store-close"
    property int heldDirection: 0
    property int heldHorizontal: 0
    property bool keyboardCycleHeld: false
    property var pressed: ({})
    readonly property bool editingKeyboard: page === "network" && networkPageLoader.item && networkPageLoader.item.editingPassword
        || page === "bluetooth" && bluetoothPageLoader.item && bluetoothPageLoader.item.editingCode
        || page === "scrape" && scrapePage.editingText
    readonly property real uiScale: Math.max(0.65, Math.min(1.4, overlay.height / 1080))
    readonly property string helperPath: Quickshell.env("ZAMAN_MENU_DEV_INPUT") || ""
    readonly property string uiFontName: "Sans Serif"
    property string clockText: Qt.formatDateTime(new Date(), "h:mm AP")
    readonly property string monoFontName: monoFont.status === FontLoader.Ready ? monoFont.name : "Monospace"
    readonly property var mainEntries: [
        { label: storefrontId.length ? "Resume " + (storefrontId === "steam" ? "Steam" : "Dukkan") : foreground === "game" ? "Resume" : "Return to library", key: "resume", detail: storefrontId.length ? "Return to the active application" : foreground === "game" ? "Return to your game" : "Continue browsing your games" },
        { label: "Activity" + (pendingTransactions ? " · " + pendingTransactions : ""), key: "transactions", detail: pendingTransactions ? transactionJobs.find(job => ["queued", "waiting", "running", "cancelling"].indexOf(job.phase) >= 0).stage : "Dukkan transactions and results" },
        { label: "Transfer Games" + (pendingImports ? " · " + pendingImports : ""), key: "transfer", detail: "Receive games and review pending imports" },
        { label: "Artwork & Game Info", key: "scrape", detail: "Find covers, screenshots and descriptions" },
        { label: "Audio", key: "audio", detail: "Volume and audio output" },
        { label: "Power", key: "power", detail: "Device power options" }
    ].concat(foreground === "library" ? [
        { label: "Storefronts", key: "storefronts", detail: "Dukkan and Steam" }
    ] : []).concat(storefrontId.length && !storeGame ? [
        { label: "Return to library", key: "store-home", detail: "Keep the storefront running in the background" }
    ] : []).concat(storefrontId.length && (!storeGame || storefrontId === "steam") ? [
        { label: "Close " + (storefrontId === "steam" ? "Steam" : "Dukkan"), key: "store-close", detail: storeGame ? "Close Steam and its running game" : "Close the storefront and return to library" }
    ] : []).concat(foreground === "game" && !storefrontId.length ? [
        { label: "Exit Game", key: "exit", detail: "Close the current game" }
    ] : [])
    readonly property var storefrontEntries: [
        { label: "Dukkan", key: "store-dukkan", detail: "Browse and install games" },
        { label: "Steam", key: "store-steam", detail: "Open your Steam library and store" }
    ]
    readonly property var settingsEntries: [
        { label: "Audio", key: "audio", detail: "Volume and audio output" },
        { label: "Notifications", key: "notification-settings", detail: "Do Not Disturb and Steam notices" },
        { label: "Controllers", key: "controllers", detail: "Connected controllers and active input profiles" },
        { label: "Network", key: "network", detail: "Connections and saved networks" },
        { label: "Bluetooth", key: "bluetooth", detail: "Pair and manage your devices" },
        { label: "Display", key: "display", detail: "Resolution and refresh rate" },
        { label: "Appearance", key: "appearance", detail: "Theme, colors and interface style" }
    ]
    readonly property var powerEntries: [
        { label: "Shutdown", key: "shutdown", detail: "Turn off your device" },
        { label: "Reboot", key: "reboot", detail: "Restart Zaman OS" },
        { label: "Sleep", key: "sleep", detail: "Coming soon", disabled: true }
    ]
    readonly property var entries: page === "notification-settings" ? notificationSettings : page === "settings" ? settingsEntries : page === "power" ? powerEntries : page === "storefronts" ? storefrontEntries : mainEntries

    FontLoader {
        id: menuFont
        source: Quickshell.env("ZAMAN_MENU_DEV_FONT") ? "file://" + Quickshell.env("ZAMAN_MENU_DEV_FONT") : ""
    }
    FontLoader {
        id: monoFont
        source: Quickshell.env("ZAMAN_MENU_DEV_MONO") ? "file://" + Quickshell.env("ZAMAN_MENU_DEV_MONO") : ""
    }

    // The page owns deferred layout work; destroyed delegates are never captured.
    Timer {
        id: focusSettle
        interval: 1
        repeat: false
        onTriggered: root.settleFocus()
    }
    function requestFocusSettle() { focusSettle.restart(); }
    function settleFocus() {
        if (!(menuOpen && listPage) || selected < 0 || selected >= menuList.count) return;
        menuList.forceLayout();
        menuList.positionViewAtIndex(selected, ListView.Contain);
    }

    function px(value) { return Math.round(value * uiScale); }
    function stopRepeat() {
        repeatDelay.stop();
        repeatStep.stop();
        horizontalDelay.stop();
        horizontalStep.stop();
        heldDirection = 0;
        heldHorizontal = 0;
    }
    function hold(direction, immediate) {
        if (closing) return;
        stopRepeat();
        if (direction === 0 || !menuOpen || !navigationPage) return;
        heldDirection = direction;
        if (immediate) move(direction);
        repeatDelay.restart();
    }
    function directionPressed(direction) {
        return direction < 0 ? (pressed.ui_up || pressed.key_up) : (pressed.ui_down || pressed.key_down);
    }
    function horizontalPressed(direction) {
        return direction < 0 ? (pressed.ui_left || pressed.key_left) : (pressed.ui_right || pressed.key_right);
    }
    function moveHorizontal(direction) {
        if (!editingKeyboard) return;
        if (page === "scrape") scrapePage.horizontal(direction);
        else if (page === "network") networkPageLoader.item.horizontal(direction);
        else bluetoothPageLoader.item.horizontal(direction);
    }
    function holdHorizontal(direction, immediate) {
        if (closing) return;
        stopRepeat();
        if (direction === 0 || !menuOpen || !editingKeyboard) return;
        heldHorizontal = direction;
        if (immediate) moveHorizontal(direction);
        horizontalDelay.restart();
    }

    Component.onCompleted: {
        // Manual restart avoids reloading an input lease during development.
        Quickshell.watchFiles = false;
        if (helperPath.length === 0) {
            console.error("Launch this prototype with its run.sh helper.");
            Qt.quit();
        } else {
            bridge.running = true;
            if (Quickshell.env("ZAMAN_MENU_DEV_OPEN_ON_START") === "1") openMenu();
        }
    }

    IpcHandler {
        target: "menu"
        function open(): string { return root.openMenu(); }
        function close(): string { root.closeMenu(); return root.menuStatus(); }
        function closeSession(key: string): string {
            if (key === root.menuInstance + ":" + root.menuGeneration && root.menuOpen && !root.exitBusy && !root.opening && !root.closing) root.closeMenu();
            return root.menuStatus();
        }
        function toggle(): string {
            if (root.exitBusy || root.opening || root.closing) return root.menuStatus();
            if (root.menuOpen) root.closeMenu();
            else root.openMenu();
            return root.menuStatus();
        }
        function status(): string { return root.menuStatus(); }
        function audioDiagnostics(): string {
            return JSON.stringify({ page: root.page, loaded: audioPageLoader.item !== null, loaderStatus: audioPageLoader.status,
                audio: audioPageLoader.item ? audioPageLoader.item.diagnostics() : null });
        }
        function timings(): string { return JSON.stringify({status:root.menuStatus(), helperIdle:root.helperIdle,
            readyMs:root.lastReadyMs, visibleMs:root.lastVisibleMs, firstFrameMs:root.firstFrameMs,
            slideFinishedMs:root.slideFinishedMs, entryPhase:root.entryPhase,
            entryProgress:panel.entryProgress, panelX:panel.x, panelWidth:panel.width,
            sessionKey:root.menuInstance + ":" + root.menuGeneration, generation:root.menuGeneration, inputOwned:root.menuOpen || root.opening || root.closing || !root.helperIdle}); }
    }

    function rememberTab() {
        tabLocations[activeTab] = {
            page:page, selected:selected, returnPage:returnPage, pageEntry:pageEntry,
            mainSelected:mainSelected, settingsSelected:settingsSelected, powerSelected:powerSelected,
            scroll:listPage ? menuList.contentY : 0,
            audio:page === "audio" && audioPageLoader.item ? audioPageLoader.item.location() : null
        };
    }
    function restoreTab(index) {
        activeTab = index;
        var location = tabLocations[index];
        mainSelected = location ? location.mainSelected : 0;
        settingsSelected = location ? location.settingsSelected : 0;
        powerSelected = location ? location.powerSelected : 0;
        returnPage = location ? location.returnPage : "";
        pageEntry = location ? location.pageEntry : ({label:"", key:"", detail:""});
        page = location ? location.page : index === 0 ? "" : index === 1 ? "notifications" : "settings";
        selected = location ? location.selected : 0;
        if (listPage) selected = Math.max(0, Math.min(entries.length - 1, selected));
        if (page === "audio" && audioPageLoader.item && location && location.audio)
            audioPageLoader.item.restoreLocation(location.audio);
        var revision = ++navigationRevision;
        var targetPage = page;
        var generation = menuGeneration;
        Qt.callLater(() => {
            if (menuOpen && activeTab === index && page === targetPage && navigationRevision === revision && menuGeneration === generation && listPage) {
                menuList.forceLayout();
                menuList.contentY = location ? location.scroll : 0;
            }
        });
    }
    function switchTab(direction) {
        if (!canSwitchTab || direction === 0) return;
        stopRepeat();
        if (audioPageLoader.item) audioPageLoader.item.stopHorizontal();
        keyboardCycleHeld = false;
        rememberTab();
        restoreTab((activeTab + direction % 3 + 3) % 3);
    }
    function notificationShortcut(clear) {
        if (!menuOpen || closing || exitBusy || editingKeyboard || (["transfer","scrape"].indexOf(page)<0 && !notifications.available)) return;
        stopRepeat();
        if (page === "scrape") {
            if (clear) scrapePage.clear();else scrapePage.dismiss();
        } else if (page === "transfer") {
            if (clear) transferPage.clear();
            else transferPage.dismiss();
        } else if (page === "notifications") {
            if (clear) notificationsPage.clear();
            else notificationsPage.dismiss();
        } else if (!clear && notifications.popups.length) {
            notifications.send({action:"dismiss-visible"});
        }
    }
    function select(index) {
        selected = (index + entries.length) % entries.length;
        menuList.positionViewAtIndex(selected, ListView.Contain);
    }
    function move(direction) {
        if (closing) return;
        if (menuOpen && page === "scrape") { scrapePage.move(direction); return; }
        if (menuOpen && page === "transfer") { transferPage.move(direction); return; }
        if (menuOpen && page === "transactions") { transactionActivity.move(direction); return; }
        if (menuOpen && page === "notifications") { notificationsPage.move(direction); return; }
        if (menuOpen && (page === "exit" || page === "reboot" || page === "shutdown" || page === "store-close")) { exitPage.move(direction); return; }
        if (menuOpen && page === "display") {
            displayPage.move(direction);
            return;
        }
        if (menuOpen && page === "bluetooth") {
            if (bluetoothPageLoader.item) bluetoothPageLoader.item.move(direction);
            return;
        }
        if (menuOpen && page === "network") {
            if (networkPageLoader.item) networkPageLoader.item.move(direction);
            return;
        }
        if (menuOpen && page === "controllers") {
            controllersPage.move(direction);
            return;
        }
        if (menuOpen && page === "audio") {
            if (audioPageLoader.item) audioPageLoader.item.move(direction);
            return;
        }
        if (!menuOpen || !listPage) return;
        select(selected + direction);
    }
    function activate() {
        if (!menuOpen || closing || exitBusy) return;
        if (page === "scrape") { stopRepeat();scrapePage.activate();return; }
        if (page === "transfer") { stopRepeat(); transferPage.activate(); return; }
        if (page === "transactions") { stopRepeat(); transactionActivity.activate(); return; }
        if (page === "notifications") { stopRepeat(); notificationsPage.activate(); return; }
        if (page === "notification-settings") {
            var preference = entries[selected].preference;
            notifications.send({action:"preferences", [preference]: !notifications.preferences[preference]});
            return;
        }
        if (page === "exit" || page === "reboot" || page === "shutdown" || page === "store-close") { stopRepeat(); exitPage.activate(); return; }
        if (page === "display") {
            stopRepeat(); displayPage.activate(); return;
        }
        if (page === "bluetooth") {
            stopRepeat();
            if (bluetoothPageLoader.item) bluetoothPageLoader.item.activate();
            else back();
            return;
        }
        if (page === "network") {
            stopRepeat();
            if (networkPageLoader.item) networkPageLoader.item.activate();
            else back();
            return;
        }
        if (page === "controllers") {
            stopRepeat();
            controllersPage.activate();
            return;
        }
        if (page === "audio") {
            stopRepeat();
            if (audioPageLoader.item) audioPageLoader.item.activate();
            else back();
            return;
        }
        if (!listPage) { back(); return; }
        stopRepeat();
        var entry = entries[selected];
        if (!entry || entry.disabled === true) return;
        if (entry.key === "store-dukkan" || entry.key === "store-steam") queueStoreAction("launch", entry.key.slice(6), "0");
        else if (entry.key === "store-home") queueStoreAction("home", storefrontId, storeGeneration);
        else if (entry.key === "store-close") { returnPage = page; pageEntry = entry; page = "store-close"; }
        else if (entry.key === "resume") closeMenu();
        else if (entry.key === "scrape") openScraping("","","");
        else if (entry.key === "settings" || entry.key === "power" || entry.key === "storefronts") {
            mainSelected = selected;
            pageEntry = entry;
            page = entry.key;
            select(page === "power" ? powerSelected : page === "storefronts" ? 0 : settingsSelected);
        } else {
            returnPage = page;
            if (page === "settings") settingsSelected = selected;
            else if (page === "power") powerSelected = selected;
            else mainSelected = selected;
            pageEntry = entry;
            if (entry.key === "exit" || entry.key === "reboot" || entry.key === "shutdown") exitNotice = "";
            if (entry.key === "bluetooth") bluetoothLoaded = true;
            page = entry.key;
        }
    }
    function leaveSettingsPage() {
        stopRepeat();
        page = "settings";
        select(settingsSelected);
    }
    function back() {
        if (exitBusy || closing) return;
        stopRepeat();
        if (page === "scrape") {
            if (!scrapePage.back()) {
                page=scrapeReturnLocation.page;returnPage=scrapeReturnLocation.returnPage;pageEntry=scrapeReturnLocation.entry;
                if (listPage) select(mainSelected);
            }
            return;
        }
        if (page === "notifications") {
            if (!notificationsPage.back()) closeMenu();
            return;
        }
        if (page === "transfer") {
            if (!transferPage.back()) { page = returnPage; if (listPage) select(mainSelected); }
            return;
        }
        if (page === "notification-settings") { leaveSettingsPage(); return; }
        if (page === "settings" && activeTab === 2) { closeMenu(); return; }
        if (page === "display") { displayPage.back(); return; }
        if (page === "bluetooth" && bluetoothPageLoader.item) {
            bluetoothPageLoader.item.back();
            return;
        }
        if (page === "network" && networkPageLoader.item) {
            networkPageLoader.item.back();
            return;
        }
        if (!listPage) {
            page = returnPage;
            select(page === "settings" ? settingsSelected : page === "power" ? powerSelected : mainSelected);
        } else if (page === "settings" || page === "power" || page === "storefronts") {
            if (page === "settings") settingsSelected = selected;
            else if (page === "power") powerSelected = selected;
            page = "";
            select(mainSelected);
        } else closeMenu();
    }
    function menuStatus() {
        if (exitBusy) return "busy";
        if (closing) return "closing";
        if (menuOpen) return "open";
        if (opening) return "opening";
        return "hidden";
    }
    function resetMenuSession() {
        resetEntry();
        menuOpen = false;
        opening = false;
        ready = false;
        closing = false;
        stopRepeat();
        pressed = ({});
        keyboardCycleHeld = false;
        controllerCount = 0;
        controllerDevices = [];
        controllerWarning = "";
        displaySupported = false;
        bluetoothLoaded = false;
        audioLoaded = false;
        networkLoaded = false;
        notificationsPage.detail = null;
        notificationsPage.confirming = false;
        restoreTab(activeTab);
    }
    function openMenu() {
        // Repeated open requests never toggle a visible menu or overlap leases.
        if (exitBusy) return "exiting-game";
        if (menuOpen || opening || closing) return menuStatus();
        resetMenuSession();
        menuGeneration += 1;
        opening = true;
        openStartedMs = Date.now();
        clockText = Qt.formatDateTime(new Date(), "h:mm AP");
        lastReadyMs = -1;
        lastVisibleMs = -1;
        firstFrameMs = -1;
        slideFinishedMs = -1;
        if (!bridge.running) {
            helperIdle = false;
            bridge.running = true;
        } else if (helperIdle) {
            helperIdle = false;
            bridge.write(JSON.stringify({action: "open"}) + "\n");
        }
        return "opening";
    }
    function hideMenu() {
        if (menuOpen) {
            rememberTab();
            // A new opening must not revive an old destructive confirmation.
            if (["exit", "reboot", "shutdown", "store-close"].indexOf(page) >= 0) {
                tabLocations[activeTab].page = returnPage;
            }
        }
        if ((menuOpen || opening) && bluetoothPageLoader.item) bluetoothPageLoader.item.shutdown();
        menuOpen = false;
        resetEntry();
        opening = false;
        stopRepeat();
        pressed = ({});
        keyboardCycleHeld = false;
    }
    function resetEntry() {
        entryPhase = "hidden";
        slideIn.stop();
        slideOut.stop();
        closeDeadline.stop();
        panel.entryProgress = 0;
    }
    function startEntryFrame() {
        if (!menuOpen || closing || entryPhase !== "await-frame" || !overlay.backingWindowVisible
                || overlay.width <= 0 || overlay.height <= 0 || panel.width <= 0 || panel.height <= 0) return;
        // This frame was queued with progress=0. Only now start moving the panel.
        firstFrameMs = Date.now() - openStartedMs;
        entryPhase = "sliding";
        slideIn.restart();
        bridge.write(JSON.stringify({action: "opened"}) + "\n");
    }
    function closeMenu(completed) {
        if (completed === true) {
            if (!closing || entryPhase !== "closing") return;
            var origin = closeOrigin;
            entryPhase = "hidden";
            hideMenu();
            // Unmap the exclusive layer before the helper restores input.
            if (bridge.running) bridge.write(JSON.stringify({action:"close",page:origin.page,tab:origin.tab}) + "\n");
            else resetMenuSession();
            return;
        }
        if ((exitBusy && completed !== "exit-complete") || closing || !(menuOpen || opening)) return;
        closeOrigin = ({page:page,tab:activeTab});
        closing = true;
        stopRepeat(); pressed = ({}); keyboardCycleHeld = false;
        slideIn.stop();
        entryPhase = "closing";
        if (menuOpen && overlay.backingWindowVisible && panel.entryProgress > 0 && bridge.running) {
            slideOut.from = panel.entryProgress;
            slideOut.duration = Math.max(1,Math.round(slideIn.duration * panel.entryProgress));
            closeDeadline.restart();
            slideOut.restart();
        } else closeMenu(true);
    }
    function input(event, value) {
        if (!menuOpen || closing || typeof value !== "number" || !isFinite(value)) return;
        var down = value > 0.5;
        var wasDown = pressed[event] === true;
        var next = Object.assign({}, pressed);
        next[event] = down;
        pressed = next;
        if (event === "ui_l1" || event === "ui_r1") {
            if (down && !wasDown) switchTab(event === "ui_l1" ? -1 : 1);
            return;
        }
        if (event === "ui_context" || event === "ui_action") {
            if (down && !wasDown && editingKeyboard && !keyboardCycleHeld) {
                keyboardCycleHeld = true;
                var keyboardPage = page === "scrape" ? scrapePage : page === "network" ? networkPageLoader.item : bluetoothPageLoader.item;
                if (keyboardPage) keyboardPage.cycleKeyboard();
            } else if (down && !wasDown && !editingKeyboard) notificationShortcut(event === "ui_action");
            else if (!down && !pressed.ui_context && !pressed.ui_action) keyboardCycleHeld = false;
            return;
        }
        if (page === "display" && (event === "ui_left" || event === "ui_right" || event === "key_left" || event === "key_right")) {
            if (down && !wasDown) displayPage.horizontal(event === "ui_left" || event === "key_left" ? -1 : 1);
        } else if ((page === "bluetooth" || page === "network" || page === "scrape" && editingKeyboard) && (event === "ui_left" || event === "ui_right" || event === "key_left" || event === "key_right")) {
            var horizontal = event === "ui_left" || event === "key_left" ? -1 : 1;
            if (editingKeyboard) {
                if (down && !wasDown) holdHorizontal(horizontal, true);
                else if (!down) {
                    var held = heldHorizontal !== 0 && horizontalPressed(heldHorizontal) ? heldHorizontal
                        : (horizontalPressed(-1) ? -1 : (horizontalPressed(1) ? 1 : 0));
                    if (held !== heldHorizontal) holdHorizontal(held, false);
                }
            } else if (down && !wasDown) {
                var item = page === "bluetooth" ? bluetoothPageLoader.item : networkPageLoader.item;
                if (item) item.horizontal(horizontal);
                else if (horizontal < 0) back();
            }
        } else if (page === "audio" && (event === "ui_left" || event === "ui_right" || event === "key_left" || event === "key_right")) {
            if (audioPageLoader.item) audioPageLoader.item.horizontal(event, value);
            else if (down && !wasDown && (event === "ui_left" || event === "key_left")) back();
        } else if (event === "ui_up" || event === "ui_down" || event === "key_up" || event === "key_down") {
            if (down && !wasDown) {
                hold(event === "ui_up" || event === "key_up" ? -1 : 1, true);
            } else if (!down) {
                var remaining = heldDirection !== 0 && directionPressed(heldDirection) ? heldDirection
                    : (directionPressed(-1) ? -1 : (directionPressed(1) ? 1 : 0));
                if (remaining !== heldDirection) hold(remaining, false);
            }
        } else if (down && !wasDown) {
            if (event === "ui_accept" || event === "ui_right" || event === "key_right") activate();
            else if (event === "ui_back" || event === "ui_cancel" || event === "ui_left" || event === "key_left") back();
        }
    }
    function queueStoreAction(action, id, generation, target) {
        if (exitBusy || pendingStoreAction !== null || closing || !menuOpen || sessionActionPath.length === 0) return;
        pendingStoreAction = ({action:action, id:id, generation:generation, target:target || ""});
        closeMenu();
    }
    function openTransaction(jobId) {
        if (!/^[a-f0-9]{32}$/.test(jobId)) return;
        if (foreground === "game" || storeGame) {
            storeNotice = "Return to the library before opening Dukkan activity.";
            return;
        }
        queueStoreAction("launch", "dukkan", "0", jobId);
    }
    Process {
        id: transactionPoll
        command: ["/usr/bin/python3", root.transactionClient, "snapshot"]
        property bool received: false
        onStarted: received = false
        stdout: SplitParser {
            onRead: line => {
                try {
                    var data = JSON.parse(line);
                    if (data.type === "transactions") {
                        transactionPoll.received = true;
                        root.transactionsAvailable = true;
                        root.transactionJobs = data.jobs || [];
                    }
                } catch (error) {}
            }
        }
        onExited: (code, status) => { if (!received) root.transactionsAvailable = false; }
    }
    Timer {
        interval: 1500; repeat: true
        running: root.menuOpen && root.transactionClient.length > 0
        triggeredOnStart: true
        onTriggered: if (!transactionPoll.running) transactionPoll.running = true
    }
    function receiveStoreResult(line) {
        var result;
        try { result = JSON.parse(line); } catch (error) { return; }
        if (result.type !== "store-result") return;
        storeResultReceived = true;
        storeNotice = result.ok ? "" : result.message;
    }
    function requestExitGame() {
        if (page === "store-close") { queueStoreAction("close", storefrontId, storeGeneration); return; }
        var power = page === "reboot" || page === "shutdown";
        if (exitBusy || closing || !menuOpen || (!power && (page !== "exit" || foreground !== "game"))) return;
        if (sessionActionPath.length === 0) {
            exitNotice = "The menu action helper is unavailable. Restart the menu service after updating.";
            return;
        }
        stopRepeat();
        sessionAction = power ? page : "exit-game";
        exitBusy = true;
        exitResultReceived = false;
        exitNotice = "";
        exitAction.running = true;
    }
    function receiveExitResult(line) {
        var message;
        try { message = JSON.parse(line); } catch (error) { return; }
        if (message.type === "exit-progress" || message.type === "power-progress") {
            exitNotice = message.message || "Closing game…";
        } else if (message.type === "exit-result" && exitBusy) {
            exitResultReceived = true;
            exitNotice = message.ok ? "" : (message.message || "Unable to complete the session action.");
            // Keep duplicate requests blocked until the process exits. This also
            // prevents a second running=true while its first result is draining.
            if (message.ok) {
                foreground = "library";
                if (menuOpen || opening) closeMenu("exit-complete");
            }
        }
    }
    function finishExitAction(exitCode) {
        if (!exitResultReceived) exitNotice = "The session action stopped before reporting completion.";
        exitBusy = false;
        if (exitNotice.length > 0) console.warn(exitNotice);
    }

    Process {
        id: storeAction
        command: ["/usr/bin/python3", root.sessionActionPath, "storefront", root.storeRequest.action, root.storeRequest.id, root.storeRequest.generation].concat(root.storeRequest.target ? [root.storeRequest.target] : [])
        stdout: SplitParser { onRead: data => root.receiveStoreResult(data) }
        stderr: SplitParser { onRead: data => console.warn(data) }
        onExited: (exitCode, exitStatus) => {
            root.exitBusy = false;
            if (!root.storeResultReceived) root.storeNotice = "The storefront request stopped before reporting completion.";
            if (root.storeNotice.length) root.openMenu();
        }
    }

    Process {
        id: exitAction
        command: ["/usr/bin/python3", root.sessionActionPath, root.sessionAction]
        stdout: SplitParser { onRead: data => root.receiveExitResult(data) }
        stderr: SplitParser { onRead: data => console.warn(data) }
        onExited: (exitCode, exitStatus) => root.finishExitAction(exitCode)
    }

    function receive(line) {
        if (!line.startsWith("{")) return;
        var message;
        try { message = JSON.parse(line); }
        catch (error) { console.error("Invalid preview message: " + line); return; }
        if (message.type === "helper-idle") {
            if (!bridge.running || message.protocol !== 1) return;
            if (menuOpen && !closing) return;
            // Idle is acknowledged only after the helper restores its lease.
            helperIdle = true;
            if (opening && !closing) {
                helperIdle = false;
                bridge.write(JSON.stringify({action: "open"}) + "\n");
            } else {
                resetMenuSession();
                if (pendingStoreAction !== null) {
                    storeRequest = pendingStoreAction;
                    pendingStoreAction = null;
                    exitBusy = true;
                    storeResultReceived = false;
                    storeNotice = "";
                    storeAction.running = true;
                }
            }
        } else if (message.type === "ready") {
            // A close during acquisition must never be undone by late readiness.
            if (!opening || closing || !bridge.running) return;
            displaySupported = message.display === true;
            foreground = message.foreground;
            storefrontId = message.storefront || "";
            storeGeneration = message.storeGeneration || "0";
            storeGame = message.storeGame === true;
            controllerCount = message.controllers;
            controllerDevices = message.devices || [];
            ready = true;
            opening = false;
            resetEntry();
            entryPhase = "await-frame";
            menuOpen = true;
            restoreTab(activeTab);
            lastReadyMs = Date.now() - openStartedMs;
            console.info("Resident menu input ready after " + lastReadyMs + " ms");
        } else if (message.type === "controllers") {
            stopRepeat();
            controllerCount = message.controllers;
            controllerDevices = message.devices || [];
        } else if (message.type === "input-reset") {
            stopRepeat(); pressed = ({}); keyboardCycleHeld = false;
            if (audioPageLoader.item) {
                audioPageLoader.item.stopHorizontal();
                audioPageLoader.item.leftDown = false;
                audioPageLoader.item.rightDown = false;
            }
        } else if (message.type === "display") displayPage.receive(message);
        else if (message.type === "input") input(message.event, message.value);
        else if (message.type === "warning") {
            controllerWarning = "Controller input interrupted. Reconnect and press a button, or use the keyboard.";
            console.warn(message.message);
        } else if (message.type === "controller-recovered") controllerWarning = "";
        else if (message.type === "closing") {
            closing = true;
            hideMenu();
        } else if (message.type === "closed") {
            // Wait for helper-idle before allowing a new input lease.
            closing = true;
            hideMenu();
        }
        else if (message.type === "error") {
            console.error(message.message);
            closing = true;
            hideMenu();
        }
    }

    Process {
        id: bridge
        command: [root.helperPath, "--resident"]
        stdinEnabled: true
        stdout: SplitParser { onRead: data => root.receive(data) }
        stderr: SplitParser { onRead: data => console.warn(data) }
        onExited: (exitCode, exitStatus) => {
            root.hideMenu();
            if (root.pendingStoreAction !== null) {
                root.pendingStoreAction = null;
                root.storeNotice = "The input helper stopped before restoring the menu lease. Reopen the menu to try again.";
            }
            root.helperIdle = false;
            if (exitCode !== 0) console.error("Preview helper exited with code " + exitCode);
            root.resetMenuSession();
        }
    }
    Timer {
        interval: 2000
        running: !bridge.running
        repeat: true
        onTriggered: {
            root.helperIdle = false;
            bridge.running = true;
        }
    }
    Timer {
        interval: 12000
        running: root.opening && !root.ready
        onTriggered: {
            console.error("Preview helper did not become ready.");
            root.closeMenu();
        }
    }
    Timer {
        interval: 1000
        running: root.menuOpen
        repeat: true
        onTriggered: bridge.write(JSON.stringify({action: "heartbeat"}) + "\n")
    }
    Timer {
        interval: 30000
        running: root.menuOpen
        repeat: true
        onTriggered: root.clockText = Qt.formatDateTime(new Date(), "h:mm AP")
    }
    Timer {
        id: repeatDelay
        interval: 220
        onTriggered: {
            if (root.heldDirection !== 0 && root.menuOpen && root.navigationPage) {
                root.move(root.heldDirection);
                repeatStep.start();
            }
        }
    }
    Timer {
        id: repeatStep
        interval: 70
        repeat: true
        onTriggered: {
            if (root.heldDirection === 0 || !root.menuOpen || !root.navigationPage) stop();
            else root.move(root.heldDirection);
        }
    }
    Timer {
        id: horizontalDelay
        interval: 220
        onTriggered: {
            if (root.heldHorizontal !== 0 && root.menuOpen && root.editingKeyboard && root.horizontalPressed(root.heldHorizontal)) {
                root.moveHorizontal(root.heldHorizontal);
                horizontalStep.start();
            }
        }
    }
    Timer {
        id: horizontalStep
        interval: 70
        repeat: true
        onTriggered: {
            if (root.heldHorizontal === 0 || !root.menuOpen || !root.editingKeyboard || !root.horizontalPressed(root.heldHorizontal)) root.stopRepeat();
            else root.moveHorizontal(root.heldHorizontal);
        }
    }

    PanelWindow {
        id: overlay
        visible: root.menuOpen
        anchors { top: true; bottom: true; left: true; right: true }
        exclusionMode: ExclusionMode.Ignore
        color: "transparent"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "zaman-menu-dev"
        WlrLayershell.keyboardFocus: root.menuOpen ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None
        onBackingWindowVisibleChanged: {
            if (backingWindowVisible && root.menuOpen) {
                root.lastVisibleMs = Date.now() - root.openStartedMs;
                console.info("Resident menu visible after " + root.lastVisibleMs + " ms");
                Qt.callLater(() => inputScope.forceActiveFocus());
            } else if (!backingWindowVisible) {
                if (root.closing && root.entryPhase === "closing") root.closeMenu(true);
                else root.resetEntry();
            }
        }
        onClosed: root.closeMenu()

        Connections {
            target: panel.Window.window
            function onFrameSwapped() { root.startEntryFrame(); }
        }

        NumberAnimation {
            id: slideIn
            target: panel
            property: "entryProgress"
            from: 0
            to: 1
            duration: 180
            easing.type: Easing.OutCubic
            onFinished: {
                if (root.menuOpen && root.entryPhase === "sliding") {
                    root.entryPhase = "settled";
                    root.slideFinishedMs = Date.now() - root.openStartedMs;
                }
            }
        }

        NumberAnimation {
            id: slideOut
            target: panel
            property: "entryProgress"
            to: 0
            duration: slideIn.duration
            easing.type: Easing.InCubic
            onFinished: root.closeMenu(true)
        }
        Timer {
            id: closeDeadline
            interval: 600
            onTriggered: root.closeMenu(true)
        }

        Rectangle {
            anchors.fill: parent
            opacity: panel.entryProgress
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0.0; color: "#89080C13" }
                GradientStop { position: 0.55; color: "#57080B11" }
                GradientStop { position: 1.0; color: "#31080A0F" }
            }
        }
        MouseArea { anchors.fill: parent; onClicked: root.back() }

        // Two small static shadow surfaces; navigation has no animation delay.
        Rectangle {
            x: panel.x + root.px(5); y: panel.y + root.px(12)
            width: panel.width; height: panel.height
            radius: panel.radius
            color: "#35000000"
        }
        Rectangle {
            x: panel.x + root.px(2); y: panel.y + root.px(5)
            width: panel.width; height: panel.height
            radius: panel.radius
            color: "#25000000"
        }
        Rectangle {
            id: panel
            property real entryProgress: 0
            readonly property real offscreenX: -width - root.px(8)
            x: offscreenX + (root.px(28) - offscreenX) * entryProgress
            y: root.px(26)
            width: Math.min(Math.round(parent.width * 0.44), root.px(700))
            height: parent.height - root.px(52)
            radius: root.px(24)
            border.width: 1
            border.color: "#405D718C"
            // Keep the surface almost opaque so background titles do not
            // compete with menu text; retain the restrained blue gradient.
            gradient: Gradient {
                GradientStop { position: 0.0; color: "#FE171D28" }
                GradientStop { position: 0.45; color: "#FE12161E" }
                GradientStop { position: 1.0; color: "#FE17191F" }
            }
            readonly property real padding: root.px(32)
            MouseArea { anchors.fill: parent; onClicked: mouse => mouse.accepted = true }

            Column {
                id: heading
                anchors {
                    left: parent.left; right: parent.right; top: parent.top
                    margins: panel.padding
                }
                spacing: root.px(12)

                Item {
                    width: parent.width
                    height: root.px(52)
                    ZamanMark {
                        id: brandMark
                        part: "bird"
                        width: root.px(50); height: root.px(48)
                        y: root.px(2)
                        ink: "#EEE1CF"
                    }
                    Column {
                        anchors { left: brandMark.right; leftMargin: root.px(16) }
                        y: root.px(3)
                        spacing: root.px(7)
                        Row {
                            spacing: root.px(10)
                            ZamanMark {
                                part: "wordmark"
                                width: root.px(166); height: root.px(31)
                                ink: "#EEE1CF"
                            }
                            MenuText {
                                text: "OS"
                                font.pixelSize: root.px(13)
                                color: "#C3CEDB"
                                y: root.px(16)
                            }
                        }
                        MenuText {
                            text: "PLAY  EXPLORE  BELONG"
                            color: "#BBC6D3"
                            font.family: root.monoFontName
                            font.pixelSize: root.px(9)
                            font.letterSpacing: 1
                        }
                    }
                    Row {
                        anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                        spacing: root.px(12)
                        MenuIcon {
                            visible: root.controllerCount > 0
                            name: "controllers"; ink: "#9CAFC5"
                            width: root.px(22); height: width
                        }
                        MenuText {
                            text: root.clockText
                            color: "#C7CFD8"
                            font.family: root.monoFontName
                            font.pixelSize: root.px(13)
                            y: root.px(3)
                        }
                    }
                }
                MenuTabBar {
                    width: parent.width
                    height: implicitHeight
                    uiScale: root.uiScale
                    currentIndex: root.activeTab
                    unread: notifications.unread
                    controllerAvailable: root.controllerCount > 0
                    enabled: root.canSwitchTab
                    animate: root.menuOpen && root.entryPhase === "settled"
                    onStepRequested: direction => root.switchTab(direction)
                    onTabRequested: index => root.switchTab(index - root.activeTab)
                }
                Rectangle { width: parent.width; height: 1; color: "#265C6E85" }
                MenuText {
                    text: root.page === "" ? "Home" : root.page === "transactions" ? "Activity" : root.page === "notifications" ? "Notifications" : root.page === "notification-settings" ? "Notification preferences" : (root.page === "settings" ? "Settings" : root.page === "power" ? "Power" : root.pageEntry.label)
                    font.pixelSize: root.px(29)
                    font.weight: Font.DemiBold
                    color: "#F0E5D6"
                }
                Row {
                    spacing: root.px(9)
                    Rectangle {
                        width: root.px(6); height: width; radius: width / 2
                        y: root.px(7)
                        color: "#88A8C9"
                    }
                    MenuText {
                        text: root.page === "transfer" ? "A selects · B returns · Receiving continues when this view closes"
                            : root.page === "transactions" ? (root.storeNotice || "A opens the selected transaction in Dukkan · B goes back")
                            : root.page === "notifications" ? (root.storeNotice || notifications.error || (notifications.preferences.dnd ? "Do Not Disturb is on." : "Your recent system notifications."))
                            : root.page === "notification-settings" ? "Popup preferences and Steam notices."
                            : root.page === "settings" ? "Devices, connections and personalization."
                            : root.page === "power" ? "Shutdown and reboot require confirmation."
                            : (root.storeNotice.length ? root.storeNotice : root.storeGame ? "Resume your game or close Steam and its game." : root.storefrontId.length ? "Your storefront is running." : root.foreground === "game" ? "Your game is still running." : "Your library is waiting.")
                        color: "#BBC6D3"
                        font.pixelSize: root.px(15)
                    }
                }
                MenuText {
                    visible: root.controllerWarning.length > 0
                    width: parent.width
                    text: root.controllerWarning
                    wrapMode: Text.WordWrap
                    color: "#E8C08C"
                    font.pixelSize: root.px(15)
                }
            }

            ListView {
                id: menuList
                property point lastPointerPosition: Qt.point(-100000, -100000)
                visible: root.listPage
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: root.px(18); rightMargin: root.px(18)
                    topMargin: root.px(18); bottomMargin: root.px(18)
                }
                model: root.entries
                currentIndex: root.selected
                spacing: root.px(4)
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                delegate: MenuFocusRow {
                    id: menuRow
                    required property var modelData
                    required property int index
                    chosen: root.selected === index
                    readonly property bool storefrontMark: modelData.key === "store-dukkan" || modelData.key === "store-steam" || (modelData.key === "resume" && root.storefrontId.length > 0)
                    readonly property bool dukkanMark: modelData.key === "store-dukkan" || (modelData.key === "resume" && root.storefrontId === "dukkan")
                    uiScale: root.uiScale
                    width: menuList.width
                    baseHeight: 66
                    onFocusSettled: if (menuRow.chosen) root.requestFocusSettle()
                    MenuFocusBackground {
                        chosen: menuRow.chosen
                        uiScale: root.uiScale
                        emphasis: menuRow.focusAmount
                    }
                    Rectangle {
                        x: root.px(14); y: Math.round((parent.height - height) / 2)
                        width: root.px(menuRow.storefrontMark ? 104 : 42 + 4 * menuRow.focusAmount)
                        height: root.px(42 + 4 * menuRow.focusAmount)
                        radius: root.px(12)
                        color: menuRow.chosen ? "#16291D10" : "#112F425C"
                        MenuIcon {
                            anchors.centerIn: parent
                            name: menuRow.modelData.key === "resume" && root.storefrontId.length ? "store-" + root.storefrontId : menuRow.modelData.key === "resume" && root.foreground !== "game" ? "library" : menuRow.modelData.key
                            ink: menuRow.chosen ? "#30271E" : "#DEC09A"
                            width: root.px(menuRow.dukkanMark ? 98 + 4 * menuRow.focusAmount : 30 + 2 * menuRow.focusAmount)
                            height: menuRow.dukkanMark ? root.px(39 + 2 * menuRow.focusAmount) : width
                        }
                    }
                    Column {
                        x: root.px(menuRow.storefrontMark ? 134 : 72)
                        y: Math.round((parent.height - height) / 2)
                        width: parent.width - x - root.px(88)
                        spacing: root.px(2)
                        MenuText {
                            width: parent.width
                            text: menuRow.modelData.label
                            color: menuRow.modelData.disabled === true ? (menuRow.chosen ? "#78634D" : "#7F8A98") : menuRow.chosen ? "#211E1B" : "#ECEEF1"
                            font.pixelSize: root.px(22 + 2 * menuRow.focusAmount)
                            elide: Text.ElideRight
                        }
                        MenuText {
                            width: parent.width
                            text: menuRow.modelData.detail
                            color: menuRow.chosen ? "#473627" : "#BBC6D3"
                            font.pixelSize: root.px(14)
                            elide: Text.ElideRight
                        }
                    }
                    Rectangle {
                        visible: menuRow.modelData.disabled === true || (menuRow.modelData.key === "appearance" && menuRow.chosen)
                        anchors { right: chevron.left; rightMargin: root.px(10) }
                        y: Math.round((parent.height - height) / 2)
                        width: root.px(45); height: root.px(21); radius: root.px(7)
                        color: "#19281C12"
                        MenuText {
                            anchors.centerIn: parent
                            text: "Soon"; font.pixelSize: root.px(10); color: "#473627"
                        }
                    }
                    MenuIcon {
                        id: chevron
                        visible: menuRow.modelData.disabled !== true
                        name: "chevron"
                        anchors { right: parent.right; rightMargin: root.px(16) }
                        y: Math.round((parent.height - height) / 2)
                        width: root.px(17); height: root.px(22)
                        ink: menuRow.chosen ? "#614930" : "#7D899B"
                    }
                    MouseArea {
                        id: menuPointer
                        anchors.fill: parent
                        hoverEnabled: true
                        onEntered: if (menuRow.pointerMoved(menuPointer)) root.select(index)
                        onPositionChanged: if (menuRow.pointerMoved(menuPointer)) root.select(index)
                        onClicked: { root.select(index); root.activate(); }
                    }
                }
            }

            TransferPage {
                id: transferPage
                visible: root.page === "transfer"
                snapshot: root.transferSnapshot
                available: root.transferAvailable
                busy: transferAction.running
                error: root.transferError
                uiScale: root.uiScale
                onOpenScraping:(importId,system,jobId)=>root.openScraping(importId,system,jobId)
                anchors {
                    left:parent.left; right:parent.right; top:heading.bottom; bottom:footer.top
                    leftMargin:panel.padding; rightMargin:panel.padding
                    topMargin:root.px(22); bottomMargin:root.px(18)
                }
                onCommand: request => {
                    if (!transferAction.running && root.transferClient.length > 0) {
                        root.transferRequest = request;
                        transferAction.running = true;
                    }
                }
            }
            ScrapePage {
                id:scrapePage
                visible:root.page === "scrape"
                snapshot:root.transferSnapshot.scraper || ({available:false,folders:[],jobs:[]})
                available:root.transferAvailable;busy:transferAction.running;error:root.transferError;uiScale:root.uiScale
                onCloseRequested:root.closeMenu()
                anchors { left:parent.left;right:parent.right;top:heading.bottom;bottom:footer.top
                    leftMargin:panel.padding;rightMargin:panel.padding;topMargin:root.px(22);bottomMargin:root.px(18) }
                onCommand:request=>{
                    if (!transferAction.running && root.transferClient.length>0) { root.transferRequest=request;transferAction.running=true; }
                }
            }
            TransactionActivityPage {
                id: transactionActivity
                visible: root.page === "transactions"
                jobs: root.transactionJobs
                available: root.transactionsAvailable
                uiScale: root.uiScale
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(22); bottomMargin: root.px(18)
                }
                onOpenTransaction: jobId => root.openTransaction(jobId)
            }
            NotificationsPage {
                id: notificationsPage
                client: notifications
                imports: root.transferSnapshot.jobs || []
                scrapes:root.transferSnapshot.scraper ? root.transferSnapshot.scraper.jobs || [] : []
                onOpenScrape:jobId=>root.openScraping("","",jobId)
                onOpenTransfer: root.openTransfers()
                transactions: root.transactionsAvailable ? root.transactionJobs : []
                onOpenTransaction: jobId => root.openTransaction(jobId)
                visible: root.page === "notifications"
                uiScale: root.uiScale
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(22); bottomMargin: root.px(18)
                }
            }
            DisplayPage {
                id: displayPage
                visible: root.page === "display"
                active: root.page === "display" && root.menuOpen
                helperReady: root.displaySupported
                uiScale: root.uiScale
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(22); bottomMargin: root.px(18)
                }
                onCommand: request => { if (bridge.running) bridge.write(JSON.stringify(request) + "\n"); }
                onBackRequested: root.leaveSettingsPage()
                onNavigationReset: root.stopRepeat()
            }
            // Keep Bluetooth alive while the menu is open to finish scan cleanup.
            Loader {
                id: bluetoothPageLoader
                visible: root.page === "bluetooth"
                active: root.menuOpen && (root.bluetoothLoaded || root.page === "bluetooth")
                source: active ? Qt.resolvedUrl("BluetoothPage.qml") : ""
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(22); bottomMargin: root.px(18)
                }
                onLoaded: { root.bluetoothLoaded = true; item.uiScale = Qt.binding(() => root.uiScale); }
            }
            Connections {
                target: bluetoothPageLoader.item
                function onBackRequested() { root.leaveSettingsPage(); }
                function onNavigationReset() { root.stopRepeat(); }
            }
            // Visited native pages retain their views while this menu lease is open.
            Loader {
                id: networkPageLoader
                visible: root.page === "network"
                active: root.menuOpen && (root.networkLoaded || root.page === "network")
                source: active ? Qt.resolvedUrl("NetworkPage.qml") : ""
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(22); bottomMargin: root.px(18)
                }
                onLoaded: { root.networkLoaded = true; item.uiScale = Qt.binding(() => root.uiScale); }
            }
            Connections {
                target: networkPageLoader.item
                function onBackRequested() { root.leaveSettingsPage(); }
                function onNavigationReset() { root.stopRepeat(); }
            }
            ControllersPage {
                id: controllersPage
                visible: root.page === "controllers"
                uiScale: root.uiScale
                devices: root.controllerDevices
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(22); bottomMargin: root.px(18)
                }
                onBackRequested: root.back()
            }
            Loader {
                id: audioPageLoader
                visible: root.page === "audio"
                active: root.menuOpen && (root.audioLoaded || root.page === "audio")
                source: active ? Qt.resolvedUrl("AudioPage.qml") : ""
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(22); bottomMargin: root.px(18)
                }
                onLoaded: {
                    root.audioLoaded = true;
                    item.uiScale = Qt.binding(() => root.uiScale);
                    item.active = Qt.binding(() => root.menuOpen && root.page === "audio");
                    var location = root.tabLocations[root.activeTab];
                    if (root.page === "audio" && location && location.audio) item.restoreLocation(location.audio);
                }
            }
            Connections {
                target: audioPageLoader.item
                function onBackRequested() { root.back(); }
            }
            Column {
                visible: (root.page === "audio" && audioPageLoader.status === Loader.Error)
                    || (root.page === "network" && networkPageLoader.status === Loader.Error)
                    || (root.page === "bluetooth" && bluetoothPageLoader.status === Loader.Error)
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom
                    leftMargin: panel.padding; rightMargin: panel.padding; topMargin: root.px(32)
                }
                spacing: root.px(20)
                MenuText {
                    width: parent.width; wrapMode: Text.WordWrap
                    text: root.page === "bluetooth" ? "Bluetooth is unavailable." : root.page === "network" ? "Network settings are unavailable." : "Audio is unavailable."
                    font.pixelSize: root.px(24)
                }
                MenuText {
                    width: parent.width; wrapMode: Text.WordWrap
                    text: root.page === "audio" ? "Press B or Escape to return to the main menu." : "Press B or Escape to return to Settings."
                    color: "#BBC6D3"; font.pixelSize: root.px(17)
                }
                Rectangle {
                    width: parent.width; height: root.px(56); radius: root.px(13)
                    color: "#EAC28D"
                    MenuText {
                        anchors.centerIn: parent
                        text: "Back"; color: "#29221C"; font.pixelSize: root.px(20)
                    }
                    MouseArea { anchors.fill: parent; onClicked: root.back() }
                }
            }

            ExitGamePage {
                id: exitPage
                visible: root.page === "exit" || root.page === "reboot" || root.page === "shutdown" || root.page === "store-close"
                active: root.menuOpen && visible
                prompt: root.page === "reboot" ? "Restart Zaman?" : root.page === "shutdown" ? "Shut down Zaman?" : root.page === "store-close" ? "Close " + (root.storefrontId === "steam" ? "Steam" : "Dukkan") + "?" : "Exit this game?"
                detail: root.page === "reboot" ? "Close any running game and restart Zaman." : root.page === "shutdown" ? "Close any running game and turn off Zaman." : root.page === "store-close" ? (root.storeGame ? "Close Steam and its running game, then return to your library. Unsaved progress may be lost. Downloads may stop." : "Close the storefront and return to your library. Downloads may stop.") : "Close the current game and return to your library."
                cancelLabel: root.page === "exit" ? "Keep playing" : "Cancel"
                confirmLabel: root.page === "reboot" ? "Reboot" : root.page === "shutdown" ? "Shutdown" : root.page === "store-close" ? "Close " + (root.storefrontId === "steam" ? "Steam" : "Dukkan") : "Exit Game"
                busyHeading: root.sessionAction === "reboot" ? "Restarting Zaman…" : root.sessionAction === "shutdown" ? "Shutting down Zaman…" : "Closing game…"
                busy: root.exitBusy
                notice: root.exitNotice
                uiScale: root.uiScale
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom; bottom: footer.top
                    leftMargin: panel.padding; rightMargin: panel.padding
                    topMargin: root.px(32); bottomMargin: root.px(18)
                }
                onConfirmRequested: root.requestExitGame()
                onBackRequested: root.back()
            }

            Column {
                visible: root.page === "appearance"
                anchors {
                    left: parent.left; right: parent.right; top: heading.bottom
                    leftMargin: panel.padding; rightMargin: panel.padding; topMargin: root.px(32)
                }
                spacing: root.px(20)
                Rectangle {
                    width: root.px(72); height: width; radius: root.px(20)
                    color: "#1E3C5579"; border.width: 1; border.color: "#305F7B9A"
                    MenuIcon {
                        anchors.centerIn: parent
                        name: root.pageEntry.key
                        ink: "#DAB991"
                        width: root.px(38); height: width
                    }
                }
                MenuText {
                    text: root.pageEntry.detail
                    width: parent.width; wrapMode: Text.WordWrap
                    font.pixelSize: root.px(24)
                }
                MenuText {
                    text: root.pageEntry.key === "appearance"
                        ? "Customize your system's appearance in a future update."
                        : "These controls are coming next."
                    color: "#BBC6D3"; width: parent.width; wrapMode: Text.WordWrap
                    font.pixelSize: root.px(17)
                }
                Rectangle {
                    width: parent.width; height: root.px(56); radius: root.px(13)
                    border.width: 1; border.color: "#E8C498"
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0.0; color: "#EAC28D" }
                        GradientStop { position: 1.0; color: "#CB986C" }
                    }
                    MenuText {
                        x: Math.round((parent.width - width) / 2)
                        y: Math.round((parent.height - height) / 2)
                        text: "Back"; color: "#29221C"; font.pixelSize: root.px(20)
                    }
                    MouseArea { anchors.fill: parent; onClicked: root.back() }
                }
            }

            Column {
                id: footer
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom; margins: panel.padding }
                spacing: root.px(12)
                Rectangle { width: parent.width; height: 1; color: "#265C6E85" }
                Flow {
                    width: parent.width
                    spacing: root.px(16)
                    id: navigationGuide
                    readonly property bool adjusting: (root.page === "display" && displayPage.adjustable)
                        || (root.page === "audio" && audioPageLoader.item && audioPageLoader.item.adjustable)
                    readonly property bool textEntry: (root.page === "network" && networkPageLoader.item && networkPageLoader.item.editingPassword)
                        || (root.page === "bluetooth" && bluetoothPageLoader.item && bluetoothPageLoader.item.editingCode)
                        || (root.page === "scrape" && scrapePage.editingText)
                    Repeater {
                        model: [
                            {directional: true, show: root.page !== "notifications" || notificationsPage.hasItems, label: navigationGuide.adjusting ? "Adjust" : navigationGuide.textEntry ? "Move" : "Navigate", key: navigationGuide.adjusting ? "←→" : navigationGuide.textEntry ? "↑↓←→" : "↑↓"},
                            {faces: ["south"], show: (root.page !== "notifications" || notificationsPage.hasItems) && (!navigationGuide.adjusting || (root.page === "audio" && audioPageLoader.item && audioPageLoader.item.rows[audioPageLoader.item.selected].key === "output")), label: navigationGuide.adjusting ? "Apply" : root.page === "notifications" && notificationsPage.selected && /^dukkan-transaction:/.test(notificationsPage.selected.target || "") ? "Open Dukkan" : root.page === "notifications" && notificationsPage.selected && notificationsPage.selected.target === "transfer-pending" ? "Open Transfer" : "Select", key: "↵"},
                            {faces: ["east"], label: "Back", key: "Esc"},
                            {faces:["west"], show:root.page === "scrape" ? scrapePage.canDismiss : root.page === "transfer" ? transferPage.canDismiss : notifications.available && !navigationGuide.textEntry && ((root.page !== "notifications" && notifications.popups.length > 0) || (root.page === "notifications" && notificationsPage.hasItems && !notificationsPage.blocked && notificationsPage.selected !== null && !notificationsPage.selected.transaction && !notificationsPage.selected.transfer && !notificationsPage.selected.scrape)), label:root.page === "notifications" || root.page === "transfer" || root.page === "scrape" ? "Dismiss" : "Dismiss popups", key:"X"},
                            {faces:["north"], show:root.page === "scrape" ? scrapePage.canClear : root.page === "transfer" ? transferPage.canClear : notifications.available && root.page === "notifications" && notificationsPage.hasItems && !notificationsPage.blocked && notifications.records.length > 0, label:"Clear history", key:"Y"}
                        ]
                        delegate: Row {
                            required property var modelData
                            visible: modelData.show !== false
                            spacing: root.px(7)
                            MenuButtonDiamond {
                                visible: !modelData.directional && root.controllerCount > 0
                                uiScale: root.uiScale
                                activeFaces: modelData.faces || []
                                width: implicitWidth; height: implicitHeight
                            }
                            MenuText {
                                visible: modelData.directional === true || root.controllerCount === 0
                                text: modelData.key
                                font.family: root.monoFontName
                                font.pixelSize: root.px(12); color: "#EAC28D"
                                y: root.px(5)
                            }
                            MenuText {
                                text: modelData.label
                                font.pixelSize: root.px(13); color: "#B8C1CE"
                                y: root.px(5)
                            }
                        }
                    }
                }
                Item {
                    width: parent.width; height: root.px(14)
                    MenuText {
                        text: "KAWN ELECTRO"
                        color: "#9FADBF"; font.family: root.monoFontName
                        font.pixelSize: root.px(9); font.letterSpacing: 1
                    }
                    MenuText {
                        anchors.right: parent.right
                        text: root.exitBusy ? "PLEASE WAIT" : ""; color: "#9FADBF"; font.family: root.monoFontName
                        font.pixelSize: root.px(9); font.letterSpacing: 1
                    }
                }
            }
        }

        NotificationToastStack {
            anchors { top: parent.top; right: parent.right; margins: root.px(32) }
            width: Math.min(root.px(420), parent.width - root.px(64))
            uiScale: root.uiScale
            popups: notifications.popups
            visible: root.menuPresentsToasts
            z: 100
        }

        FocusScope {
            id: inputScope
            anchors.fill: parent
            focus: true
            enabled: root.menuOpen
            Keys.onPressed: event => {
                if (root.closing) { event.accepted = true; return; }
                if ((event.modifiers & Qt.ControlModifier) && (event.key === Qt.Key_PageUp || event.key === Qt.Key_PageDown)) {
                    if (!event.isAutoRepeat) root.switchTab(event.key === Qt.Key_PageUp ? -1 : 1);
                    event.accepted = true; return;
                }
                if (!root.editingKeyboard && (event.key === Qt.Key_X || event.key === Qt.Key_Y)) {
                    if (!event.isAutoRepeat) root.notificationShortcut(event.key === Qt.Key_Y);
                    event.accepted = true; return;
                }
                if (root.page === "bluetooth" && bluetoothPageLoader.item && bluetoothPageLoader.item.physicalKey(event)) {
                    event.accepted = true;
                    return;
                }
                if (root.page === "network" && networkPageLoader.item && networkPageLoader.item.physicalKey(event)) {
                    event.accepted = true;
                    return;
                }
                if (root.page === "scrape" && scrapePage.physicalKey(event)) { event.accepted=true;return; }
                if (event.key === Qt.Key_Up) {
                    if (!event.isAutoRepeat) root.input("key_up", 1);
                } else if (event.key === Qt.Key_Down) {
                    if (!event.isAutoRepeat) root.input("key_down", 1);
                } else if (event.key === Qt.Key_Right || event.key === Qt.Key_Left) {
                    if (!event.isAutoRepeat) {
                        root.input(event.key === Qt.Key_Left ? "key_left" : "key_right", 1);
                    }
                } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
                    if (!event.isAutoRepeat) root.activate();
                } else if (event.key === Qt.Key_Escape || event.key === Qt.Key_Back) {
                    if (!event.isAutoRepeat) root.back();
                } else return;
                event.accepted = true;
            }
            Keys.onReleased: event => {
                if (root.closing) { event.accepted = true; return; }
                if (event.key === Qt.Key_Up || event.key === Qt.Key_Down) {
                    if (!event.isAutoRepeat) root.input(event.key === Qt.Key_Up ? "key_up" : "key_down", 0);
                    event.accepted = true;
                } else if (event.key === Qt.Key_Left || event.key === Qt.Key_Right) {
                    if (!event.isAutoRepeat) root.input(event.key === Qt.Key_Left ? "key_left" : "key_right", 0);
                    event.accepted = true;
                }
            }
        }
        MouseArea {
            anchors.fill: parent
            z: 200
            visible: root.closing
            acceptedButtons: Qt.AllButtons
            onWheel: event => { event.accepted = true; }
        }
    }
}
