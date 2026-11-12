import QtQuick

Item {
    id: focusRow
    property bool chosen: false
    property real uiScale: 1
    property real baseHeight: 66
    property bool initialized: false
    property real focusAmount: chosen ? 1 : 0
    readonly property real contentOffset: Math.round(6 * uiScale * focusAmount)
    height: Math.round(uiScale * (baseHeight + 12 * focusAmount))
    signal focusSettled()
    onFocusAmountChanged: {
        if (initialized && (focusAmount === 0 || focusAmount === 1)) focusSettled();
    }

    function pointerMoved(area) {
        var view = focusRow.ListView.view;
        if (!view || !area.containsMouse) return false;
        var position = area.mapToItem(null, area.mouseX, area.mouseY);
        var previous = view.lastPointerPosition;
        view.lastPointerPosition = position;
        // Layout motion under a stationary pointer must not steal controller
        // focus or repeatedly switch the row being enlarged.
        return Math.abs(position.x - previous.x) > 0.5
            || Math.abs(position.y - previous.y) > 0.5;
    }

    // Actual delegate height changes let ListView move neighboring rows.
    // A new selection retargets the running animation immediately.
    Behavior on focusAmount {
        enabled: focusRow.initialized && focusRow.visible
        NumberAnimation {
            duration: 120
            easing.type: Easing.OutCubic
        }
    }
    Component.onCompleted: initialized = true
}
