import QtQuick
import QtQml.Models

Item {
    id: page
    property real uiScale: 1
    property var snapshot: ({available:false,folders:[],jobs:[]})
    property bool available: false
    property bool busy: false
    property string error: ""
    property string view: "setup"
    property string folderId: "all"
    property string importId: ""
    property string importSystem: ""
    property string jobId: ""
    property int mode: 0
    property bool review: false
    property int selected: 1
    property var knownJobs: []
    property bool pendingStart: false
    property string editingPromptId: ""
    property bool scanPending: true
    property string syncedView: ""
    property var syncedIds: []
    property bool animateActivityRemovals: false
    signal command(var request)
    signal closeRequested()
    readonly property var jobs: snapshot.jobs || []
    readonly property var activeJobs: jobs.filter(j => ["queued","running","needs-input","waiting-library","publishing"].indexOf(j.phase) >= 0)
    readonly property var job: jobs.find(j => j.id === jobId) || null
    readonly property var prompt: job ? job.prompt : null
    readonly property string promptKey: jobId + ":" + (prompt ? prompt.id : "")
    readonly property string jobPhase: job ? job.phase : ""
    readonly property bool editingText: view === "answer"
    readonly property bool canDismiss: available && !busy && view === "activity" && !!rows[selected] && !!rows[selected].job && ["complete","failed","cancelled","interrupted"].indexOf(rows[selected].phase) >= 0
    readonly property bool canClear: available && !busy && view === "activity" && (snapshot.historyCount || 0) > 0
    readonly property var modes: ["Fill in missing info","Replace artwork & info","Use saved info · offline"]
    readonly property var rows: {
        if (view === "folders") return [{id:"folder-all",label:"All collections",detail:"Every supported game folder",folder:"all"}].concat((snapshot.folders || []).map(f => ({id:"folder-"+f.id,label:f.name,detail:f.reason || "",folder:f.id,disabled:!f.supported})));
        if (view === "options") return modes.map((label,index) => ({id:"mode-"+index,label:label,detail:index === 0 ? "Recommended · keep current info and fill in the gaps" : index === 1 ? "Find fresh results for every game" : "Use previous downloads without internet",action:"mode",value:index,checked:mode === index})).concat([
            {id:"review",label:"Choose game matches myself",detail:mode === 2 ? "Not used offline" : review ? "On · review each result before using it" : "Off · use the suggested match",action:"review",disabled:mode === 2},
            {id:"done",label:"Done",action:"setup",kind:"primary"}]);
        if (view === "confirm") return [{id:"replace",label:"Replace artwork & info",action:"start-confirmed",kind:"primary"},{id:"keep",label:"Keep current info",action:"setup"}];
        if (view === "clear") return [{id:"keep",label:"Keep history",action:"activity",kind:"primary"},{id:"clear",label:"Clear history",action:"confirm-clear"}];
        if (view === "cancel") return [{id:"keep",label:"Keep updating",action:"back-job",kind:"primary"},{id:"stop",label:"Stop update",action:"cancel-job"}];
        if (view === "log" || view === "answer") return [];
        if (view === "activity") return jobs.map(j => ({id:j.id,label:collectionNames(j.folders),detail:j.stage,job:j.id,phase:j.phase,action:"job"})).concat([{id:"back",label:"Back to Artwork & Game Info",action:"setup",kind:"quiet"}]);
        if (view === "job") {
            var result=[];
            if (prompt && prompt.choices && prompt.choices.length) result = prompt.choices.map(c => ({id:"answer-"+c.value,label:c.label,detail:c.detail || "",action:"answer",value:c.value}));
            else if (prompt && prompt.text) result.push({id:"text",label:"Enter an answer…",action:"text",kind:"primary"});
            else if (job && ["failed","interrupted","cancelled"].indexOf(job.phase) >= 0) result.push({id:"retry",label:"Try artwork update again",action:"retry",kind:"primary"});
            else result.push({id:"play",label:jobPhase === "complete" ? "Go to library" : "Keep playing",action:"close",kind:"primary"});
            result.push({id:"details",label:"Details",action:"log",kind:"quiet"});
            if (job && ["queued","running","needs-input","waiting-library"].indexOf(job.phase) >= 0) result.push({id:"stop",label:"Stop update…",action:"cancel",kind:"quiet"});
            result.push({id:"another",label:"Back to Artwork & Game Info",action:"setup",kind:"quiet"});
            return result;
        }
        return [{id:"collection",label:collectionNames([folderId]),detail:"Collection",action:"folder",kind:"collection",disabled:activeJobs.length > 0},
            {id:"start",label:activeJobs.length ? "View update progress" : "Find artwork & info",action:activeJobs.length ? "active-job" : "start",kind:"primary",disabled:!snapshot.available && !activeJobs.length},
            {id:"options",label:"Options",detail:["Missing info only","Refresh all games","Offline"][mode] + (review && mode !== 2 ? " · Review matches" : ""),action:"options",kind:"options",disabled:activeJobs.length > 0},
            {id:"activity",label:"View activity",action:"activity",kind:"quiet"}];
    }
    function px(n) { return Math.round(n * uiScale); }
    function collectionNames(ids) {
        return (ids || []).map(id => id === "all" ? "All collections" : (snapshot.folders || []).find(f => f.id === id)?.name || id).join(", ");
    }
    function syncRows() {
        if (!activityRows) return;
        var sameActivity = view === "activity" && syncedView === "activity";
        animateActivityRemovals = sameActivity;
        var previous = syncedIds[selected];
        var oldIndex = selected;
        var ids = rows.map(row => view + ":" + row.id);
        for (var i = activityRows.count - 1; i >= 0; --i)
            if (ids.indexOf(activityRows.get(i).recordId) < 0) activityRows.remove(i);
        for (var i = 0; i < ids.length; ++i) {
            var found = -1;
            for (var j = i; j < activityRows.count; ++j) if (activityRows.get(j).recordId === ids[i]) found = j;
            if (found < 0) activityRows.insert(i,{recordId:ids[i]});
            else if (found !== i) activityRows.move(found,i,1);
        }
        if (sameActivity) selected = ids.indexOf(previous) >= 0 ? ids.indexOf(previous) : Math.min(oldIndex,Math.max(0,ids.length - 1));
        else if (selected >= rows.length) selected = 0;
        syncedIds = ids;
        syncedView = view;
        Qt.callLater(() => {
            if (list.visible && rows.length) list.positionViewAtIndex(selected,ListView.Contain);
        });
    }
    function resultSummary() {
        var results=job ? job.results || [] : [];
        var failed=results.filter(r=>!r.ok).length;
        if (failed) return failed + (failed === 1 ? " collection needs attention. " : " collections need attention. ") + "Open Details to see what happened.";
        var total=results.reduce((sum,r)=>sum+(r.games || 0),0);
        var missing=results.reduce((sum,r)=>sum+(r.missing || 0),0);
        return total ? total + " games checked" + (missing ? " · " + missing + " without matching info" : " · Artwork and info are ready") : "You can try again whenever you like.";
    }
    function open(importItem, system, existingJob) {
        importId="";importSystem="";
        if (system) folderId=system;
        jobId=existingJob || "";view=jobId.length ? "job" : "setup";selected=jobId.length ? 0 : 1;
    }
    function move(direction) {
        if (busy) return;
        if (editingText) { answerKeys.navigate(0,direction);return; }
        if (view === "log") { logView.contentY=Math.max(0,Math.min(logView.contentHeight-logView.height,logView.contentY+direction*px(70)));return; }
        selected=(selected+direction+rows.length)%Math.max(1,rows.length);
        list.positionViewAtIndex(selected,ListView.Contain);
    }
    function horizontal(direction) { if (editingText) answerKeys.navigate(direction,0); }
    function cycleKeyboard() { if (editingText) answerKeys.cycleView(); }
    function start() {
        knownJobs=jobs.map(j=>j.id);pendingStart=true;
        command({action:"scrape-start",folder:folderId,mode:["cached","refresh","offline"][mode],review:review && mode !== 2});
    }
    function activate() {
        if (busy || !available) return;
        if (editingText) { answerKeys.activate();return; }
        var row=rows[selected];if (!row || row.disabled) return;
        if (row.folder) { folderId=row.folder;view="setup";selected=1;return; }
        if (row.action === "folder") { view="folders";selected=Math.max(0,rows.findIndex(r=>r.folder === folderId)); }
        else if (row.action === "options") { view="options";selected=mode; }
        else if (row.action === "mode") mode=row.value;
        else if (row.action === "review") review=!review;
        else if (row.action === "start") { if (mode === 1) { view="confirm";selected=1; } else start(); }
        else if (row.action === "start-confirmed") start();
        else if (row.action === "active-job") { jobId=activeJobs[0].id;view="job";selected=0; }
        else if (row.action === "job") { jobId=row.job;view="job";selected=0; }
        else if (row.action === "answer") submitAnswer(row.value);
        else if (row.action === "text") { answerKeys.reset();editingPromptId=prompt.id;view="answer"; }
        else if (row.action === "log") { view="log";selected=0; }
        else if (row.action === "cancel") { view="cancel";selected=0; }
        else if (row.action === "cancel-job") { command({action:"scrape-cancel",id:jobId});view="job";selected=0; }
        else if (row.action === "retry") { knownJobs=jobs.map(j=>j.id);pendingStart=true;command({action:"scrape-retry",id:jobId}); }
        else if (row.action === "confirm-clear") { command({action:"scrape-clear"});view="activity";selected=0; }
        else if (row.action === "back-job") { view="job";selected=0; }
        else if (row.action === "setup") { view="setup";selected=1; }
        else if (row.action === "activity") { view="activity";selected=0; }
        else if (row.action === "close") closeRequested();
    }
    function submitAnswer(value) {
        if (!prompt || busy || (editingText && prompt.id !== editingPromptId)) return;
        command({action:"scrape-answer",id:jobId,prompt:prompt.id,answer:value});
        answerKeys.reset();view="job";selected=0;
    }
    function back() {
        if (view === "setup") return false;
        if (view === "answer" || view === "cancel" || view === "log") { view="job";selected=0; }
        else if (view === "clear") { view="activity";selected=0; }
        else { view="setup";selected=1; }
        answerKeys.reset();return true;
    }
    function dismiss() { if (canDismiss) command({action:"scrape-dismiss",id:rows[selected].job}); }
    function clear() { if (canClear) { view="clear";selected=0; } }
    function physicalKey(event) {
        if (!editingText) return false;
        if (event.key === Qt.Key_Escape || event.key === Qt.Key_Back) { if (!event.isAutoRepeat) back(); }
        else if ([Qt.Key_Up,Qt.Key_Down,Qt.Key_Left,Qt.Key_Right].indexOf(event.key) >= 0) return false;
        else if (event.key === Qt.Key_Backspace || event.key === Qt.Key_Delete) answerKeys.erase();
        else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) { if (!event.isAutoRepeat) submitAnswer(answerKeys.secret); }
        else if (!(event.modifiers & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier))) answerKeys.append(event.text);
        return true;
    }
    onRowsChanged: syncRows()
    Component.onCompleted: syncRows()
    onPromptKeyChanged: { if (view === "job") selected=0;if (view === "answer" && (!prompt || prompt.id !== editingPromptId)) { view="job";answerKeys.reset(); } }
    onJobPhaseChanged: if (view === "job" && ["complete","failed","cancelled","interrupted"].indexOf(jobPhase) >= 0) selected=0
    onJobsChanged: {
        if (pendingStart) {
            var fresh=jobs.find(j=>knownJobs.indexOf(j.id)<0 && !j.automatic);
            if (fresh) { jobId=fresh.id;view="job";selected=0;pendingStart=false; }
        }
        if (jobId.length && !job) {
            jobId="";
            if (["job","answer","cancel","log"].indexOf(view) >= 0) { view="setup";selected=1; }
        }
    }
    onErrorChanged: if (error.length) pendingStart=false
    onVisibleChanged: if (visible) scanPending=true
    ListModel { id: activityRows }
    Timer {
        interval:1;running:page.visible && page.available && !page.busy && page.scanPending
        onTriggered:{ page.scanPending=false;page.command({action:"scrape-scan"}); }
    }
    Column {
        id:header
        width:parent.width;spacing:page.px(12)
        MenuText {
            objectName:"artworkHeading"
            width:parent.width;wrapMode:Text.WordWrap;font.pixelSize:page.px(page.view === "setup" ? 18 : 23)
            text:page.view === "setup" ? "Find covers, screenshots and descriptions for your games."
                : page.view === "options" ? "Update options" : page.view === "folders" ? "Choose a collection"
                : page.view === "confirm" ? "Replace existing artwork and info?" : page.view === "activity" ? "Your updates"
                : page.view === "clear" ? "Clear update history?" : page.view === "cancel" ? "Stop this update?"
                : page.view === "log" ? "Update details" : page.prompt ? (page.prompt.kind === "match" ? "Which game is this?" : "Your choice")
                : page.job ? page.job.stage : "Update unavailable"
        }
        MenuText {
            width:parent.width;wrapMode:Text.WordWrap;font.pixelSize:page.px(16);color:page.error.length ? "#E8C08C" : "#BBC6D3"
            visible:text.length > 0
            text:page.error || page.snapshot.error || (page.view === "setup" ? ""
                : page.view === "options" ? "Keep your current info and fill in the gaps, or choose a different approach."
                : page.view === "folders" ? "Your current game folders. Unsupported collections are shown with a reason."
                : page.view === "confirm" ? "Fresh results will replace the current artwork and descriptions in " + page.collectionNames([page.folderId]) + ". Games and saves stay in place."
                : page.view === "clear" ? "Finished records will be removed. Artwork, games and current updates stay in place."
                : page.view === "cancel" ? "Completed collections keep their new info. Your games and saves will not be removed."
                : page.prompt && (page.view === "job" || page.view === "answer") ? page.prompt.question
                : page.view === "log" ? "Up/down scroll · B returns"
                : page.view === "activity" ? page.jobs.length ? "Updates continue after you close this menu." : "Your updates will appear here."
                : page.job ? page.collectionNames(page.job.folders)+(page.job.automatic ? " · New transfer" : "")+"\n"+(["complete","failed","cancelled","interrupted"].indexOf(page.job.phase) < 0 ? "You can keep playing while this runs." : page.resultSummary()) : "")
        }
        Rectangle {
            objectName:"artworkProgress"
            visible:page.view === "job" && page.job !== null
            readonly property real value:page.job ? Math.max(0,Math.min(1,page.job.progress || 0)) : 0
            width:parent.width;height:page.px(8);radius:height/2;color:"#394758";clip:true
            Rectangle { width:parent.width*parent.value;height:parent.height;radius:height/2;color:"#6FD5D1" }
        }
        MenuText {
            width:parent.width;wrapMode:Text.WordWrap;font.pixelSize:page.px(15);color:"#BBC6D3"
            visible:page.view === "job" && page.job !== null && text.length > 0
            text:page.job && page.job.current ? page.job.current : ""
        }
    }
    PasswordKeyboard {
        id:answerKeys
        visible:page.editingText
        anchors { left:parent.left;right:parent.right;top:header.bottom;topMargin:page.px(14) }
        uiScale:page.uiScale;maxLength:256;masked:page.prompt ? !!page.prompt.masked : false
        title:"Answer artwork service";submitLabel:"Send"
        onSubmitted:value=>page.submitAnswer(value)
    }
    ListView {
        id:list;visible:!page.editingText && page.view !== "log"
        anchors { left:parent.left;right:parent.right;top:header.bottom;bottom:parent.bottom;topMargin:page.px(22) }
        model:activityRows;spacing:page.px(10);clip:true
        remove: Transition {
            enabled:page.animateActivityRemovals
            ParallelAnimation {
                NumberAnimation { property:"dismissOffset"; to:page.px(96); duration:180; easing.type:Easing.OutCubic }
                NumberAnimation { property:"opacity"; to:0; duration:180; easing.type:Easing.OutCubic }
            }
        }
        removeDisplaced: Transition {
            enabled:page.animateActivityRemovals
            NumberAnimation { properties:"x,y"; duration:180; easing.type:Easing.OutCubic }
        }
        delegate:Item {
            id:rowItem
            required property string recordId
            required property int index
            objectName:"artwork-row:" + recordId
            property var modelData: ({})
            function updateRecord() {
                var current = page.rows.find(row => page.view + ":" + row.id === recordId);
                if (current) modelData = current;
            }
            onRecordIdChanged: updateRecord()
            Component.onCompleted: updateRecord()
            Connections {
                target: page
                function onRowsChanged() { rowItem.updateRecord(); }
            }
            property real dismissOffset: 0
            property bool retiring: false
            ListView.onRemove: retiring = true
            transform: Translate { x: rowItem.dismissOffset }
            readonly property bool primary:modelData.kind === "primary"
            readonly property bool chosen:index === page.selected
            readonly property bool quiet:modelData.kind === "quiet" || modelData.kind === "options" || modelData.kind === "collection"
            width:list.width;height:page.px(primary ? (page.view === "setup" ? 116 : 82) : (modelData.kind === "options" || modelData.kind === "collection") ? 63 : modelData.kind === "quiet" ? 52 : modelData.detail ? 90 : 66)
            Rectangle {
                objectName:"artworkSurface-" + modelData.id
                width:parent.width;height:page.px(rowItem.primary ? 82 : rowItem.height/page.uiScale);radius:page.px(rowItem.quiet ? 8 : 13)
                color:rowItem.chosen ? "#EAC28D" : rowItem.quiet ? "transparent" : "#1D2A3A"
                border.width:rowItem.chosen ? page.px(2) : 0
                border.color:rowItem.chosen ? "#F3D4A5" : "#3A4859"
                opacity:modelData.disabled ? .45 : 1
                Column {
                    x:page.px(17);anchors.verticalCenter:parent.verticalCenter;width:parent.width-page.px(62);spacing:page.px(6)
                    MenuText {
                        visible:modelData.kind === "collection";text:"Collection";font.pixelSize:page.px(17);color:rowItem.chosen ? "#211E1B" : "#ECEEF1"
                    }
                    MenuText {
                        objectName:modelData.kind === "primary" ? "artworkPrimaryAction" : ""
                        width:parent.width;text:(modelData.checked ? "✓  " : "")+modelData.label;elide:Text.ElideRight
                        font.pixelSize:page.px(rowItem.primary ? 23 : modelData.kind === "collection" ? 14 : rowItem.quiet ? 17 : 20);font.bold:rowItem.primary
                        color:rowItem.chosen ? (modelData.kind === "collection" ? "#473627" : "#211E1B") : modelData.kind === "collection" ? "#BBC6D3" : "#ECEEF1"
                    }
                    MenuText {
                        visible:!!modelData.detail && modelData.kind !== "collection";width:parent.width
                        text:modelData.detail || "";elide:Text.ElideRight;font.pixelSize:page.px(14);color:rowItem.chosen ? "#473627" : "#BBC6D3"
                    }
                }
                MenuText {
                    anchors { right:parent.right;rightMargin:page.px(18);verticalCenter:parent.verticalCenter }
                    text:rowItem.primary ? "→" : modelData.kind === "collection" ? "⌄" : rowItem.quiet ? "›" : ""
                    font.pixelSize:page.px(24);color:rowItem.chosen ? "#30271E" : "#BBC6D3"
                }
            }
            MenuText {
                visible:rowItem.primary && page.view === "setup";x:page.px(5);y:page.px(91);width:parent.width-page.px(10)
                text:"Your games and saves stay untouched.";font.pixelSize:page.px(14);color:"#BBC6D3"
            }
            MouseArea { anchors.fill:parent;enabled:!rowItem.retiring;onClicked:{ page.selected=index;page.activate(); } }
        }
    }
    Flickable {
        id:logView;visible:page.view === "log"
        anchors { left:parent.left;right:parent.right;top:header.bottom;bottom:parent.bottom;topMargin:page.px(16) }
        contentHeight:logText.height;clip:true
        MenuText {
            id:logText;width:parent.width;wrapMode:Text.WrapAnywhere;font.pixelSize:page.px(15)
            text:page.job ? (page.job.results || []).map(r=>r.folder+": "+(r.ok ? r.games+" games"+(r.missing ? ", "+r.missing+" without a match" : "") : r.error)).join("\n")+"\n\n"+page.job.log : ""
        }
    }
}
