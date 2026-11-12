import QtQuick
import QtQml.Models

Item {
    id: root
    property var popups: []
    property real uiScale: 1
    readonly property bool hasCards: cards.count > 0
    implicitHeight: stack.implicitHeight
    onPopupsChanged: syncCards()
    property bool hasClosingCards: false
    ListModel { id: cards }
    function syncCards() {
        const incoming = popups;
        const now = Date.now();
        for (let i = cards.count - 1; i >= 0; --i) {
            const card = cards.get(i);
            const update = incoming.find(r => r.id === card.rid);
            if (update) {
                cards.setProperty(i, "payload", update);
                cards.setProperty(i, "isClosing", false);
            } else if (!card.isClosing) {
                cards.setProperty(i, "isClosing", true);
                cards.setProperty(i, "removeAt", now + 140);
            } else if (card.removeAt <= now) cards.remove(i);
        }
        for (const record of incoming) {
            let found = false;
            for (let i = 0; i < cards.count; ++i) if (cards.get(i).rid === record.id) found = true;
            if (!found && cards.count < 2) cards.append({rid:record.id, payload:record, isClosing:false, removeAt:0});
        }
        hasClosingCards = false;
        for (let i = 0; i < cards.count; ++i) if (cards.get(i).isClosing) hasClosingCards = true;
    }
    Timer { interval:50; repeat:true; running:root.hasClosingCards; onTriggered:root.syncCards() }
    Column {
        id: stack
        width: root.width
        spacing: 10 * root.uiScale
        Repeater {
            model: cards
            delegate: NotificationToastCard {
                required property var payload
                required property bool isClosing
                record: payload
                closing: isClosing
                width: stack.width
                uiScale: root.uiScale
            }
        }
    }
}
