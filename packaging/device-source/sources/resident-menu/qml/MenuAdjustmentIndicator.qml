import QtQuick

Item {
    id: hint
    property real uiScale: 1
    property bool editing: false
    property bool chosen: false
    signal stepped(int direction)
    implicitWidth: Math.round(56 * uiScale)
    implicitHeight: Math.round(36 * uiScale)
    Rectangle {
        anchors.fill: parent
        radius: Math.round(8 * hint.uiScale)
        color: hint.editing ? (hint.chosen ? "#2859402A" : "#203F6487") : "transparent"
        border.width: hint.editing ? 1 : 0
        border.color: hint.chosen ? "#80614930" : "#607FA6C9"
    }
    Repeater {
        model: [-1, 1]
        delegate: Item {
            required property int modelData
            width: hint.width / 2; height: hint.height
            x: modelData < 0 ? 0 : width
            MenuText {
                anchors.centerIn: parent
                text: modelData < 0 ? "‹" : "›"
                font.pixelSize: Math.round(24 * hint.uiScale)
                color: hint.chosen ? "#59402A" : "#BCCDDD"
            }
            MouseArea { anchors.fill: parent; onClicked: hint.stepped(modelData) }
        }
    }
}
