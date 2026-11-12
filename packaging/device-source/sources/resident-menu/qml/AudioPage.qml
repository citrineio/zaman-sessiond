import QtQuick
import Quickshell.Services.Pipewire

Item {
    id: audio
    property bool active: false
    property real uiScale: 1
    property int selected: 0
    property int changeDirection: 1
    readonly property bool adjustable: rows[selected].key === "volume" || rows[selected].key === "output"
    readonly property PwNode sink: Pipewire.defaultAudioSink
    // Object identity, rather than a cached numeric ID, follows node lifetime.
    readonly property var outputs: Pipewire.nodes.values.filter(node => node.audio !== null && node.isSink && !node.isStream)
        .sort((a, b) => outputLabel(a).localeCompare(outputLabel(b)) || a.name.localeCompare(b.name))
    property PwNode candidateNode: null
    property PwNode pendingOutput: null
    property bool outputRequestPending: false
    readonly property bool busy: outputRequestPending
    property string errorText: ""
    property bool leftDown: false
    property bool rightDown: false
    property int heldHorizontal: 0
    // Read audio-specific values only after binding and initial synchronization.
    readonly property bool hasLevel: active && Pipewire.ready && sink !== null && sink.ready
        && sink.audio !== null && sink.audio.channels.length > 0 && Number.isFinite(sink.audio.volume)
    readonly property int percent: hasLevel ? Math.round(Math.max(0, sink.audio.volume) * 100) : -1
    readonly property int displayedPercent: percent
    readonly property bool muted: hasLevel && sink.audio.muted
    readonly property var rows: [
        { key: "volume", icon: "audio", label: "Volume", detail: "Left / Right to adjust · B to return" },
        { key: "mute", icon: "audio", label: muted ? "Unmute" : "Mute", detail: muted ? "Restore sound at the current volume" : "Silence sound without changing the volume" },
        { key: "output", icon: "audio", label: "Audio output", detail: "Left / Right to choose · A to apply · B to return" },
        { key: "back", icon: "back", label: "Back", detail: "Return to the main menu" }
    ]
    signal backRequested()

    PwObjectTracker {
        objects: audio.active ? [audio.sink] : []
    }

    // The page owns deferred layout work; destroyed delegates are never captured.
    Timer {
        id: focusSettle
        interval: 1
        repeat: false
        onTriggered: audio.settleFocus()
    }
    function requestFocusSettle() { focusSettle.restart(); }
    function settleFocus() {
        if (!(active) || selected < 0 || selected >= controls.count) return;
        controls.forceLayout();
        controls.positionViewAtIndex(selected, ListView.Contain);
    }

    function px(value) { return Math.round(value * uiScale); }
    function location() {
        return {selected:selected, scroll:controls.contentY, candidateName:candidate() ? candidate().name : ""};
    }
    function restoreLocation(saved) {
        stopHorizontal();
        selected = Math.max(0, Math.min(rows.length - 1, saved.selected));
        var output = outputs.find(node => node.name === saved.candidateName);
        if (output) candidateNode = output;
        controls.forceLayout();
        controls.contentY = saved.scroll;
    }
    function outputLabel(node) {
        return node ? (node.description || node.nickname || node.name).replace(/[\x00-\x1f\x7f]/g, "").slice(0, 80) : "";
    }
    function candidate() {
        return candidateNode !== null && outputs.indexOf(candidateNode) >= 0 ? candidateNode : null;
    }
    function current() { return sink; }
    function diagnostics() {
        return {
            active: active, pipewireReady: Pipewire.ready, hasLevel: hasLevel,
            sink: sink ? {name: sink.name, description: outputLabel(sink), ready: sink.ready} : null,
            percent: percent, muted: muted, busy: busy, error: errorText,
            outputs: outputs.map(node => ({name: node.name, description: outputLabel(node), active: node === sink}))
        };
    }
    function resetCandidate() {
        candidateNode = sink !== null && outputs.indexOf(sink) >= 0 ? sink : (outputs[0] || null);
    }
    function stopHorizontal() {
        if (horizontalDelay) horizontalDelay.stop();
        if (horizontalStep) horizontalStep.stop();
        heldHorizontal = 0;
    }
    function move(direction) {
        stopHorizontal();
        selected = (selected + direction + rows.length) % rows.length;
        controls.positionViewAtIndex(selected, ListView.Contain);
    }
    function select(index) {
        if (selected !== index) stopHorizontal();
        selected = index;
    }
    function adjustVolume(direction) {
        if (!hasLevel || busy) return;
        // Keep this operation on the current bound object. Do not change mute.
        var target = sink;
        if (direction > 0 && target.audio.volume >= 1) return;
        var next = Math.max(0, Math.min(1, Math.round(target.audio.volume * 100 + direction * 5) / 100));
        if (next === target.audio.volume) return;
        errorText = "";
        target.audio.volume = next;
    }
    function cycleOutput(direction) {
        if (!Pipewire.ready || outputs.length === 0 || busy) return;
        var index = outputs.indexOf(candidateNode);
        var next = index < 0 ? 0 : (index + direction + outputs.length) % outputs.length;
        candidateNode = outputs[next];
        errorText = "";
    }
    function horizontalStepOnce(direction) {
        if (!active) return;
        changeDirection = direction;
        if (rows[selected].key === "volume") adjustVolume(direction);
        else if (rows[selected].key === "output") cycleOutput(direction);
    }
    function holdHorizontal(direction, immediate) {
        stopHorizontal();
        if (direction === 0 || !active) return;
        heldHorizontal = direction;
        if (immediate) horizontalStepOnce(direction);
        horizontalDelay.restart();
    }
    function horizontal(event, value) {
        var left = event === "ui_left" || event === "key_left";
        var down = value > 0.5;
        var wasDown = left ? leftDown : rightDown;
        if (left) leftDown = down; else rightDown = down;
        if (down && !wasDown && !adjustable) {
            if (left) backRequested();
            else activate();
            return;
        }
        if (down && !wasDown) holdHorizontal(left ? -1 : 1, true);
        else if (!down) {
            var remaining = heldHorizontal < 0 && leftDown ? -1
                : (heldHorizontal > 0 && rightDown ? 1 : (leftDown ? -1 : (rightDown ? 1 : 0)));
            if (remaining !== heldHorizontal) holdHorizontal(remaining, false);
        }
    }
    function finishOutputIfActive() {
        if (outputRequestPending && pendingOutput !== null && sink === pendingOutput) {
            outputDeadline.stop(); outputRequestPending = false;
            pendingOutput = null; errorText = ""; resetCandidate();
        }
    }
    function activate() {
        stopHorizontal();
        var key = rows[selected].key;
        if (key === "back") backRequested();
        else if (key === "mute" && hasLevel && !busy) {
            var target = sink;
            errorText = "";
            target.audio.muted = !target.audio.muted;
        } else if (key === "output" && Pipewire.ready && !busy) {
            var chosen = candidate();
            if (!chosen) { errorText = "The selected output is no longer available."; return; }
            if (chosen === sink) return;
            // The preference is a request, not confirmation of active routing.
            pendingOutput = chosen;
            outputRequestPending = true;
            outputDeadline.restart();
            errorText = "";
            Pipewire.preferredDefaultAudioSink = chosen;
            finishOutputIfActive();
        }
        // Volume editing never toggles mute; mute remains a separate row.
    }
    onActiveChanged: {
        stopHorizontal(); leftDown = false; rightDown = false;
        if (active) { errorText = ""; if (!candidate()) resetCandidate(); }
        else { outputDeadline.stop(); outputRequestPending = false; pendingOutput = null; }
    }
    onSinkChanged: {
        stopHorizontal();
        if (outputRequestPending) finishOutputIfActive();
        else resetCandidate();
    }
    onHasLevelChanged: if (!hasLevel) stopHorizontal()
    onOutputsChanged: {
        if (outputRequestPending && (pendingOutput === null || outputs.indexOf(pendingOutput) < 0)) {
            outputDeadline.stop(); outputRequestPending = false; pendingOutput = null;
            errorText = "The requested output disconnected.";
        }
        if (!candidate()) resetCandidate();
    }
    Timer {
        id: outputDeadline
        interval: 2500
        onTriggered: {
            if (audio.outputRequestPending && (audio.pendingOutput === null || audio.sink !== audio.pendingOutput))
                audio.errorText = "The requested output did not become active. Choose an output and try again.";
            audio.outputRequestPending = false;
            audio.pendingOutput = null;
        }
    }
    Timer {
        id: horizontalDelay
        interval: 220
        onTriggered: {
            if (audio.active && audio.heldHorizontal !== 0) {
                audio.horizontalStepOnce(audio.heldHorizontal); horizontalStep.start();
            }
        }
    }
    Timer {
        id: horizontalStep
        interval: 70; repeat: true
        onTriggered: {
            if (!audio.active || audio.heldHorizontal === 0) stop();
            else audio.horizontalStepOnce(audio.heldHorizontal);
        }
    }

    Column {
        id: status
        width: parent.width
        spacing: audio.px(6)
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: !Pipewire.ready ? "Connecting to audio…"
                : (audio.current() ? "Active output: " + audio.outputLabel(audio.current()) : "No active playback output")
            color: "#BBC6D3"; font.pixelSize: audio.px(15)
        }
        MenuText {
            visible: audio.busy || audio.errorText.length > 0
            width: parent.width; wrapMode: Text.WordWrap
            text: audio.errorText || "Switching output…"
            color: audio.errorText ? "#E8B898" : "#88A8C9"
            font.pixelSize: audio.px(14)
        }
    }
    ListView {
        id: controls
        property point lastPointerPosition: Qt.point(-100000, -100000)
        anchors { left: parent.left; right: parent.right; top: status.bottom; bottom: parent.bottom; topMargin: audio.px(16) }
        model: audio.rows
        currentIndex: audio.selected
        spacing: audio.px(6)
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        delegate: MenuFocusRow {
            id: row
            required property var modelData
            required property int index
            chosen: audio.selected === index
            uiScale: audio.uiScale
            readonly property bool volumeRow: modelData.key === "volume"
            readonly property bool selector: volumeRow || modelData.key === "output"
            width: controls.width
            baseHeight: volumeRow ? 118 : (modelData.key === "output" ? 96 : 72)
            onFocusSettled: if (row.chosen) audio.requestFocusSettle()
            MenuFocusBackground {
                chosen: row.chosen
                uiScale: audio.uiScale
                emphasis: row.focusAmount
                idleStart: "#162F425C"
                idleEnd: "#10121820"
            }
            MenuIcon {
                x: audio.px(16); y: audio.px(18) + row.contentOffset
                name: row.modelData.icon
                width: audio.px(28 + 2 * row.focusAmount); height: width
                ink: row.chosen ? "#30271E" : "#DEC09A"
            }
            Column {
                x: audio.px(58); y: audio.px(14) + row.contentOffset
                width: parent.width - audio.px(row.selector ? 144 : 78)
                spacing: audio.px(3)
                MenuText {
                    width: parent.width
                    text: row.modelData.label
                    font.pixelSize: audio.px(22 + 2 * row.focusAmount)
                    color: row.chosen ? "#211E1B" : "#ECEEF1"
                }
                MenuText {
                    width: parent.width; elide: Text.ElideRight
                    text: row.modelData.detail
                    font.pixelSize: audio.px(13)
                    color: row.chosen ? "#473627" : "#BBC6D3"
                }
                MenuChoiceValue {
                    visible: row.modelData.key === "output"
                    width: parent.width
                    direction: audio.changeDirection; uiScale: audio.uiScale
                    text: audio.candidate() ? audio.outputLabel(audio.candidate())
                        + (audio.candidate() === audio.sink ? " · Active" : " · Not applied")
                        : "No playback outputs available"
                    fontSize: audio.px(15)
                    color: row.chosen ? "#30271E" : "#C7CFD8"
                }
                Item {
                    visible: row.volumeRow
                    width: parent.width; height: audio.px(36)
                    Rectangle {
                        y: audio.px(17)
                        width: parent.width - audio.px(100); height: audio.px(6); radius: height / 2
                        color: row.chosen ? "#38594333" : "#34465A73"
                        Rectangle {
                            width: parent.width * Math.min(100, Math.max(0, audio.displayedPercent)) / 100
                            height: parent.height; radius: parent.radius
                            Behavior on width { NumberAnimation { duration: 90; easing.type: Easing.OutCubic } }
                            color: row.chosen ? "#59402A" : "#DAB991"
                        }
                    }
                    MenuChoiceValue {
                        anchors.right: parent.right
                        width: audio.px(96)
                        direction: audio.changeDirection; uiScale: audio.uiScale
                        y: audio.px(5)
                        text: audio.hasLevel ? audio.displayedPercent + "%" + (audio.muted ? " · Muted" : "") : "—"
                        color: row.chosen ? "#30271E" : "#DAB991"
                        fontSize: audio.px(audio.muted ? 15 : 21)
                    }
                }
            }
            MouseArea {
                id: audioPointer
                anchors.fill: parent; hoverEnabled: true
                onEntered: if (row.pointerMoved(audioPointer)) audio.select(index)
                onPositionChanged: if (row.pointerMoved(audioPointer)) audio.select(index)
                onClicked: { audio.select(index); audio.activate(); }
                onWheel: wheel => {
                    audio.select(index);
                    if (row.volumeRow || row.modelData.key === "output") audio.horizontalStepOnce(wheel.angleDelta.y > 0 ? 1 : -1);
                    else audio.move(wheel.angleDelta.y > 0 ? -1 : 1);
                    wheel.accepted = true;
                }
            }
            MenuAdjustmentIndicator {
                visible: row.selector
                width: implicitWidth; height: implicitHeight
                uiScale: audio.uiScale; chosen: row.chosen
                editing: row.chosen
                enabled: !audio.busy
                anchors { right: parent.right; rightMargin: audio.px(12); verticalCenter: parent.verticalCenter }
                onStepped: direction => {
                    audio.select(row.index);
                    audio.horizontalStepOnce(direction);
                }
            }
        }
    }
}
