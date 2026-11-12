.pragma library

// Pegasus's configured key lists use GamepadKeyId, starting at 0x100000.
// Decode bindings rather than assuming that X/Y have their default actions.
// Reference: pegasus-frontend/src/backend/types/GamepadKeyId.h and model/keys/Key.h.
function binding(keys, role, controller) {
    var list = keys && keys[role] ? keys[role] : [];
    var fallback = null;
    for (var i = 0; i < list.length; ++i) {
        var key = list[i], code = Number(key.key), offset = code - 0x100000;
        var pad = offset >= 0 && offset <= 12;
        if (pad && !key.modifiers) {
            var faces = ["south", "east", "west", "north"];
            var hint = offset < 4 ? {kind: "face", face: faces[offset]} :
                offset === 4 || offset === 7 ? {kind: "shoulder", side: offset === 4 ? "left" : "right"} :
                {kind: "key", text: ["", "", "", "", "", "LT", "L3", "", "RT", "R3", "Select", "Start", "Guide"][offset]};
            if (controller) return hint;
            if (!fallback) fallback = hint;
        } else {
            var name = typeof key.name === "function" ? key.name() : key.label;
            // Unknown bindings are omitted, never displayed as internal role names.
            if (!name) continue;
            var keyboard = {kind: "key", text: String(name), escape: code === 0x01000000};
            if (!controller) return keyboard;
            if (!fallback) fallback = keyboard;
        }
    }
    return fallback;
}
function forState(shell, modal) {
    var out = [];
    var keys = shell.backend ? shell.backend.keys : null;
    function action(role, label) {
        var hint = binding(keys, role, shell.controllerHints);
        if (!hint) return;
        hint.label = role === "cancel" && !modal && hint.escape ? "System menu" : label;
        out.push(hint);
    }
    function arrows(text, label) { out.push({kind: "arrows", text: text, label: label}); }
    if (modal === "search") {
        arrows("↑ ↓ ← →", "Choose key");
        action("accept", shell.keyboard[shell.keyboardIndex] === "DONE" ? "Search" : "Type");
        if (shell.controllerHints) {
            action("filters", "Change keyboard");
            action("details", "Delete");
            action("cancel", "Done");
            out.push({kind: "key", text: "Start", label: "Done"});
        } else {
            out.push({kind: "key", text: "Tab", label: "Change keyboard"});
            out.push({kind: "key", text: "Backspace", label: "Delete"});
            action("cancel", "Done");
        }
    } else if (modal === "collections") {
        arrows("↑ ↓", "Browse"); action("accept", "Select"); action("cancel", "Cancel");
    } else if (modal === "details") {
        arrows("← →", "Actions"); arrows("↑ ↓", "Description");
        action("accept", ["Play", "Favorite", "Back"][shell.detailAction]); action("cancel", "Back");
    } else {
        if (shell.zone === "nav") {
            arrows("← →", "Tabs"); arrows("↓", "Browse"); action("accept", "Open");
        } else if (shell.zone === "hero") {
            arrows("← →", "Actions"); arrows("↑ ↓", "Move");
            action("accept", shell.heroAction === 0 ? "Play" : "Details");
        } else if (shell.zone === "settings") {
            arrows("↑ ↓", "Choose");
            arrows("← →", "Switch group");
            action("accept", shell.settingIndex < 3 ? "Change" : "Open");
        } else if (shell.zone === "filters") {
            arrows("← →", "Filters"); arrows("↑ ↓", "Move"); action("accept", "Change");
        } else {
            arrows("↑ ↓ ← →", "Browse");
            if (shell.currentGame) { action("accept", "Play"); action("details", "Details"); }
            action("filters", "Search");
        }
        action("cancel", shell.zone === "nav" && shell.page === 0 ? "System menu" : "Back");
        action("prevPage", "Previous tab"); action("nextPage", "Next tab");
    }
    return out;
}
