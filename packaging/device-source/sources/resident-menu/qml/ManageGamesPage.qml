import QtQuick
import Quickshell.Io

Item {
    id: page
    property real uiScale: 1
    property string client: ""
    property string view: "collections"
    property string collection: ""
    property string collectionName: ""
    property string selectedGame: ""
    property bool direct: false
    property var linkedGames: []
    property var collections: []
    property var games: []
    property var confirmation: ({})
    property int currentPage: 0
    property bool more: false
    property int count: 0
    property int selected: 0
    property string notice: ""
    property var request: ({})
    readonly property bool removing: worker.running && request.action === "rom-delete"
    signal backRequested()
    signal storeRequested(string id)

    readonly property var entries: {
        if (view === "confirm") return [
            {kind: "cancel", label: "Keep game", detail: ""},
            {kind: "delete", label: "Delete permanently", detail: ""}
        ];
        if (view === "linked") {
            return linkedGames.map(game => ({kind:"linked", game:game, label:game.label, detail:game.collection}))
                .concat([{kind:"back", label:"Back to Pegasus", detail:""}]);
        }
        if (view === "games") {
            var result = games.map(game => ({kind:"game", id:game.id, label:game.label, detail:game.detail}));
            if (currentPage > 0) result.push({kind:"previous", label:"Previous page", detail:""});
            if (more) result.push({kind:"next", label:"Next page", detail:""});
            result.push({kind:"back", label:"Back to collections", detail:""});
            return result;
        }
        return collections.map(item => ({kind:"collection", id:item.id, label:item.label, detail:"Emulated games"}))
            .concat([{kind:"dukkan", label:"Dukkan", detail:"Uninstall native games"},
                     {kind:"steam", label:"Steam", detail:"Uninstall Steam games"},
                     {kind:"refresh", label:"Refresh collections", detail:""},
                     {kind:"back", label:"Back", detail:""}]);
    }

    function px(value) { return Math.round(value * uiScale); }
    function send(message) {
        if (worker.running) return;
        if (!client.length) { notice = "The Transfer service client is unavailable."; return; }
        request = message;
        notice = "";
        worker.running = true;
    }
    function openSelected(games) {
        direct = true;
        linkedGames = games;
        view = "linked";
        selected = 0;
        if (games.length === 1) previewLinked(games[0]);
    }
    function previewLinked(game) {
        collection = game.collection;
        selectedGame = game.id;
        send({action:"rom-preview", collection:collection, id:selectedGame});
    }
    function open() {
        direct = false;
        linkedGames = [];
        view = "collections";
        selected = 0;
        send({action:"rom-collections"});
    }
    function select(index) {
        selected = (index + entries.length) % entries.length;
        list.positionViewAtIndex(selected, ListView.Contain);
    }
    function move(delta) {
        if (!worker.running) select(selected + delta);
    }
    function listGames(index) {
        send({action:"rom-games", collection:collection, page:index});
    }
    function back() {
        if (worker.running) return;
        notice = "";
        if (direct) {
            if (view === "confirm" && linkedGames.length > 1) {
                view = "linked";
                selected = Math.max(0, linkedGames.findIndex(game => game.id === selectedGame
                                                           && game.collection === collection));
            } else backRequested();
            return;
        }
        if (view === "confirm") {
            view = "games";
            selected = Math.max(0, games.findIndex(game => game.id === selectedGame));
        } else if (view === "games") {
            view = "collections";
            selected = Math.max(0, collections.findIndex(item => item.id === collection));
        } else backRequested();
    }
    function activate() {
        if (worker.running || !entries[selected]) return;
        var entry = entries[selected];
        switch (entry.kind) {
        case "collection":
            collection = entry.id;
            collectionName = entry.label;
            listGames(0);
            break;
        case "linked":
            previewLinked(entry.game);
            break;
        case "game":
            selectedGame = entry.id;
            send({action:"rom-preview", collection:collection, id:selectedGame});
            break;
        case "delete":
            send({action:"rom-delete", collection:collection, id:selectedGame, token:confirmation.token, page:currentPage});
            break;
        case "previous": listGames(currentPage - 1); break;
        case "next": listGames(currentPage + 1); break;
        case "refresh": open(); break;
        case "dukkan":
        case "steam": storeRequested(entry.kind); break;
        default: back();
        }
    }
    function sizeText(value) {
        return value >= 1073741824 ? (value / 1073741824).toFixed(2) + " GiB"
            : value >= 1048576 ? (value / 1048576).toFixed(1) + " MiB"
            : Math.ceil(value / 1024) + " KiB";
    }
    function receive(text) {
        var data;
        try { data = JSON.parse(text); }
        catch (error) { notice = "The game library service did not reply. Try again."; return; }
        switch (data.type) {
        case "rom-collections":
            collections = data.collections;
            break;
        case "rom-games":
            games = data.games;
            count = data.count;
            currentPage = data.page;
            more = data.more;
            view = "games";
            selected = 0;
            break;
        case "rom-confirm":
            confirmation = data;
            view = "confirm";
            selected = 0;
            break;
        case "rom-deleted":
            if (direct) {
                linkedGames = [];
                view = "linked";
                selected = 0;
                notice = "Deleted " + data.title + ".";
                break;
            }
            games = data.games;
            count = data.count;
            currentPage = data.page;
            more = data.more;
            view = "games";
            selected = 0;
            notice = "Deleted " + data.title + ".";
            break;
        case "error":
            notice = data.error;
            break;
        default:
            notice = "Game management is unavailable.";
        }
    }

    Process {
        id: worker
        command: ["/usr/bin/python3", page.client, "request", JSON.stringify(page.request)]
        stdout: StdioCollector { onStreamFinished: page.receive(this.text) }
    }

    Column {
        id: header
        width: parent.width
        spacing: page.px(8)
        MenuText {
            width: parent.width
            wrapMode: Text.WordWrap
            text: page.view === "confirm" ? "Delete " + page.confirmation.title + "?"
                : page.view === "linked" ? (page.linkedGames.length > 1 ? "Choose a game file" : "Game management")
                : page.view === "games" ? page.collectionName + " · " + page.count + " games"
                : "Choose a collection or storefront"
            font.pixelSize: page.px(24)
            color: "#ECEEF1"
        }
        MenuText {
            width: parent.width
            wrapMode: Text.WordWrap
            text: worker.running ? (page.removing ? "Deleting game files…" : "Checking game library…")
                : page.notice || (page.view === "confirm"
                    ? page.confirmation.files + " game file(s) · " + page.sizeText(page.confirmation.bytes)
                      + (page.confirmation.shared ? " · " + page.confirmation.shared + " shared file(s) kept" : "")
                    : page.view === "games" && page.count ? "Page " + (page.currentPage + 1) : "")
            visible: text.length > 0
            font.pixelSize: page.px(16)
            color: page.notice ? "#EAC28D" : "#BBC6D3"
        }
    }

    ListView {
        id: list
        anchors { top:header.bottom; left:parent.left; right:parent.right; bottom:parent.bottom; topMargin:page.px(16) }
        model: page.entries
        spacing: page.px(8)
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        delegate: Rectangle {
            id: card
            required property var modelData
            required property int index
            readonly property bool focused: page.selected === index
            width: list.width
            height: page.px(modelData.detail ? 62 : 48)
            radius: page.px(11)
            color: focused ? (modelData.kind === "delete" ? "#E79E94" : "#EAC28D") : "#1D2634"
            border.color: focused ? "#F4D2A4" : "#3A495C"
            Column {
                anchors { verticalCenter:parent.verticalCenter; left:parent.left; right:parent.right; margins:page.px(14) }
                spacing: page.px(3)
                MenuText {
                    width: parent.width
                    text: card.modelData.label
                    elide: Text.ElideRight
                    font.pixelSize: page.px(20)
                    color: card.focused ? "#211E1B" : "#ECEEF1"
                }
                MenuText {
                    width: parent.width
                    text: card.modelData.detail
                    visible: text.length > 0
                    elide: Text.ElideMiddle
                    font.pixelSize: page.px(14)
                    color: card.focused ? "#473627" : "#BBC6D3"
                }
            }
            MouseArea {
                anchors.fill: parent
                enabled: !worker.running
                onClicked: { page.select(card.index); page.activate(); }
            }
        }
    }
}
