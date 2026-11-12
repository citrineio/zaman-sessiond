import QtQuick

Item {
    id: exitPage
    property string prompt: "Exit this game?"
    property string detail: "Close the current game and return to your library."
    property string cancelLabel: "Keep playing"
    property string confirmLabel: "Exit Game"
    property string busyHeading: "Closing game…"
    property bool active: false
    property bool busy: false
    property string notice: ""
    property real uiScale: 1
    property int selected: 0
    signal confirmRequested()
    signal backRequested()
    function px(value) { return Math.round(value * uiScale); }
    function move(direction) {
        if (active && !busy) selected = (selected + direction + 2) % 2;
    }
    function activate() {
        if (!active || busy) return;
        if (selected === 0) backRequested();
        else confirmRequested();
    }
    onActiveChanged: if (active && !busy) selected = 0

    Column {
        width: parent.width
        spacing: exitPage.px(18)
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: exitPage.busy ? exitPage.busyHeading : exitPage.prompt
            font.pixelSize: exitPage.px(30)
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: exitPage.busy ? "Please wait…" : exitPage.detail
            font.pixelSize: exitPage.px(19); color: "#BBC6D3"
        }
        MenuText {
            visible: exitPage.notice.length > 0
            width: parent.width; wrapMode: Text.WordWrap
            text: exitPage.notice; textFormat: Text.PlainText
            font.pixelSize: exitPage.px(17); color: "#EAC28D"
        }
        Repeater {
            model: [exitPage.cancelLabel, exitPage.confirmLabel]
            delegate: Rectangle {
                required property int index
                required property string modelData
                width: exitPage.width; height: exitPage.px(64)
                radius: exitPage.px(13)
                readonly property bool highlighted: exitPage.selected === index
                color: highlighted ? "#EAC28D" : "#213349"
                border.width: 1; border.color: highlighted ? "#E8C498" : "#355C718C"
                opacity: exitPage.busy ? 0.45 : 1
                MenuText {
                    anchors.centerIn: parent
                    text: modelData
                    font.pixelSize: exitPage.px(22)
                    color: parent.highlighted ? "#29221C" : "#E9EDF2"
                }
                MouseArea {
                    anchors.fill: parent; enabled: exitPage.active && !exitPage.busy
                    onClicked: { exitPage.selected = index; exitPage.activate(); }
                }
            }
        }
    }
}
