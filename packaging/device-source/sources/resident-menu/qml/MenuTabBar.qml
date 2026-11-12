import QtQuick

Item {
    id: bar
    property real uiScale: 1
    property int currentIndex: 0
    property int unread: 0
    property bool controllerAvailable: true
    property bool animate: true
    readonly property real controlWidth: Math.round((controllerAvailable ? 40 : 68) * uiScale)
    readonly property real edgeGap: Math.round(8 * uiScale)
    readonly property real tabGap: Math.round(7 * uiScale)
    readonly property real availableWidth: Math.max(0, width - 2 * controlWidth - 2 * edgeGap - 2 * tabGap)
    implicitHeight: Math.round(48 * uiScale)
    signal stepRequested(int direction)
    signal tabRequested(int index)

    function tabWidth(index) {
        const defaults = [104, 171, 104];
        const total = 379 * uiScale;
        const fit = Math.min(1, availableWidth / Math.max(1, total));
        const remaining = Math.max(0, availableWidth - total);
        return defaults[index] * uiScale * fit + (index === currentIndex ? remaining : 0);
    }
    Item {
        id: previous
        width: bar.controlWidth; height: bar.height
        opacity: bar.enabled ? 1 : 0.45
        MenuShoulderGlyph {
            anchors.centerIn: parent
            visible: bar.controllerAvailable
            side: "left"; uiScale: bar.uiScale
            width: implicitWidth; height: implicitHeight
        }
        MenuText {
            visible: !bar.controllerAvailable
            anchors.centerIn: parent
            text: "Ctrl Pg↑"; font.pixelSize: Math.round(12 * bar.uiScale); color: "#EAC28D"
        }
        MouseArea { anchors.fill: parent; onClicked: bar.stepRequested(-1) }
    }
    Row {
        id: tabs
        anchors { left: previous.right; right: next.left; leftMargin: bar.edgeGap; rightMargin: bar.edgeGap; verticalCenter: parent.verticalCenter }
        spacing: bar.tabGap
        Repeater {
            model: ["Home", "Notifications", "Settings"]
            delegate: Rectangle {
                id: tab
                required property string modelData
                required property int index
                readonly property bool chosen: bar.currentIndex === index
                width: bar.tabWidth(index)
                height: Math.round(44 * bar.uiScale)
                radius: Math.round(11 * bar.uiScale)
                color: chosen ? "#EAC28D" : "#1D2634"
                border.width: chosen ? 1 : 0
                border.color: "#E8C498"
                Behavior on width {
                    enabled: bar.animate
                    NumberAnimation { duration: 180; easing.type: Easing.OutCubic }
                }
                Behavior on color { enabled: bar.animate; ColorAnimation { duration: 120 } }
                Rectangle {
                    id: unreadBadge
                    visible: tab.index === 1 && bar.unread > 0
                    anchors { right: parent.right; rightMargin: 8 * bar.uiScale; verticalCenter: parent.verticalCenter }
                    width: Math.max(22 * bar.uiScale, unreadText.implicitWidth + 10 * bar.uiScale)
                    height: Math.round(20 * bar.uiScale); radius: height / 2
                    color: tab.chosen ? "#30271E" : "#354257"
                    MenuText {
                        id: unreadText
                        anchors.centerIn: parent
                        text: String(bar.unread)
                        font.pixelSize: Math.round(12 * bar.uiScale)
                        color: tab.chosen ? "#F0E5D6" : "#EAC28D"
                    }
                }
                MenuText {
                    anchors { left: parent.left; right: unreadBadge.visible ? unreadBadge.left : parent.right; verticalCenter: parent.verticalCenter; leftMargin: 10 * bar.uiScale; rightMargin: unreadBadge.visible ? 6 * bar.uiScale : 10 * bar.uiScale }
                    horizontalAlignment: Text.AlignHCenter
                    text: tab.modelData
                    color: tab.chosen ? "#211E1B" : "#BBC6D3"
                    font.pixelSize: Math.round(18 * bar.uiScale)
                    elide: Text.ElideRight
                }
                Accessible.name: modelData
                Accessible.role: Accessible.Button
                Accessible.selected: chosen
                MouseArea { anchors.fill: parent; onClicked: bar.tabRequested(tab.index) }
            }
        }
    }
    Item {
        id: next
        anchors.right: parent.right
        width: bar.controlWidth; height: bar.height
        opacity: bar.enabled ? 1 : 0.45
        MenuShoulderGlyph {
            anchors.centerIn: parent
            visible: bar.controllerAvailable
            side: "right"; uiScale: bar.uiScale
            width: implicitWidth; height: implicitHeight
        }
        MenuText {
            visible: !bar.controllerAvailable
            anchors.centerIn: parent
            text: "Ctrl Pg↓"; font.pixelSize: Math.round(12 * bar.uiScale); color: "#EAC28D"
        }
        MouseArea { anchors.fill: parent; onClicked: bar.stepRequested(1) }
    }
}
