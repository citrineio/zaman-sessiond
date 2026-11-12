import QtQuick
import QtQml.Models

Item {
    id: page
    property real uiScale: 1
    property var snapshot: ({jobs:[], systems:[], enabled:false})
    property bool available: false
    property bool busy: false
    property string error: ""
    property string selectedId: "receiver"
    property string detailId: ""
    property bool confirming: false
    property int choice: 0
    signal command(var request)
    signal openScraping(string importId, string system, string jobId)
    readonly property var jobs: snapshot.jobs || []
    readonly property int historyCount: snapshot.historyCount || 0
    readonly property var selected: jobs.find(j => j.id === selectedId) || null
    readonly property var detail: detailId === "history" ? ({id:"history",name:"Clear transfer history"}) : jobs.find(j => j.id === detailId) || null
    readonly property bool blocked: confirming || detail !== null
    readonly property bool canDismiss: available && !busy && !blocked && selected !== null && ["complete","failed","cancelled"].indexOf(selected.phase) >= 0
    readonly property bool canClear: available && !busy && !blocked && historyCount > 0
    readonly property var rows: [{id:"receiver",name:snapshot.enabled ? "Stop receiving new transfers" : "Receive for 15 minutes"}].concat(jobs)
    readonly property var choices: {
        if (!detail) return [];
        if (detailId === "history") return [{label:"Cancel",action:"back"},{label:"Clear history",action:"clear-history"}];
        if (confirming) return [{label:"Keep in inbox",action:"back"},{label:"Confirm cancellation",action:"confirm-cancel"}];
        var result = [];
        if (detail.phase === "needs-destination") {
            result = (detail.candidates || []).map(sid => ({label:(snapshot.systems || []).find(s => s.id === sid)?.name || sid,action:"choose",system:sid}));
        }
        if (detail.phase === "needs-files") result.push({label:"Retry validation / import",action:"retry"});
        if (detail.phase === "scraping") result.push({label:"View artwork update",action:"scrape-job"});
        if (detail.phase === "complete" && !detail.folderOnly) result.push({label:detail.artwork_warning ? "Find artwork & info" : "Artwork & Game Info",action:"artwork"});
        result.push({label:"Back to inbox",action:"back"});
        if (["complete","cancelled","failed","importing"].indexOf(detail.phase) < 0)
            result.push({label:detail.phase === "receiving" || detail.phase === "offered" ? "Cancel sender session…" : "Cancel import…",action:"cancel"});
        return result;
    }
    function px(v) { return Math.round(v * uiScale); }
    ListModel { id: inboxRows }
    function sync() {
        if (!inboxRows) return;
        var oldIndex = 0;
        for (var i = 0; i < inboxRows.count; ++i) if (inboxRows.get(i).recordId === selectedId) oldIndex = i;
        for (var i = inboxRows.count - 1; i >= 0; --i)
            if (!rows.some(row => row.id === inboxRows.get(i).recordId)) inboxRows.remove(i);
        for (var i = 0; i < rows.length; ++i) {
            var found = -1;
            for (var j = i; j < inboxRows.count; ++j) if (inboxRows.get(j).recordId === rows[i].id) found = j;
            if (found < 0) inboxRows.insert(i,{recordId:rows[i].id});
            else if (found !== i) inboxRows.move(found,i,1);
        }
        if (!rows.some(row => row.id === selectedId)) selectedId = rows[Math.min(oldIndex, rows.length - 1)].id;
        Qt.callLater(() => {
            if (list.visible) list.positionViewAtIndex(Math.max(0, rows.findIndex(row => row.id === selectedId)), ListView.Contain);
        });
    }
    onRowsChanged: sync()
    Component.onCompleted: sync()
    function connectionHint() {
        if (!available || !snapshot.enabled) return "";
        var addresses = snapshot.addresses || [];
        if (!addresses.length) return "Waiting for a LAN address. Connect to Wi-Fi or Ethernet.";
        return "Manual sending · " + addresses.map(a => a.address + " (" + a.interface + ")").join(" · ")
            + " · Port " + (snapshot.port || 53317) + " · Encryption on";
    }
    function bytes(v) {
        if (v >= 1073741824) return (v / 1073741824).toFixed(2) + " GiB";
        return (v / 1048576).toFixed(1) + " MiB";
    }
    function targetHint() {
        if (!available) return "";
        return snapshot.targetError || "";
    }
    function progress(job) {
        if (job.phase === "complete") return 1;
        if (!hasByteProgress(job)) return 0;
        var size = Number(job.size), received = Number(job.received);
        if (!isFinite(size) || size <= 0 || !isFinite(received)) return 0;
        return Math.max(0, Math.min(1, received / size));
    }
    function percentage(job) {
        return Math.floor(progress(job) * 100) + "%";
    }
    function hasByteProgress(job) {
        return ["offered","receiving","failed","cancelled"].indexOf(job.phase) >= 0;
    }
    function working(job) {
        return ["validating","needs-scrape","scraping","importing"].indexOf(job.phase) >= 0;
    }
    function hint(job) {
        if (job.folderOnly && job.phase === "ready") return "Close this menu to save the files in " + job.destination + ".";
        if (job.folderOnly && job.phase === "complete") return "Files saved in " + job.destination + ".";
        if (job.phase === "needs-scrape" || job.phase === "scraping")
            return "Your game has been received. You can keep playing while we finish.";
        if (job.phase === "ready") return job.artwork_warning
            ? "Close this menu to update your library. You can find artwork later in Artwork & Game Info."
            : "Artwork and game info are ready. Close this menu to update your library.";
        if (job.phase === "complete") return "Your game is ready to play.";
        return "";
    }
    function progressColor(job, chosen) {
        if (job.phase === "failed") return chosen ? "#704024" : "#E8C08C";
        if (job.phase === "cancelled") return chosen ? "#665648" : "#8392A4";
        if (job.phase === "complete") return chosen ? "#285C43" : "#89C99B";
        return chosen ? "#244D53" : "#6FD5D1";
    }
    function stage(job) {
        var names = {offered:"Waiting for sender",receiving:"Receiving",validating:"Validating",
            "needs-destination":"Choose a destination folder","needs-files":"Needs attention",ready:"Ready to add to your library",
            "needs-scrape":"Finding artwork & info…",scraping:"Finding artwork & info…",
            importing:"Adding to your library…",complete:"Added to your library",failed:"Failed",cancelled:"Cancelled"};
        if (job.folderOnly && job.phase === "ready") return "Ready to transfer to folder";
        if (job.folderOnly && job.phase === "importing") return "Saving to folder…";
        if (job.folderOnly && job.phase === "complete") return "Transferred to folder";
        if (job.phase === "ready" && job.artwork_warning) return "Ready to add without artwork";
        return (names[job.phase] || job.phase) + (hasByteProgress(job) ? " · " + percentage(job) + " · " + bytes(job.received) + " / " + bytes(job.size) : "");
    }
    function move(direction) {
        if (busy || !available) return;
        if (detail) { choice = (choice + direction + choices.length) % choices.length; options.positionViewAtIndex(choice,ListView.Contain); return; }
        var i = rows.findIndex(r => r.id === selectedId);
        selectedId = rows[(i + direction + rows.length) % rows.length].id;
        list.positionViewAtIndex(rows.findIndex(r => r.id === selectedId),ListView.Contain);
    }
    function activate() {
        if (busy || !available) return;
        if (!detail) {
            if (selectedId === "receiver") command({action:snapshot.enabled ? "disable" : "enable"});
            else { detailId = selectedId; choice = 0; confirming = false; }
            return;
        }
        var option = choices[choice];
        if (!option) return;
        if (option.action === "back") { back(); return; }
        if (option.action === "artwork" || option.action === "scrape-job") {
            openScraping("",detail.system,option.action === "scrape-job" ? detail.scrape_job : "");
            detailId="";choice=0;return;
        }
        if (option.action === "clear-history") {
            command({action:"clear-history"});
            detailId = ""; choice = 0; return;
        }
        if (option.action === "cancel") { confirming = true; choice = 0; return; }
        if (option.action === "confirm-cancel") {
            var active = detail.phase === "receiving" || detail.phase === "offered";
            command(active ? {action:"cancel-session",batch:detail.batch} : {action:"cancel",id:detail.id});
            confirming = false; detailId = ""; choice = 0; return;
        }
        if (option.action === "choose") {
            command({action:"choose",id:detail.id,system:option.system});
            detailId = ""; choice = 0;
        }
        if (option.action === "retry") {
            command({action:"retry",id:detail.id});
            detailId = ""; choice = 0;
        }
    }
    function back() {
        if (confirming) { confirming = false; choice = 0; return true; }
        if (detailId.length) { detailId = ""; choice = 0; return true; }
        return false;
    }
    function dismiss() {
        if (!canDismiss) return;
        command({action:"dismiss-history",id:selected.id});
    }
    function clear() {
        if (!canClear) return;
        detailId = "history"; confirming = false; choice = 0;
    }
    onChoicesChanged: if (choice >= choices.length) choice = 0
    onJobsChanged: {
        if (detailId.length && detailId !== "history" && !jobs.some(j => j.id === detailId)) {
            detailId = ""; confirming = false; choice = 0;
        }
    }

    Column {
        id: header
        width: parent.width
        spacing: page.px(10)
        MenuText {
            width: parent.width
            text: !page.available ? "Transfer unavailable" : page.snapshot.enabled
                ? "Send to " + page.snapshot.alias + " · PIN " + page.snapshot.pin
                : "Receiving is off"
            font.pixelSize: page.px(26)
            wrapMode: Text.WordWrap
        }
        MenuText {
            width: parent.width
            text: page.error || (!page.available ? "Saved imports remain intact. Check the receiver service."
                : page.snapshot.enabled ? "LocalSend · " + Math.ceil(page.snapshot.seconds / 60) + " min remaining. You can close this menu and keep playing."
                : "Send games from LocalSend. Artwork and game info are added automatically when available.")
            color: page.error.length ? "#E8C08C" : "#BBC6D3"
            font.pixelSize: page.px(17)
            wrapMode: Text.WordWrap
        }
        MenuText {
            visible: page.snapshot.enabled && (page.snapshot.addresses || []).length > 0 && (page.snapshot.discoveryError || "").length > 0
            width: parent.width; text: page.snapshot.discoveryError || ""
            font.pixelSize: page.px(15); color:"#E8C08C"; wrapMode:Text.WordWrap
        }
        MenuText {
            objectName: "transferConnectionHint"
            visible: text.length > 0
            width: parent.width
            text: page.connectionHint()
            font.pixelSize: page.px(17); color: "#BBC6D3"; wrapMode: Text.WordWrap
        }
        MenuText {
            objectName: "transferTargetHint"
            visible: text.length > 0
            width: parent.width
            text: page.targetHint()
            font.pixelSize: page.px(15); color: "#E8C08C"; wrapMode: Text.WordWrap
        }
        MenuText {
            visible: page.detail !== null
            width: parent.width
            text: page.detailId === "history" ? "Clear finished transfer records? Imported games and pending transfers will be kept."
                : page.detail ? page.detail.name + "\n" + page.stage(page.detail) + "\n" + page.hint(page.detail) + "\n" + (page.detail.error || page.detail.artwork_warning || page.detail.evidence || "") : ""
            font.pixelSize:page.px(18); wrapMode:Text.WordWrap; color:"#DEC09A"
        }
        Rectangle {
            id: detailTrack
            objectName: "transferDetailProgress"
            visible: page.detail !== null && page.detailId !== "history" && (page.hasByteProgress(page.detail) || page.working(page.detail))
            width: parent.width; height: page.px(8); radius: height / 2
            readonly property real value: page.detail ? page.progress(page.detail) : 0
            readonly property bool indeterminate: page.detail !== null && page.working(page.detail)
            color: "#394758"; clip: true
            Rectangle {
                id: detailFill
                width: parent.width * (detailTrack.indeterminate ? .28 : detailTrack.value); height: parent.height; radius: height / 2
                color: page.detail ? page.progressColor(page.detail, false) : "#6FD5D1"
                Behavior on width { enabled: !detailTrack.indeterminate; NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
                NumberAnimation on x {
                    from: -detailFill.width; to: detailTrack.width; duration: 1200; loops: Animation.Infinite
                    running: detailTrack.visible && page.visible && detailTrack.indeterminate
                    onStopped: detailFill.x = 0
                }
            }
        }
        MenuText {
            visible:page.confirming; width:parent.width
            text:"Cancel this import? Its staged files will be removed. Games already in your library stay in place."
            font.pixelSize:page.px(17); wrapMode:Text.WordWrap; color:"#E8C08C"
        }
    }
    ListView {
        id: list
        visible: page.detail === null && page.available
        anchors { left:parent.left; right:parent.right; top:header.bottom; bottom:parent.bottom; topMargin:page.px(18) }
        model: inboxRows
        spacing: page.px(8); clip:true
        remove: Transition {
            ParallelAnimation {
                NumberAnimation { property:"dismissOffset"; to:page.px(96); duration:180; easing.type:Easing.OutCubic }
                NumberAnimation { property:"opacity"; to:0; duration:180; easing.type:Easing.OutCubic }
            }
        }
        removeDisplaced: Transition { NumberAnimation { properties:"x,y"; duration:180; easing.type:Easing.OutCubic } }
        delegate: Rectangle {
            id: inboxRow
            required property string recordId
            objectName:"transfer-row:" + recordId
            property var payload: ({})
            function updateRecord() {
                var current = page.rows.find(row => row.id === recordId);
                if (current) payload = current;
            }
            onRecordIdChanged: updateRecord()
            Component.onCompleted: updateRecord()
            Connections {
                target: page
                function onRowsChanged() { inboxRow.updateRecord(); }
            }
            property real dismissOffset: 0
            property bool retiring: false
            ListView.onRemove: retiring = true
            transform: Translate { x: inboxRow.dismissOffset }
            readonly property bool chosen: recordId === page.selectedId
            readonly property bool actionRow: recordId === "receiver"
            width:list.width; height:Math.max(page.px(actionRow ? 72 : 102), rowContent.implicitHeight + page.px(24)); radius:page.px(12)
            color:chosen ? "#EAC28D" : "#1D2634"
            Column {
                id: rowContent
                x:page.px(14); y:page.px(12); width:parent.width-page.px(28); spacing:page.px(6)
                MenuText { width:parent.width; text:inboxRow.payload.name || ""; elide:Text.ElideRight; font.pixelSize:page.px(21); color:parent.parent.chosen ? "#211E1B" : "#ECEEF1" }
                MenuText { objectName:"transferStage-" + inboxRow.recordId; visible:!inboxRow.actionRow; width:parent.width; text:inboxRow.actionRow ? "" : page.stage(inboxRow.payload); wrapMode:Text.WordWrap; font.pixelSize:page.px(16); color:parent.parent.chosen ? "#473627" : "#BBC6D3" }
                MenuText { objectName:"transferHint-" + inboxRow.recordId; visible:!inboxRow.actionRow && text.length > 0; width:parent.width; text:inboxRow.actionRow ? "" : page.hint(inboxRow.payload); wrapMode:Text.WordWrap; font.pixelSize:page.px(16); color:inboxRow.chosen ? "#473627" : "#BBC6D3" }
                Rectangle {
                    id: rowTrack
                    objectName: "transferProgress-" + inboxRow.recordId
                    visible: !inboxRow.actionRow && (page.hasByteProgress(inboxRow.payload) || page.working(inboxRow.payload))
                    width: parent.width; height: page.px(8); radius: height / 2
                    readonly property real value: page.progress(inboxRow.payload)
                    readonly property bool indeterminate: page.working(inboxRow.payload)
                    color: inboxRow.chosen ? "#AE8758" : "#394758"; clip: true
                    Rectangle {
                        id: rowFill
                        width: parent.width * (rowTrack.indeterminate ? .28 : rowTrack.value); height: parent.height; radius: height / 2
                        color: page.progressColor(inboxRow.payload, inboxRow.chosen)
                        Behavior on width { enabled: !rowTrack.indeterminate; NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
                        NumberAnimation on x {
                            from: -rowFill.width; to: rowTrack.width; duration: 1200; loops: Animation.Infinite
                            running: rowTrack.visible && page.visible && rowTrack.indeterminate
                            onStopped: rowFill.x = 0
                        }
                    }
                }
            }
            MouseArea { anchors.fill:parent; enabled:!inboxRow.retiring; onClicked:{ page.selectedId=inboxRow.recordId; page.activate(); } }
        }
    }
    ListView {
        id: options
        visible: page.detail !== null
        anchors { left:parent.left; right:parent.right; top:header.bottom; bottom:parent.bottom; topMargin:page.px(18) }
        model:page.choices; spacing:page.px(8); clip:true
        delegate:Rectangle {
            required property var modelData
            required property int index
            width:options.width; height:page.px(64); radius:page.px(12)
            color:index === page.choice ? "#EAC28D" : "#1D2634"
            MenuText { anchors.centerIn:parent; width:parent.width-page.px(28); text:modelData.label; font.pixelSize:page.px(20); elide:Text.ElideRight; color:index === page.choice ? "#211E1B" : "#ECEEF1" }
            MouseArea { anchors.fill:parent; onClicked:{ page.choice=index; page.activate(); } }
        }
    }
}
