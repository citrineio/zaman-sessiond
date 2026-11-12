import QtQuick 2.12
import "../lib/Library.js" as Library
import "../lib/Hints.js" as Hints

FocusScope {
    id: shell
    objectName: "zamanShell"
    property bool controllerHints: true
    property var backend
    property real unit: Math.min(width / 1920, height / 1080)
    property int page: 0
    property string zone: "shelf"
    property int homeIndex: 0
    property int heroAction: 0
    property int gameIndex: 0
    property int toolbarIndex: 0
    property int settingIndex: 0
    readonly property var libraryTools: [
        {kind: "transfer", label: "Transfer Games", detail: "Receive games and review pending imports"},
        {kind: "scrape", label: "Artwork Lookup", detail: "Find covers, screenshots and game information"}
    ]
    property string collectionName: ""
    property string query: ""
    property int sortMode: 0
    property bool reducedMotion: true
    property bool compact: false
    property bool artworkEnabled: true
    property var games: []
    property var collections: []
    property var homeGames: []
    property var filteredGames: []
    property bool hasRecent: false
    property string modal: ""
    property var detailGame: null
    property int detailAction: 0
    readonly property var detailManagement: managementTarget(detailGame)
    readonly property int detailBackIndex: detailManagement ? 3 : 2
    readonly property string managedRomPrefix: "/srv/zaman/roms/"
    readonly property string gameManagementKey: "6f6a69288fc6aa0afedcbfcedb33fc90dd93b9127027e396"
    property int pickerIndex: 0
    property int keyboardIndex: 0
    property int keyboardView: 0 // lower | upper | symbols, as in Dukkan
    property string draftQuery: ""
    property bool launching: false
    property string notice: ""
    property string clock: Qt.formatTime(new Date(), "hh:mm")
    readonly property var keyRows: keyboardView === 2 ? [
        ["!","@","#","$","%","^","&","*","(",")"],
        ["-","_","=","+","[","]","{","}","\\","⌫"],
        [";",":","'","\"",",",".","<",">","/","?"],
        ["`","~","|","0","1","2","3","SPACE","CLEAR","DONE"]
    ] : [
        keyboardView === 1 ? ["Q","W","E","R","T","Y","U","I","O","P"] : ["q","w","e","r","t","y","u","i","o","p"],
        keyboardView === 1 ? ["A","S","D","F","G","H","J","K","L","⌫"] : ["a","s","d","f","g","h","j","k","l","⌫"],
        keyboardView === 1 ? ["Z","X","C","V","B","N","M","1","2","3"] : ["z","x","c","v","b","n","m","1","2","3"],
        ["4","5","6","7","8","9","0","SPACE","CLEAR","DONE"]
    ]
    readonly property string keyboardViewLabel: keyboardView === 0 ? "lowercase" : keyboardView === 1 ? "UPPERCASE" : "symbols"
    readonly property var keyboard: [].concat(keyRows[0], keyRows[1], keyRows[2], keyRows[3])
    readonly property int searchMatchCount: modal === "search"
        ? Library.filter(games, collectionName, page === 2, draftQuery, sortMode).length : 0
    readonly property int columns: compact ? 5 : 4
    readonly property var currentGame: page === 0 ? (homeGames[homeIndex] || null) : (filteredGames[gameIndex] || null)
    readonly property string selectedIdentity: Library.identity(currentGame)
    Brand { id: palette }
    Timer { interval: 60000; running: shell.visible; repeat: true; onTriggered: shell.clock = Qt.formatTime(new Date(), "hh:mm") }
    Timer { id: launchGuard; interval: 1400; onTriggered: shell.launching = false }
    Timer { id: noticeTimer; interval: 4000; onTriggered: shell.notice = "" }
    Timer { id: refresh; interval: 40; onTriggered: shell.rebuild() }
    Connections {
        target: shell.backend ? shell.backend.allGames : null
        ignoreUnknownSignals: true
        function onCountChanged() { refresh.restart(); }
        function onModelReset() { refresh.restart(); }
    }
    Connections {
        target: shell.backend ? shell.backend.collections : null
        ignoreUnknownSignals: true
        function onCountChanged() { refresh.restart(); }
    }

    function rebuild() {
        var keep = Library.identity(currentGame);
        games = Library.array(backend.allGames);
        collections = Library.array(backend.collections);
        if (collectionName && !collections.some(function(c) { return c.name === collectionName; })) collectionName = "";
        var recent = Library.recent(games);
        hasRecent = recent.length > 0;
        homeGames = (hasRecent ? recent : games.slice().sort(Library.titleOrder)).slice(0, 20);
        homeIndex = Math.max(0, Math.min(homeIndex, homeGames.length - 1));
        filterGames(keep);
    }
    function filterGames(keep) {
        filteredGames = Library.filter(games, collectionName, page === 2, query, sortMode);
        var found = -1;
        if (keep) for (var i = 0; i < filteredGames.length; ++i) if (Library.identity(filteredGames[i]) === keep) { found = i; break; }
        gameIndex = found >= 0 ? found : Math.max(0, Math.min(gameIndex, filteredGames.length - 1));
    }
    function save() {
        backend.memory.set("zaman.v1", {
            page: page, collection: collectionName, query: query, sort: sortMode,
            identity: Library.identity(currentGame), homeIndex: homeIndex, gameIndex: gameIndex,
            reducedMotion: reducedMotion, compact: compact, artworkEnabled: artworkEnabled
        });
    }
    function restore() {
        var m = backend.memory.get("zaman.v1");
        if (m) {
            page = Math.max(0, Math.min(3, Number(m.page) || 0));
            collectionName = m.collection || ""; query = m.query || ""; sortMode = m.sort === 1 ? 1 : 0;
            homeIndex = Number(m.homeIndex) || 0; gameIndex = Number(m.gameIndex) || 0;
            reducedMotion = m.reducedMotion !== false; compact = m.compact === true; artworkEnabled = m.artworkEnabled !== false;
        }
        rebuild();
        if (m && m.identity) {
            if (page === 0) {
                for (var i = 0; i < homeGames.length; ++i) if (Library.identity(homeGames[i]) === m.identity) homeIndex = i;
            }
            if (page === 1 || page === 2) filterGames(m.identity);
        }
        zone = page === 0 ? "shelf" : page === 3 ? "settings" : "grid";
    }
    Component.onCompleted: { restore(); forceActiveFocus(); }
    Component.onDestruction: { if (backend) save(); }
    function switchPage(next) {
        page = (next + 4) % 4;
        gameIndex = 0; filterGames("");
        zone = "nav"; save();
    }
    function enterContent() { zone = page === 0 ? (currentGame ? "hero" : "shelf") : page === 3 ? "settings" : "filters"; }
    function showNotice(message) { notice = message; noticeTimer.restart(); }
    function launch(game) {
        if (!game || launching) return;
        if (game.missing) { showNotice("This game file is unavailable."); return; }
        save(); launching = true; launchGuard.restart();
        game.launch(); // Existing Pegasus metadata owns the zamanctl launch command.
    }
    function managementTarget(game) {
        if (!game) return null;
        if (game.collections) {
            for (var i = 0; i < game.collections.count; ++i) {
                var collection = game.collections.get(i);
                var name = String(collection.shortName || "").toLowerCase();
                if (name === "native") return {kind: "dukkan"};
                if (name === "steam") return {kind: "steam"};
            }
        }
        if (game.missing || !game.files) return null;
        var paths = [];
        for (var j = 0; j < game.files.count; ++j) {
            var file = game.files.get(j);
            var path = String(file ? file.path : "");
            if (path.indexOf(managedRomPrefix) === 0 && paths.indexOf(path) < 0) paths.push(path);
        }
        if (!paths.length) return null;
        return {kind: "rom", paths: paths};
    }
    function openMenuRequest(payload, errorMessage) {
        if (!payload || launching) return false;
        var request = Object.assign({key: gameManagementKey}, payload);
        var url = "zaman-manage-game://game?request=" + encodeURIComponent(JSON.stringify(request));
        if (Qt.openUrlExternally(url)) {
            save();
            return true;
        }
        showNotice(errorMessage);
        return false;
    }
    function manageDetailGame() {
        if (openMenuRequest(detailManagement, "Could not open Zaman game management.")) modal = "";
    }
    function showDetails(game) {
        if (!game) return;
        detailGame = game; detailAction = 0; descriptionScroll.contentY = 0; modal = "details";
    }
    function favorite(game) {
        if (!game) return;
        var keep = Library.identity(game);
        game.favorite = !game.favorite;
        filterGames(keep); save();
    }
    function openSearch() { draftQuery = query; keyboardIndex = 0; keyboardView = 0; modal = "search"; forceActiveFocus(); }
    function applySearch() { query = draftQuery; gameIndex = 0; if (page === 0 || page === 3) page = 1; filterGames(""); modal = ""; zone = "grid"; save(); forceActiveFocus(); }
    function cycleKeyboardView() { keyboardView = (keyboardView + 1) % 3; }
    function deleteSearchCharacter() {
        // Delete a complete UTF-16 character when a physical keyboard supplied it.
        var length = draftQuery.length;
        if (!length) return;
        var last = draftQuery.charCodeAt(length - 1);
        var previous = length > 1 ? draftQuery.charCodeAt(length - 2) : 0;
        var pair = last >= 0xDC00 && last <= 0xDFFF && previous >= 0xD800 && previous <= 0xDBFF;
        draftQuery = draftQuery.slice(0, length - (pair ? 2 : 1));
    }
    function appendSearchText(text) {
        if (text && draftQuery.length + text.length <= 80) draftQuery += text;
    }
    function typeKey(index) {
        var key = keyboard[index];
        if (key === "DONE") applySearch();
        else if (key === "⌫") deleteSearchCharacter();
        else if (key === "CLEAR") draftQuery = "";
        else if (key !== undefined) appendSearchText(key === "SPACE" ? " " : key);
    }
    function handleSearchInput(event) {
        var controller = event.key >= 0x100000 && event.key <= 0x10000c;
        if (backend.keys.isCancel(event) || event.key === Qt.Key_Escape || event.key === 0x10000b) {
            if (!event.isAutoRepeat) applySearch();
        } else if (backend.keys.isAccept(event)) {
            if (!event.isAutoRepeat) typeKey(keyboardIndex);
        } else if (event.key === Qt.Key_Backspace || (controller && backend.keys.isDetails(event))) {
            deleteSearchCharacter();
        } else if (event.key === Qt.Key_Tab || (controller && backend.keys.isFilters(event))) {
            if (!event.isAutoRepeat) cycleKeyboardView();
        } else if (!controller && !(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))
                   && event.text && !/[\x00-\x1f\x7f]/.test(event.text)) {
            appendSearchText(event.text);
        }
        // Keep all search input inside this modal, including bound tab actions.
    }
    function openPicker() {
        pickerIndex = 0;
        for (var i = 0; i < collections.length; ++i) if (collections[i].name === collectionName) pickerIndex = i + 1;
        modal = "collections";
    }
    function chooseCollection(index) {
        collectionName = index === 0 ? "" : collections[index - 1].name;
        gameIndex = 0; filterGames(""); modal = ""; zone = "grid"; save();
    }
    function activateToolbar() {
        if (toolbarIndex === 0) openPicker();
        else if (toolbarIndex === 1) { sortMode = 1 - sortMode; filterGames(Library.identity(currentGame)); save(); }
        else openSearch();
    }
    function activatePreference(index) {
        if (index < 3) changeSetting(index);
        else {
            var tool = libraryTools[index - 3];
            if (tool) openMenuRequest({kind: tool.kind}, "Could not open " + tool.label + ".");
        }
    }
    function changeSetting(index) {
        if (index === 0) reducedMotion = !reducedMotion;
        else if (index === 1) compact = !compact;
        else artworkEnabled = !artworkEnabled;
        save();
    }
    function accept() {
        if (modal === "search") typeKey(keyboardIndex);
        else if (modal === "collections") chooseCollection(pickerIndex);
        else if (modal === "details") {
            if (detailAction === 0) launch(detailGame);
            else if (detailAction === 1) favorite(detailGame);
            else if (detailAction === 2 && detailManagement) manageDetailGame();
            else modal = "";
        }
        else if (zone === "nav") enterContent();
        else if (zone === "hero") {
            if (heroAction === 0) launch(currentGame);
            else showDetails(currentGame);
        }
        else if (zone === "filters") activateToolbar();
        else if (zone === "settings") activatePreference(settingIndex);
        else launch(currentGame);
    }
    function cancel() {
        if (modal === "search") applySearch();
        else if (modal) modal = "";
        else if (zone !== "nav") zone = "nav";
        else if (page !== 0) switchPage(0);
    }
    function movePreference(dx, dy) {
        var inTools = settingIndex >= 3;
        var row = inTools ? settingIndex - 3 : settingIndex;
        if (dx > 0 && !inTools && libraryTools.length)
            settingIndex = 3 + Math.min(row, libraryTools.length - 1);
        else if (dx < 0 && inTools)
            settingIndex = Math.min(row, 2);
        else if (dy < 0 && row === 0)
            zone = "nav";
        else if (dy)
            settingIndex = (inTools ? 3 : 0)
                + Math.max(0, Math.min(inTools ? libraryTools.length - 1 : 2, row + dy));
    }
    function move(dx, dy) {
        if (modal === "search") {
            var row = Math.floor(keyboardIndex / 10);
            var column = keyboardIndex % 10;
            row = Math.max(0, Math.min(keyRows.length - 1, row + dy));
            column = Math.max(0, Math.min(keyRows[row].length - 1, column + dx));
            keyboardIndex = row * 10 + column;
        } else if (modal === "collections") {
            pickerIndex = Math.max(0, Math.min(collections.length, pickerIndex + dy));
            picker.positionViewAtIndex(pickerIndex, ListView.Contain);
        } else if (modal === "details") {
            detailAction = Math.max(0, Math.min(detailBackIndex, detailAction + dx));
            if (dy) descriptionScroll.contentY = Math.max(0, Math.min(Math.max(0, descriptionScroll.contentHeight - descriptionScroll.height), descriptionScroll.contentY + dy * 100));
        } else if (zone === "nav") {
            if (dx) switchPage(page + dx);
            if (dy > 0) enterContent();
        } else if (zone === "shelf") {
            if (dy < 0) zone = currentGame ? "hero" : "nav";
            homeIndex = Math.max(0, Math.min(homeGames.length - 1, homeIndex + dx));
            shelf.positionViewAtIndex(homeIndex, ListView.Contain);
        } else if (zone === "hero") {
            heroAction = Math.max(0, Math.min(1, heroAction + dx));
            if (dy < 0) zone = "nav";
            if (dy > 0) zone = "shelf";
        } else if (zone === "filters") {
            toolbarIndex = Math.max(0, Math.min(2, toolbarIndex + dx));
            if (dy < 0) zone = "nav";
            if (dy > 0) zone = "grid";
        } else if (zone === "grid") {
            if (dy < 0 && gameIndex < columns) zone = "filters";
            else {
                gameIndex = Math.max(0, Math.min(filteredGames.length - 1, gameIndex + dx + dy * columns));
                grid.positionViewAtIndex(gameIndex, GridView.Contain);
            }
        } else if (zone === "settings") {
            movePreference(dx, dy);
        }
    }
    Keys.onPressed: function(event) {
        // Close theme dialogs first; otherwise Escape belongs to Pegasus.
        if (event.key === Qt.Key_Escape && !modal) {
            controllerHints = false;
            event.accepted = false;
            return;
        }
        event.accepted = true;
        if (event.key >= 0x100000 && event.key <= 0x10000c) controllerHints = true;
        else if ([Qt.Key_Left, Qt.Key_Right, Qt.Key_Up, Qt.Key_Down].indexOf(event.key) < 0) controllerHints = false;
        if (event.key === Qt.Key_Left) move(-1, 0);
        else if (event.key === Qt.Key_Right) move(1, 0);
        else if (event.key === Qt.Key_Up) move(0, -1);
        else if (event.key === Qt.Key_Down) move(0, 1);
        else if (modal === "search") handleSearchInput(event);
        else if (backend.keys.isAccept(event)) { if (!event.isAutoRepeat) accept(); }
        else if (backend.keys.isCancel(event)) {
            if (!modal && zone === "nav" && page === 0) event.accepted = false;
            else if (!event.isAutoRepeat) cancel();
        }
        else if (backend.keys.isDetails(event) && !modal && page !== 3) { if (!event.isAutoRepeat) showDetails(currentGame); }
        else if (backend.keys.isFilters(event) && !modal) { if (!event.isAutoRepeat) openSearch(); }
        else if (backend.keys.isNextPage(event) && !modal) { if (!event.isAutoRepeat) switchPage(page + 1); }
        else if (backend.keys.isPrevPage(event) && !modal) { if (!event.isAutoRepeat) switchPage(page - 1); }
        else if (backend.keys.isPageDown(event) && !modal && (page === 1 || page === 2)) { zone = "grid"; move(0, 2); }
        else if (backend.keys.isPageUp(event) && !modal && (page === 1 || page === 2)) { zone = "grid"; move(0, -2); }
        else event.accepted = false;
    }

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: palette.ink }
            GradientStop { position: 0.55; color: "#0D121B" }
            GradientStop { position: 1; color: "#10151D" }
        }
    }
    Item {
        id: canvas
        width: shell.width / shell.unit; height: shell.height / shell.unit
        scale: shell.unit; transformOrigin: Item.TopLeft
        Surface { x: 28; y: 26; width: parent.width - 56; height: 100; brand: palette }
        BrandLogo { x: 76; y: 40; brand: palette }
        Rectangle { x: 396; y: 48; width: 1; height: 55; color: palette.divider }
        Row {
            x: 428; y: 44; spacing: 10
            Repeater {
                model: ["Home", "Library", "Favorites", "Preferences"]
                delegate: SelectionSurface {
                    brand: palette
                    selected: !shell.modal && shell.page === index && shell.zone === "nav"
                    width: index === 3 ? 196 : 154; height: 64
                    border.width: selected ? 1 : 0
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: shell.page === index && shell.zone === "nav" ? palette.gold : "#00121820" }
                        GradientStop { position: 1; color: shell.page === index && shell.zone === "nav" ? palette.caramel : "#00121820" }
                    }
                    Text { anchors.centerIn: parent; text: modelData; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 23; color: shell.page === index && shell.zone === "nav" ? palette.selectedText : shell.page === index ? palette.gold : palette.muted }
                    Rectangle { visible: shell.page === index && shell.zone !== "nav"; anchors.bottom: parent.bottom; anchors.horizontalCenter: parent.horizontalCenter; width: parent.width - 44; height: 3; color: palette.gold }
                    MouseArea { anchors.fill: parent; onClicked: { shell.switchPage(index); shell.enterContent(); shell.forceActiveFocus(); } }
                }
            }
        }
        Text {
            anchors.right: parent.right; anchors.rightMargin: 78; y: 63
            text: shell.clock; font.family: palette.mono; font.pixelSize: 19; color: palette.muted
        }
        Text {
            anchors.right: parent.right; anchors.rightMargin: 213; y: 65
            text: shell.backend && !isNaN(shell.backend.device.batteryPercent) ? Math.round(shell.backend.device.batteryPercent * 100) + "%" + (shell.backend.device.batteryCharging ? " +" : "") : ""
            font.family: palette.mono; font.pixelSize: 16; color: palette.muted
        }

        Item {
            id: home
            x: 76; y: 151; width: parent.width - 152; height: parent.height - 247
            visible: shell.page === 0
            Surface {
                id: hero
                objectName: "heroPanel"
                brand: palette
                width: parent.width; height: parent.height - 335
                clip: true
                GameArtwork {
                    id: heroArt
                    objectName: "heroArtwork"
                    x: parent.width * (fitted ? 0.60 : 0.32); y: 12
                    width: parent.width * (fitted ? 0.38 : 0.68) - 12; height: parent.height - 24
                    game: shell.currentGame; enabledArtwork: shell.artworkEnabled
                    roles: ["background", "screenshot", "banner", "titlescreen", "steam", "logo", "boxFront", "poster", "tile"]
                }
                Rectangle {
                    anchors.fill: parent
                    radius: 24
                    visible: !heroArt.fitted
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: palette.panel }
                        GradientStop { position: 0.37; color: palette.panel }
                        GradientStop { position: 0.74; color: "#9912161E" }
                        GradientStop { position: 1; color: "#6612161E" }
                    }
                }
                
                Rectangle {
                    anchors.fill: parent; color: "transparent"
                    radius: 24; border.width: 1; border.color: palette.line
                }
                MouseArea {
                    objectName: "heroArtworkInput"
                    anchors.fill: parent
                    enabled: !!shell.currentGame && !shell.launching
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        shell.zone = "hero"; shell.heroAction = 1;
                        shell.forceActiveFocus(); shell.showDetails(shell.currentGame);
                    }
                }
                Column {
                    z: 1
                    x: 45; y: 31; width: parent.width * 0.59; spacing: 9
                    Text { text: shell.hasRecent ? "BACK TO YOUR GAMES" : "YOUR NEXT ADVENTURE"; font.family: palette.mono; font.pixelSize: 16; font.letterSpacing: 2; color: palette.gold }
                    Text {
                        width: parent.width; height: 156
                        text: shell.currentGame ? shell.currentGame.title : "Every generation.\nOne home."
                        font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 64; fontSizeMode: Text.Fit; minimumPixelSize: 32
                        wrapMode: Text.WordWrap; maximumLineCount: 2; elide: Text.ElideRight; color: palette.ivory
                    }
                    Text { width: parent.width; text: "Category: " + Library.collection(shell.currentGame).toUpperCase(); elide: Text.ElideRight; font.family: palette.body; font.pixelSize: 17; color: palette.gold }
                    Text {
                        width: parent.width; height: 66
                        text: shell.currentGame ? (shell.currentGame.summary || shell.currentGame.description || Library.metadata(shell.currentGame)) : "Your games will appear here when they are added to Pegasus."
                        maximumLineCount: 2; elide: Text.ElideRight; wrapMode: Text.WordWrap
                        font.family: palette.body; font.pixelSize: 23; color: palette.ivory
                    }
                    Row {
                        spacing: 16; visible: !!shell.currentGame
                        ActionButton {
                            objectName: "heroPlay"; brand: palette
                            label: shell.launching ? "Launching…" : "Play game"
                            selected: !shell.modal && shell.zone === "hero" && shell.heroAction === 0
                            enabled: !!shell.currentGame && !shell.launching
                            onActivated: {
                                shell.zone = "hero"; shell.heroAction = 0;
                                shell.forceActiveFocus(); shell.launch(shell.currentGame);
                            }
                        }
                        ActionButton {
                            objectName: "heroDetails"; brand: palette; label: "Game details"
                            selected: !shell.modal && shell.zone === "hero" && shell.heroAction === 1
                            enabled: !!shell.currentGame && !shell.launching
                            onActivated: {
                                shell.zone = "hero"; shell.heroAction = 1;
                                shell.forceActiveFocus(); shell.showDetails(shell.currentGame);
                            }
                        }
                    }
                }
            }
            Text { y: hero.height + 20; text: shell.hasRecent ? "Recently played" : "From your library"; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 29; color: palette.ivory }
            Text { anchors.right: parent.right; y: hero.height + 32; text: shell.homeGames.length ? (shell.homeIndex + 1) + " / " + shell.homeGames.length : "0 GAMES"; font.family: palette.mono; font.pixelSize: 17; color: palette.muted }
            ListView {
                id: shelf
                objectName: "recentShelf"
                y: hero.height + 75; width: parent.width; height: 260
                orientation: ListView.Horizontal; spacing: 24; clip: true
                model: shell.homeGames; currentIndex: shell.homeIndex
                boundsBehavior: Flickable.StopAtBounds
                highlightMoveDuration: shell.reducedMotion ? 0 : 120
                delegate: GameCard {
                    brand: palette; reducedMotion: shell.reducedMotion; game: modelData; width: (home.width - 72) / 4; height: 258
                    selected: !shell.modal && shell.zone === "shelf" && index === shell.homeIndex
                    onChosen: { shell.homeIndex = index; shell.zone = "shelf"; shell.forceActiveFocus(); }
                    onOpened: shell.showDetails(modelData)
                }
            }
        }

        Item {
            id: library
            x: 76; y: 151; width: parent.width - 152; height: parent.height - 247
            visible: shell.page === 1 || shell.page === 2
            Text { text: shell.page === 2 ? "Your favorites" : "Your library"; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 42; color: palette.ivory }
            Text { anchors.right: parent.right; y: 26; text: shell.filteredGames.length + (shell.filteredGames.length === 1 ? " GAME" : " GAMES"); font.family: palette.mono; font.pixelSize: 16; color: palette.muted }
            Row {
                y: 87; spacing: 16
                ActionButton { brand: palette; width: (library.width - 32) * 0.36; label: "Category: " + (shell.collectionName || "All"); selected: !shell.modal && shell.zone === "filters" && shell.toolbarIndex === 0; onActivated: { shell.toolbarIndex = 0; shell.openPicker(); } }
                ActionButton { brand: palette; width: (library.width - 32) * 0.26; label: shell.sortMode === 0 ? "Sort: Title A–Z" : "Sort: Recently played"; selected: !shell.modal && shell.zone === "filters" && shell.toolbarIndex === 1; onActivated: { shell.toolbarIndex = 1; shell.activateToolbar(); } }
                ActionButton { brand: palette; width: (library.width - 32) * 0.38; label: shell.query ? "Search: " + shell.query : "Search games"; selected: !shell.modal && shell.zone === "filters" && shell.toolbarIndex === 2; onActivated: shell.openSearch() }
            }
            GridView {
                id: grid
                objectName: "libraryGrid"
                y: 184; width: parent.width + 24; height: parent.height - 184
                clip: true; model: shell.filteredGames; currentIndex: shell.gameIndex
                cellWidth: width / shell.columns; cellHeight: (cellWidth - 24) * 9 / 16 + 88
                boundsBehavior: Flickable.StopAtBounds
                highlightMoveDuration: shell.reducedMotion ? 0 : 120
                delegate: GameCard {
                    brand: palette; reducedMotion: shell.reducedMotion; game: modelData; width: grid.cellWidth - 24; height: grid.cellHeight - 16
                    selected: !shell.modal && shell.zone === "grid" && index === shell.gameIndex
                    onChosen: { shell.gameIndex = index; shell.zone = "grid"; shell.forceActiveFocus(); }
                    onOpened: shell.showDetails(modelData)
                }
            }
            Column {
                visible: shell.filteredGames.length === 0
                anchors.centerIn: parent; width: parent.width * 0.7; spacing: 20
                Text { width: parent.width; text: shell.query ? "No matching games" : shell.page === 2 ? "Make room for your favorites" : "Your library starts here"; horizontalAlignment: Text.AlignHCenter; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 42; color: palette.ivory }
                Text { width: parent.width; text: shell.query || shell.collectionName ? "Try another search or choose Category: All." : shell.page === 2 ? "Open a game's details and select Add favorite." : "Games and artwork appear here from your Pegasus library."; horizontalAlignment: Text.AlignHCenter; wrapMode: Text.WordWrap; font.family: palette.body; font.pixelSize: 25; color: palette.muted }
                ActionButton { anchors.horizontalCenter: parent.horizontalCenter; brand: palette; width: 300; label: "Search and filters"; onActivated: { shell.zone = "filters"; shell.toolbarIndex = 0; } }
            }
        }

        Item {
            id: preferences
            x: 76; y: 169; width: parent.width - 152; height: parent.height - 260
            visible: shell.page === 3
            readonly property real columnGap: 36
            readonly property real columnWidth: (width - columnGap) / 2
            readonly property real rowHeight: 116
            readonly property real rowGap: 16

            Text {
                text: "Preferences"
                font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 46
                color: palette.ivory
            }
            Text {
                y: 64; text: "Adjust the display or manage your game library."
                font.family: palette.body; font.pixelSize: 24; color: palette.muted
            }
            Text {
                y: 135; text: "DISPLAY PREFERENCES"
                font.family: palette.mono; font.pixelSize: 18; font.letterSpacing: 2
                color: palette.gold
            }
            Text {
                x: preferences.columnWidth + preferences.columnGap
                y: 135; text: "LIBRARY TOOLS"
                font.family: palette.mono; font.pixelSize: 18; font.letterSpacing: 2
                color: palette.gold
            }
            Repeater {
                model: [
                    {label: "Motion", detail: "Focus transitions between games"},
                    {label: "Library density", detail: "Games across each library row"},
                    {label: "Featured artwork", detail: "Large game art on the home screen"}
                ].concat(shell.libraryTools)
                delegate: SelectionSurface {
                    id: preferenceCard
                    readonly property bool tool: index >= 3
                    x: tool ? preferences.columnWidth + preferences.columnGap : 0
                    y: 176 + (tool ? index - 3 : index) * (preferences.rowHeight + preferences.rowGap)
                    width: preferences.columnWidth
                    height: preferences.rowHeight
                    brand: palette
                    selected: !shell.modal && shell.zone === "settings" && shell.settingIndex === index

                    Text {
                        id: rowValue
                        anchors.right: parent.right; anchors.rightMargin: 26
                        anchors.verticalCenter: parent.verticalCenter
                        text: preferenceCard.tool ? "›"
                            : index === 0 ? (shell.reducedMotion ? "Reduced" : "Standard")
                            : index === 1 ? (shell.compact ? "5 columns" : "4 columns")
                            : shell.artworkEnabled ? "On" : "Off"
                        font.family: palette.heading; font.weight: Font.DemiBold
                        font.pixelSize: preferenceCard.tool ? 36 : 25
                        color: preferenceCard.selected ? palette.selectedText : palette.gold
                    }
                    Column {
                        anchors.left: parent.left; anchors.leftMargin: 28
                        anchors.right: rowValue.left; anchors.rightMargin: 22
                        anchors.verticalCenter: parent.verticalCenter
                        spacing: 6
                        Text {
                            width: parent.width; text: modelData.label; elide: Text.ElideRight
                            font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 29
                            color: preferenceCard.selected ? palette.selectedText : palette.text
                        }
                        Text {
                            width: parent.width; text: modelData.detail; elide: Text.ElideRight
                            font.family: palette.body; font.pixelSize: 22
                            color: preferenceCard.selected ? palette.selectedDetail : palette.dim
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            shell.settingIndex = index;
                            shell.zone = "settings";
                            shell.activatePreference(index);
                        }
                    }
                }
            }
        }

        Rectangle { x: 76; y: parent.height - 82; width: parent.width - 152; height: 1; color: palette.line }
        NavigationLegend {
            x: 76; y: parent.height - 66
            brand: palette
            entries: Hints.forState(shell, "")
        }
        Text { anchors.right: parent.right; anchors.rightMargin: 76; y: parent.height - 58; text: "KAWN ELECTRO"; font.family: palette.mono; font.pixelSize: 16; font.letterSpacing: 1; color: palette.caramel }

        Rectangle {
            id: details
            anchors.fill: parent; visible: shell.modal === "details"; color: palette.ink
            MouseArea { anchors.fill: parent } // Block pointer events reaching the library.
            Surface { x: 28; y: 26; width: parent.width * 0.63 - 28; height: parent.height - 52; brand: palette }
            
            Text { x: 76; y: 42; text: "ZAMAN / GAME DETAILS"; font.family: palette.mono; font.pixelSize: 17; color: palette.gold }
            Image {
                x: parent.width * 0.67; y: 155; width: parent.width * 0.27; height: parent.height - 295
                source: Library.asset(shell.detailGame, ["boxFront", "poster", "tile", "banner", "screenshot"])
                sourceSize.width: 650; sourceSize.height: 950; asynchronous: true; fillMode: Image.PreserveAspectFit
            }
            Text { x: parent.width * 0.67; y: parent.height - 103; width: parent.width * 0.27; text: shell.detailGame ? "PLAYED " + Library.duration(shell.detailGame.playTime).toUpperCase() : ""; horizontalAlignment: Text.AlignHCenter; font.family: palette.mono; font.pixelSize: 17; color: palette.muted }
            Column {
                x: 76; y: 132; width: parent.width * 0.56; spacing: 20
                Text { width: parent.width; text: "Category: " + Library.collection(shell.detailGame).toUpperCase(); elide: Text.ElideRight; font.family: palette.body; font.pixelSize: 16; color: palette.caramel }
                Text { width: parent.width; height: 173; text: shell.detailGame ? shell.detailGame.title : ""; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 66; fontSizeMode: Text.Fit; minimumPixelSize: 32; maximumLineCount: 2; wrapMode: Text.WordWrap; elide: Text.ElideRight; color: palette.ivory }
                Text { width: parent.width; text: Library.metadata(shell.detailGame); wrapMode: Text.WordWrap; font.family: palette.body; font.pixelSize: 23; color: palette.gold }
                Text { width: parent.width; text: shell.detailGame ? shell.detailGame.developer || "" : ""; elide: Text.ElideRight; font.family: palette.mono; font.pixelSize: 16; color: palette.muted }
                Flickable {
                    id: descriptionScroll
                    width: parent.width; height: canvas.height - 676
                    clip: true; contentWidth: width; contentHeight: detailDescription.height
                    boundsBehavior: Flickable.StopAtBounds
                    Text { id: detailDescription; width: parent.width; text: shell.detailGame ? (shell.detailGame.description || shell.detailGame.summary || "No description is available for this game yet.") : ""; wrapMode: Text.WordWrap; font.family: palette.body; font.pixelSize: 25; lineHeight: 1.15; color: palette.ivory }
                }
            }
            Row {
                x: 76; y: parent.height - 195; spacing: 18
                ActionButton { brand: palette; label: shell.launching ? "Launching…" : "Play game"; selected: shell.detailAction === 0; onActivated: shell.launch(shell.detailGame) }
                ActionButton { brand: palette; width: 280; label: shell.detailGame && shell.detailGame.favorite ? "Remove favorite" : "Add favorite"; selected: shell.detailAction === 1; onActivated: shell.favorite(shell.detailGame) }
                ActionButton {
                    brand: palette
                    width: 260
                    visible: shell.detailManagement !== null
                    label: shell.detailManagement && shell.detailManagement.kind === "rom" ? "Delete ROM" : "Manage installation"
                    selected: shell.detailAction === 2
                    onActivated: shell.manageDetailGame()
                }
                ActionButton { brand: palette; label: "Back"; selected: shell.detailAction === shell.detailBackIndex; onActivated: shell.modal = "" }
            }
            NavigationLegend { x: 76; y: parent.height - 72; brand: palette; entries: Hints.forState(shell, "details") }
        }

        Rectangle {
            anchors.fill: parent; visible: shell.modal === "collections"; color: palette.ink
            MouseArea { anchors.fill: parent }
            Surface { x: 28; y: 26; width: parent.width * 0.63 - 28; height: parent.height - 52; brand: palette }
            
            Text { x: 76; y: 62; text: "Choose a category"; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 42; color: palette.ivory }
            Text { x: 78; y: 150; text: "YOUR CATEGORIES"; font.family: palette.body; font.pixelSize: 16; color: palette.gold }
            ListView {
                id: picker
                x: 76; y: 220; width: parent.width * 0.54; height: parent.height - 334; clip: true; spacing: 16
                model: shell.collections.length + 1; currentIndex: shell.pickerIndex
                delegate: SelectionSurface {
                    brand: palette; selected: index === shell.pickerIndex
                    width: picker.width; height: 98
                    Text { x: 28; anchors.verticalCenter: parent.verticalCenter; width: parent.width - 140; text: index === 0 ? "All" : shell.collections[index - 1].name; elide: Text.ElideRight; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 29; color: index === shell.pickerIndex ? palette.selectedText : palette.text }
                    Text { anchors.right: parent.right; anchors.rightMargin: 28; anchors.verticalCenter: parent.verticalCenter; text: index === 0 ? shell.games.length : shell.collections[index - 1].games.count; font.family: palette.mono; font.pixelSize: 22; color: index === shell.pickerIndex ? palette.selectedDetail : palette.gold }
                    MouseArea { anchors.fill: parent; onClicked: shell.chooseCollection(index) }
                }
            }
            NavigationLegend { x: 76; y: parent.height - 72; brand: palette; entries: Hints.forState(shell, "collections") }
        }

        Rectangle {
            anchors.fill: parent; visible: shell.modal === "search"; color: palette.ink
            MouseArea { anchors.fill: parent }
            
            Text { x: 76; y: 63; text: "Find your next game."; font.family: palette.heading; font.weight: Font.DemiBold; font.pixelSize: 42; color: palette.ivory }
            Text { x: 78; y: 150; text: "SEARCH YOUR LIBRARY"; font.family: palette.body; font.pixelSize: 16; color: palette.gold }
            ActionButton {
                objectName: "keyboardViewButton"
                brand: palette; x: parent.width - 416; y: 123; width: 340; height: 64
                label: shell.keyboardViewLabel + "  ↻"
                onActivated: { shell.cycleKeyboardView(); shell.forceActiveFocus(); }
            }
            SelectionSurface {
                brand: palette
                x: 76; y: 232; width: parent.width - 152; height: 102; border.color: palette.gold
                Text { x: 30; anchors.verticalCenter: parent.verticalCenter; width: parent.width - 60; text: shell.draftQuery ? shell.draftQuery + "│" : "Enter a game title…"; elide: Text.ElideLeft; font.family: palette.body; font.pixelSize: 39; color: shell.draftQuery ? palette.ivory : palette.muted }
            }
            Text { x: 78; y: 354; text: shell.searchMatchCount + (shell.searchMatchCount === 1 ? " game matches" : " games match") + (shell.page === 2 ? " in your favorites" : " in your library"); font.family: palette.body; font.pixelSize: 22; color: palette.muted }
            Grid {
                id: searchKeyboard
                objectName: "searchKeyboard"
                x: 76; y: 407; columns: 10; spacing: 16
                Repeater {
                    model: shell.keyboard
                    delegate: SelectionSurface {
                        brand: palette; selected: index === shell.keyboardIndex
                        width: (canvas.width - 152 - 144) / 10; height: 94
                        Text { anchors.centerIn: parent; text: modelData; font.family: palette.body; font.pixelSize: modelData.length > 1 ? 21 : 32; color: index === shell.keyboardIndex ? palette.selectedText : palette.text }
                        MouseArea { anchors.fill: parent; onClicked: { shell.keyboardIndex = index; shell.forceActiveFocus(); shell.typeKey(index); } }
                    }
                }
            }
            NavigationLegend { x: 76; y: parent.height - 72; brand: palette; keyFont: palette.body; entries: Hints.forState(shell, "search") }
        }
        SelectionSurface {
            brand: palette; selected: true
            visible: shell.notice !== ""; anchors.horizontalCenter: parent.horizontalCenter; y: parent.height - 157
            width: Math.min(parent.width - 152, noticeText.implicitWidth + 64); height: 70
            Text { id: noticeText; anchors.centerIn: parent; text: shell.notice; font.family: palette.body; font.pixelSize: 24; color: palette.selectedText }
        }
    }
}
