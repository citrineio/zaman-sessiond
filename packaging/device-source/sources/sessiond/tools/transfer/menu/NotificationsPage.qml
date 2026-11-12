import QtQuick
import QtQml.Models

Item {
    id: page
    signal openTransaction(string jobId)
    signal openTransfer()
    signal openScrape(string jobId)
    property var scrapes: []
    readonly property var liveScrapes: scrapes.filter(j => !j.automatic && ["queued","running","needs-input","waiting-library","publishing"].indexOf(j.phase)>=0).map(j => ({
        id:"scrape:"+j.id,scrape:true,title:"Artwork & Game Info · "+j.folders.join(", "),body:j.stage,
        source:"Zaman Artwork & Game Info",timestamp:j.created,target:"scrape-pending:"+j.id,urgency:1,dismissed:false
    }))
    property var imports: []
    readonly property var liveImports: imports.filter(j => ["complete","cancelled","failed"].indexOf(j.phase) < 0).map(j => ({
        id:"transfer:" + j.id, transfer:true, title:j.name, body:j.error || j.evidence || j.phase,
        category:"transfer", source:"Zaman Transfer · " + j.phase, timestamp:j.created,
        urgency:1, dismissed:false, target:"transfer-pending"
    }))
    property real uiScale: 1
    property var client
    property var transactions: []
    property string selectedId: ""
    property var detail: null
    property var clearIds: []
    property bool confirming: false
    property bool finishingRemoval: false
    property int confirmSelected: 0
    readonly property bool blocked: confirming || detail !== null
    readonly property var liveTransactions: transactions.filter(job => ["queued", "waiting", "running", "cancelling"].indexOf(job.phase) >= 0).map(job => ({
        id: "transaction:" + job.id, transaction: true, jobId: job.id,
        title: job.name + (job.operation === "install" ? " · Installing" : " · Removing"),
        body: job.stage || job.phase, phase: job.phase, percent: job.percent,
        category: "install", source: "Dukkan", timestamp: job.created,
        urgency: 1, dismissed: false, target: "dukkan-transaction:" + job.id
    }))
    readonly property var records: liveImports.concat(liveScrapes).concat(liveTransactions).concat(client ? client.records.filter(record => !record.dismissed) : [])
    readonly property bool hasItems: records.length > 0
    readonly property var selected: records.find(r => r.id === selectedId) || null
    function px(n) { return Math.round(n * uiScale); }
    ListModel { id: visibleRecords }
    function sync() {
        if (!records.length) {
            if (visibleRecords.count > 0) { finishingRemoval = false; finishingRemoval = true; }
            for (var emptyIndex = visibleRecords.count - 1; emptyIndex >= 0; --emptyIndex) visibleRecords.remove(emptyIndex);
            selectedId = "";
            detail = null;
            return;
        }
        // Preserve delegates and the nearest selected neighbor during dismissal.
        var oldIndex = 0;
        for (var i = 0; i < visibleRecords.count; ++i) if (visibleRecords.get(i).recordId === selectedId) oldIndex = i;
        for (var i = visibleRecords.count - 1; i >= 0; --i) {
            if (!records.some(record => record.id === visibleRecords.get(i).recordId)) visibleRecords.remove(i);
        }
        for (var i = 0; i < records.length; ++i) {
            var found = -1;
            for (var j = i; j < visibleRecords.count; ++j) if (visibleRecords.get(j).recordId === records[i].id) found = j;
            if (found < 0) visibleRecords.insert(i, {recordId:records[i].id});
            else {
                if (found !== i) visibleRecords.move(found, i, 1);
            }
        }
        // IDs keep focus anchored when newer records arrive above it.
        if (!records.some(r => r.id === selectedId)) selectedId = records.length ? records[Math.min(oldIndex, records.length - 1)].id : "";
        Qt.callLater(() => {
            if (visible) history.positionViewAtIndex(Math.max(0, records.findIndex(r => r.id === selectedId)), ListView.Contain);
        });
    }
    onRecordsChanged: sync()
    onFinishingRemovalChanged: if (finishingRemoval) emptyDelay.restart()
    Timer { id:emptyDelay; interval:240; onTriggered:page.finishingRemoval=false }
    function move(direction) {
        if (confirming) { confirmSelected = (confirmSelected + direction + 2) % 2; return; }
        if (detail) { details.contentY = Math.max(0, Math.min(details.contentHeight - details.height, details.contentY + direction * px(70))); return; }
        if (!records.length) return;
        var index = records.findIndex(r => r.id === selectedId);
        selectedId = records[(index + direction + records.length) % records.length].id;
        sync();
    }
    function activate() {
        if (confirming) {
            if (confirmSelected === 1) client.send({action:"clear", ids:clearIds});
            confirming = false; clearIds = []; return;
        }
        var record = detail || selected;
        if (record && /^dukkan-transaction:[a-f0-9]{32}$/.test(record.target || "")) {
            openTransaction(record.target.slice(19)); return;
        }
        if (record && record.target === "transfer-pending") { openTransfer(); return; }
        if (record && /^scrape-pending:[a-f0-9]{32}$/.test(record.target || "")) { openScrape(record.target.slice(15));return; }
        if (!detail && selected) detail = Object.assign({}, selected);
    }
    function back() {
        if (confirming) { confirming = false; clearIds = []; return true; }
        if (detail) { detail = null; return true; }
        return false;
    }
    function dismiss() {
        if (!confirming && !detail && selected && !selected.transaction && !selected.transfer && !selected.scrape && client.available) client.send({action:"dismiss", id:selectedId});
    }
    function clear() {
        if (blocked || !client.records.length || !client.available) return;
        clearIds = client.records.map(r => r.id);
        confirmSelected = 0;
        confirming = true;
    }
    function stamp(record) { return Qt.formatDateTime(new Date(record.timestamp * 1000), "ddd h:mm AP"); }

    ListView {
        id: history
        objectName: "notificationHistory"
        anchors.fill: parent
        visible: page.hasItems && !page.blocked || page.finishingRemoval && !page.blocked
        model: visibleRecords
        add: Transition { NumberAnimation { property:"opacity"; from:0; to:1; duration:120 } }
        remove: Transition {
            ParallelAnimation {
                NumberAnimation { property:"dismissOffset"; to:page.px(96); duration:180; easing.type:Easing.OutCubic }
                NumberAnimation { property:"opacity"; to:0; duration:180; easing.type:Easing.OutCubic }
            }
        }
        removeDisplaced: Transition { NumberAnimation { properties:"x,y"; duration:180; easing.type:Easing.OutCubic } }
        spacing: page.px(7)
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        delegate: Rectangle {
            id: notificationRow
            required property string recordId
            property var payload: ({})
            objectName: "notification-row:" + recordId
            // ListModel carries only the stable string ID. Keep the actual
            // record as a JS object, including during a removal animation.
            function updateRecord() {
                var current = page.records.find(record => record.id === recordId);
                if (current) payload = current;
            }
            onRecordIdChanged: updateRecord()
            Component.onCompleted: updateRecord()
            Connections {
                target: page
                function onRecordsChanged() { notificationRow.updateRecord(); }
            }
            // Keep the horizontal exit separate from ListView's row placement.
            property real dismissOffset: 0
            property bool retiring: false
            ListView.onRemove: retiring = true
            transform: Translate { x: notificationRow.dismissOffset }
            readonly property bool chosen: recordId === page.selectedId
            width: history.width
            height: page.px(notificationRow.payload.transaction ? 122 : 96)
            radius: page.px(12)
            color: notificationRow.chosen ? "#EAC28D" : "#1D2634"
            MenuIcon {
                x: page.px(14); y: page.px(20)
                width: page.px(29); height: width
                name: notificationRow.payload.category || "system"
                ink: parent.chosen ? "#30271E" : "#DEC09A"
            }
            Column {
                x: page.px(58); y: page.px(12)
                width: parent.width - page.px(76)
                spacing: page.px(4)
                MenuText {
                    width: parent.width
                    objectName: "notification-title:" + notificationRow.recordId
                    text: notificationRow.payload.title || "Notification"
                    font.pixelSize: page.px(22)
                    elide: Text.ElideRight
                    color: notificationRow.chosen ? "#211E1B" : "#ECEEF1"
                }
                MenuText {
                    width: parent.width
                    text: notificationRow.payload.transaction ? notificationRow.payload.phase + " · " + notificationRow.payload.body : (notificationRow.payload.urgency === 2 ? "Critical · " : "") + page.stamp(notificationRow.payload) + (notificationRow.payload.source ? " · " + notificationRow.payload.source : "")
                    font.pixelSize: page.px(14)
                    elide: Text.ElideRight
                    color: notificationRow.chosen ? "#473627" : "#BBC6D3"
                }
                MenuText {
                    visible: notificationRow.payload.transaction === true
                    objectName: "notification-progress:" + notificationRow.recordId
                    width: parent.width
                    text: typeof notificationRow.payload.percent === "number" ? Math.round(notificationRow.payload.percent) + "% · current step" : "Waiting for progress…"
                    font.pixelSize: page.px(14)
                    color: notificationRow.chosen ? "#473627" : "#BBC6D3"
                }
                Rectangle {
                    id: progressTrack
                    visible: notificationRow.payload.transaction === true
                    width: parent.width; height: page.px(6); radius: height / 2
                    color: notificationRow.chosen ? "#40503922" : "#35465C"
                    clip: true
                    Rectangle {
                        id: progressFill
                        readonly property bool determinate: typeof notificationRow.payload.percent === "number"
                        width: parent.width * (determinate ? Math.max(0, Math.min(100, notificationRow.payload.percent)) / 100 : .3)
                        height: parent.height; radius: height / 2
                        color: notificationRow.chosen ? "#614930" : "#88A8C9"
                        NumberAnimation on x {
                            running: page.visible && notificationRow.payload.transaction === true && !progressFill.determinate
                            from: -progressFill.width; to: progressTrack.width
                            duration: 1200; loops: Animation.Infinite
                        }
                        onDeterminateChanged: if (determinate) x = 0
                    }
                }
            }
            MouseArea {
                anchors.fill: parent
                enabled: !notificationRow.retiring
                onClicked: { page.selectedId = notificationRow.payload.id; page.activate(); }
            }
        }
    }
    Column {
        objectName: "notificationEmpty"
        visible: !page.hasItems && !page.blocked && !page.finishingRemoval
        width: parent.width
        spacing: page.px(15)
        MenuIcon { name:"notifications"; width:page.px(42); height:width; ink:"#DEC09A" }
        MenuText { text:"You're all caught up"; font.pixelSize:page.px(26) }
        MenuText { width:parent.width; text:page.client && page.client.error ? page.client.error : "System updates appear here."; wrapMode:Text.WordWrap; color:"#BBC6D3"; font.pixelSize:page.px(18) }
    }
    Flickable {
        id: details
        anchors.fill: parent
        visible: page.detail !== null && !page.confirming
        contentHeight: detailContent.height
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        Column {
            id: detailContent
            width: parent.width
            spacing: page.px(18)
            MenuText { width:parent.width; text:page.detail ? page.detail.title : ""; wrapMode:Text.Wrap; font.pixelSize:page.px(28) }
            MenuText { width:parent.width; text:page.detail ? page.stamp(page.detail) + " · " + page.detail.source : ""; wrapMode:Text.Wrap; font.pixelSize:page.px(16); color:"#BBC6D3" }
            MenuText { width:parent.width; text:page.detail ? page.detail.body : ""; wrapMode:Text.Wrap; font.pixelSize:page.px(21) }
            MenuText { visible:page.detail !== null && page.detail.target === "transfer-pending"; text:"Open Transfer to review pending files."; width:parent.width; wrapMode:Text.Wrap; font.pixelSize:page.px(18); color:"#DEC09A" }
        }
    }
    Column {
        visible: page.confirming
        width: parent.width
        spacing: page.px(18)
        MenuText { text:"Clear notification history?"; font.pixelSize:page.px(26) }
        MenuText { width:parent.width; text:"Downloads and pending imports will continue. New notifications will stay."; wrapMode:Text.WordWrap; color:"#BBC6D3"; font.pixelSize:page.px(18) }
        Repeater {
            model: ["Cancel", "Clear history"]
            delegate: Rectangle {
                required property string modelData
                required property int index
                width: page.width; height:page.px(60); radius:page.px(12)
                color:page.confirmSelected === index ? "#EAC28D" : "#1D2634"
                MenuText { anchors.centerIn:parent; text:modelData; font.pixelSize:page.px(22); color:page.confirmSelected === index ? "#211E1B" : "#ECEEF1" }
                MouseArea { anchors.fill:parent; onClicked:{ page.confirmSelected = index; page.activate(); } }
            }
        }
    }
}
