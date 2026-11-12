import QtQuick

Item {
    id: display
    property real uiScale: 1
    property bool active: false
    property bool helperReady: false
    property var outputs: []
    property bool available: false
    property string outputId: ""
    property var mode: null
    property var preview: null
    readonly property bool previewing: preview !== null
    property real previewDeadline: 0
    property real clockNow: Date.now()
    property int selected: 0
    property int changeDirection: 1
    readonly property bool adjustable: !previewing && ["output", "resolution", "refresh"].indexOf(rows[selected].key) >= 0
    property int sequence: 0
    property int requestId: -1
    property string requestAction: ""
    property bool waiting: false
    property var queued: null
    property string notice: ""
    property string backendError: ""
    readonly property bool busy: (waiting && requestAction !== "display-status") || queued !== null
    readonly property int secondsLeft: preview ? Math.max(0, Math.min(preview.seconds_left,
        Math.ceil((previewDeadline - clockNow) / 1000))) : 0
    readonly property var output: outputs.find(o => identity(o) === outputId) || null
    readonly property bool hasOutput: output !== null
    readonly property var modes: distinctModes(output ? output.modes : [])
    readonly property var resolutions: {
        var result = [];
        for (var i = 0; i < modes.length; ++i)
            if (!result.some(m => m.width === modes[i].width && m.height === modes[i].height)) result.push(modes[i]);
        return result;
    }
    readonly property var rates: mode ? modes.filter(m => m.width === mode.width && m.height === mode.height) : []
    readonly property bool changed: output && mode && !sameMode(mode, output.current_mode)
    readonly property var rows: previewing ? [
        { key: "revert", label: "Revert", detail: "Restore the previous display mode" },
        { key: "keep", label: "Keep this mode", detail: "Save only if the picture looks correct" }
    ] : hasOutput ? [
        { key: "output", label: "Display", detail: "" },
        { key: "resolution", label: "Resolution", detail: "" },
        { key: "refresh", label: "Refresh rate", detail: "" },
        { key: "preview", label: "Preview changes", detail: "Try for 15 seconds before saving" },
        { key: "back", label: "Back", detail: "Return to Settings" }
    ] : [{ key: "back", label: "Back", detail: "Return to Settings" }]
    signal command(var request)
    signal backRequested()
    signal navigationReset()

    // The page owns deferred layout work; destroyed delegates are never captured.
    Timer {
        id: focusSettle
        interval: 1
        repeat: false
        onTriggered: display.settleFocus()
    }
    function requestFocusSettle() { focusSettle.restart(); }
    function settleFocus() {
        if (!(active) || selected < 0 || selected >= displayList.count) return;
        displayList.forceLayout();
        displayList.positionViewAtIndex(selected, ListView.Contain);
    }

    function px(value) { return Math.round(value * uiScale); }
    function clean(value) { return String(value || "").replace(/[\x00-\x1f\x7f]/g, "").slice(0, 120); }
    function identity(o) { return JSON.stringify([o.name, o.make, o.model, o.serial]); }
    function sameMode(a, b) { return a && b && a.width === b.width && a.height === b.height && a.refresh === b.refresh; }
    function distinctModes(values) {
        var result = [];
        for (var mode of values) {
            if (mode.width > 0 && mode.height > 0 && mode.refresh > 0 && !result.some(other => sameMode(other, mode))) result.push(mode);
        }
        return result.sort((a, b) => b.width - a.width || b.height - a.height || b.refresh - a.refresh);
    }
    function hz(rate) { return (rate / 1000).toFixed(3) + " Hz"; }
    function modeLabel(m) { return m ? m.width + " × " + m.height + " · " + hz(m.refresh) : "Unknown mode"; }
    function outputName(o) {
        var name = clean([o.make, o.model].filter(v => v).join(" "));
        return name ? name + " · " + clean(o.name) : clean(o.name);
    }
    function select(index) {
        selected = (index + rows.length) % rows.length;
        displayList.positionViewAtIndex(selected, ListView.Contain);
    }
    function move(direction) { if (active) select(selected + direction); }
    function send(request) {
        if (!active || !helperReady) return;
        if (waiting) {
            if (request.action !== "display-status" && queued === null) queued = request;
            return;
        }
        requestId = ++sequence;
        requestAction = request.action;
        waiting = true;
        command(Object.assign({}, request, { id: requestId }));
    }
    function receive(message) {
        if (!active || message.id !== requestId) return;
        var action = requestAction;
        waiting = false;
        if (message.busy) {
            // Another visit's request can still be finishing. Poll instead of
            // replaying a mutation after an uncertain result.
            if (action !== "display-status") notice = "A previous display request is finishing. Try again.";
            queued = null;
            return;
        }
        var state = message.state;
        var hadPreview = preview !== null;
        var previousOutput = outputId;
        if (state) {
            available = state.available;
            if (JSON.stringify(outputs) !== JSON.stringify(state.outputs)) outputs = state.outputs || [];
            if (!outputs.some(o => identity(o) === outputId)) {
                outputId = outputs.length ? identity(outputs[0]) : "";
                mode = null;
                if (previousOutput.length) notice = "The selected display disconnected or changed.";
            }
            var current = output;
            if (!mode || !current || !current.modes.some(m => sameMode(m, mode)))
                mode = current ? (current.modes.find(m => sameMode(m, current.current_mode)) || current.modes[0] || null) : null;
            if (state.preview && (!preview || state.preview.token !== preview.token)) {
                clockNow = Date.now();
                previewDeadline = clockNow + state.preview.seconds_left * 1000;
            }
            preview = state.preview;
            backendError = clean(state.error);
            if (!hadPreview && preview) { navigationReset(); select(0); }
            if (hadPreview && !preview) {
                navigationReset();
                mode = current ? (current.modes.find(m => sameMode(m, current.current_mode)) || current.modes[0] || null) : null;
                select(output ? 3 : 0);
                notice = action === "display-keep" && !message.error ? "Display mode saved."
                    : action === "display-revert" && !message.error ? "Previous mode restored."
                    : "Preview ended. Check the active mode below.";
            }
            if (selected >= rows.length) select(rows.length - 1);
        }
        if (message.error) notice = clean(message.error);
        var next = queued;
        queued = null;
        if (next) send(next);
    }
    function cycle(key, direction) {
        if (busy || preview || !output) return;
        notice = "";
        changeDirection = direction;
        if (key === "output") {
            var i = outputs.findIndex(o => identity(o) === outputId);
            var next = outputs[(i + direction + outputs.length) % outputs.length];
            outputId = identity(next);
            mode = next.modes.find(m => sameMode(m, next.current_mode)) || next.modes[0] || null;
        } else if (key === "resolution" && resolutions.length) {
            var r = resolutions.findIndex(m => mode && m.width === mode.width && m.height === mode.height);
            var resolution = resolutions[(r + direction + resolutions.length) % resolutions.length];
            var choices = modes.filter(m => m.width === resolution.width && m.height === resolution.height);
            mode = choices.find(m => output.current_mode && m.refresh === output.current_mode.refresh) || choices[0];
        } else if (key === "refresh" && rates.length) {
            var rate = rates.findIndex(m => sameMode(m, mode));
            mode = rates[rate < 0 ? (direction > 0 ? 0 : rates.length - 1) : (rate + direction + rates.length) % rates.length];
        }
    }
    function horizontal(direction) {
        if (!active) return;
        if (adjustable) cycle(rows[selected].key, direction);
        else if (direction < 0) back();
        else activate();
    }
    function activate() {
        if (!active) return;
        var key = rows[selected].key;
        if (key === "back") { back(); return; }
        if (busy) return;
        if (key === "keep") {
            if (preview && preview.confirmable && secondsLeft > 0)
                send({ action: "display-keep", token: preview.token });
        } else if (key === "revert") {
            if (preview) send({ action: "display-revert", token: preview.token });
        } else if (key === "preview") {
            if (changed) {
                notice = "";
                send({ action: "display-preview", output: output, mode: mode });
            }
        }
    }
    function back() {
        if (preview || busy) {
            // Leaving invalidates the transaction immediately in the input
            // helper, including a mode apply whose response is still pending.
            command({ action: "display-leave" });
            backRequested();
        } else backRequested();
    }
    onActiveChanged: {
        waiting = false;
        queued = null;
        requestId = -1;
        if (active) {
            notice = ""; backendError = ""; preview = null;
            select(Math.min(selected, rows.length - 1));
            if (helperReady) {
                command({ action: "display-open" });
                send({ action: "display-status" });
            } else notice = "Rebuild zaman-menu-dev-input from this package, then relaunch the preview.";
        } else if (helperReady) command({ action: "display-leave" });
    }
    Timer {
        interval: 500; repeat: true; running: display.active
        onTriggered: { display.clockNow = Date.now(); display.send({ action: "display-status" }); }
    }
    Column {
        id: status
        width: parent.width
        spacing: display.px(9)
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: display.preview ? "Keep this picture? " + display.secondsLeft + "s remaining"
                : display.output ? "Active: " + display.modeLabel(display.output.current_mode)
                : display.waiting ? "Reading connected displays…" : display.available ? "No active display available." : "Display settings unavailable."
            font.pixelSize: display.px(19); color: display.preview ? "#EAC28D" : "#C7CFD8"
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: display.preview ? "No confirmation restores the previous mode automatically."
                : "Left / Right chooses · A selects · nothing changes until Preview."
            font.pixelSize: display.px(14); color: "#BBC6D3"
        }
        MenuText {
            visible: display.busy
            width: parent.width; wrapMode: Text.WordWrap
            text: "Waiting for Sway to verify the display request…"
            font.pixelSize: display.px(14); color: "#BBC6D3"
        }
        MenuText {
            visible: text.length > 0
            width: parent.width; wrapMode: Text.WordWrap
            text: display.notice || display.backendError
            font.pixelSize: display.px(14); color: "#D9BBA0"
        }
    }
    ListView {
        id: displayList
        property point lastPointerPosition: Qt.point(-100000, -100000)
        anchors { left: parent.left; right: parent.right; top: status.bottom; bottom: parent.bottom; topMargin: display.px(22) }
        model: display.rows
        currentIndex: display.selected
        spacing: display.px(8)
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        delegate: MenuFocusRow {
            id: row
            required property var modelData
            required property int index
            chosen: display.selected === index
            uiScale: display.uiScale
            width: displayList.width
            baseHeight: 76
            readonly property bool selector: ["output", "resolution", "refresh"].indexOf(modelData.key) >= 0
            readonly property bool enabledRow: modelData.key === "back" || modelData.key === "revert" || (!display.busy
                && (modelData.key !== "preview" || display.changed)
                && (modelData.key !== "keep" || (display.preview && display.preview.confirmable && display.secondsLeft > 0)))
            onFocusSettled: if (row.chosen) display.requestFocusSettle()
            MenuFocusBackground { chosen: row.chosen; uiScale: display.uiScale; emphasis: row.focusAmount }
            MenuIcon {
                x: display.px(16); y: Math.round((parent.height - height) / 2)
                name: row.modelData.key === "back" || row.modelData.key === "revert" ? "back" : "display"
                width: display.px(28 + 2 * row.focusAmount); height: width
                ink: row.chosen ? "#30271E" : "#DEC09A"
            }
            Column {
                x: display.px(58); y: Math.round((parent.height - height) / 2)
                width: parent.width - display.px(row.selector ? 144 : 78)
                spacing: display.px(4)
                // Disabled text uses explicit colors, keeping crisp edges
                // and readable contrast on the focused sand background.
                MenuText {
                    width: parent.width; elide: Text.ElideRight
                    text: row.modelData.label
                    font.pixelSize: display.px(21 + 2 * row.focusAmount)
                    color: row.chosen
                        ? (row.enabledRow ? "#211E1B" : "#433527")
                        : (row.enabledRow ? "#ECEEF1" : "#AEBAC8")
                }
                MenuChoiceValue {
                    width: parent.width
                    text: row.modelData.key === "output" ? (display.output ? display.outputName(display.output) : "Unavailable")
                        : row.modelData.key === "resolution" ? (display.mode ? display.mode.width + " × " + display.mode.height : "No advertised modes")
                        : row.modelData.key === "refresh" ? (display.mode ? display.hz(display.mode.refresh) : "Unavailable")
                        : row.modelData.key === "preview" && !display.changed ? "Selected mode is already active" : row.modelData.detail
                    direction: display.changeDirection
                    animate: row.selector
                    uiScale: display.uiScale
                    fontSize: display.px(14); color: row.chosen ? "#473627" : "#BBC6D3"
                }
            }
            MouseArea {
                id: pointer
                anchors.fill: parent; hoverEnabled: true
                onEntered: if (row.pointerMoved(pointer)) display.select(index)
                onPositionChanged: if (row.pointerMoved(pointer)) display.select(index)
                onClicked: { display.select(index); display.activate(); }
                onWheel: wheel => { display.move(wheel.angleDelta.y > 0 ? -1 : 1); wheel.accepted = true; }
            }
            MenuAdjustmentIndicator {
                visible: row.selector
                width: implicitWidth; height: implicitHeight
                uiScale: display.uiScale; chosen: row.chosen
                editing: row.chosen
                enabled: row.enabledRow
                anchors { right: parent.right; rightMargin: display.px(12); verticalCenter: parent.verticalCenter }
                onStepped: direction => {
                    display.select(row.index);
                    display.cycle(row.modelData.key, direction);
                }
            }
        }
    }
}
