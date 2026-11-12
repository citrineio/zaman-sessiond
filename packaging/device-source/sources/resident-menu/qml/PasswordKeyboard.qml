import QtQuick

Item {
    id: keyboard
    property real uiScale: 1
    property string secret: ""
    property string title: "Wi-Fi password"
    property string submitLabel: "Join"
    property int maxLength: 64
    property bool masked: true
    property bool digitsOnly: false
    property int selected: 0
    property int view: 0 // lower | upper | symbols
    readonly property string letters: view === 1
        ? "QWERTYUIOPASDFGHJKL-ZXCVBNM,./1234567890"
        : "qwertyuiopasdfghjkl-zxcvbnm,./1234567890"
    readonly property string symbols: "!@#$%^&*()_+{}[]<>?=\\|;:'\"`,.~/-01234567"
    readonly property var characters: (digitsOnly ? "1234567890" : view === 2 ? symbols : letters).split("")
    readonly property int characterRows: digitsOnly ? 1 : 4
    readonly property var actions: digitsOnly ? ["Delete", submitLabel]
        : [view === 0 ? "ABC" : view === 1 ? "#+=" : "abc", "Space", "Delete", submitLabel]
    signal submitted(string password)
    function px(value) { return Math.round(value * uiScale); }
    function reset() { secret = ""; selected = 0; view = 0; }
    function cycleView() { if (!digitsOnly) view = (view + 1) % 3; }
    function append(value) {
        // Printable credentials only; never log this string.
        if (/^[\x20-\x7e]+$/.test(value) && (!digitsOnly || /^[0-9]+$/.test(value)) && secret.length + value.length <= maxLength) secret += value;
    }
    function erase() { secret = secret.slice(0, -1); }
    function navigate(dx, dy) {
        var row = selected < 40 ? Math.floor(selected / 10) : characterRows;
        var column = selected < 40 ? selected % 10 : (selected - 40) * (10 / actions.length);
        if (dy !== 0) {
            row = (row + dy + characterRows + 1) % (characterRows + 1);
            selected = row === characterRows ? 40 + Math.floor(column / (10 / actions.length)) : row * 10 + column;
        } else if (row === characterRows) selected = 40 + (selected - 40 + dx + actions.length) % actions.length;
        else selected = row * 10 + (column + dx + 10) % 10;
    }
    function activate() {
        if (selected < 40) append(characters[selected]);
        else if (digitsOnly) { if (selected === 40) erase(); else submitted(secret); }
        else if (selected === 40) cycleView();
        else if (selected === 41) append(" ");
        else if (selected === 42) erase();
        else submitted(secret);
    }
    Component.onDestruction: secret = ""
    Column {
        width: parent.width
        spacing: keyboard.px(14)
        MenuText {
            width: parent.width
            text: keyboard.title
            font.pixelSize: keyboard.px(22)
        }
        Rectangle {
            width: parent.width; height: keyboard.px(58); radius: keyboard.px(12)
            color: "#273548"; border.color: "#607C99"
            MenuText {
                anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: keyboard.px(16) }
                text: keyboard.secret.length ? (keyboard.masked ? "•".repeat(keyboard.secret.length) : keyboard.secret) : "Enter " + (keyboard.masked ? "password" : "code")
                elide: Text.ElideLeft
                color: keyboard.secret.length ? "#ECEEF1" : "#BBC6D3"
                font.pixelSize: keyboard.px(20)
            }
        }
        MenuText {
            width: parent.width; wrapMode: Text.WordWrap
            text: "D-pad: choose key · A: enter · B: cancel" + (keyboard.digitsOnly ? "" : " · Y: change keyboard")
                + "\nKeyboard: type, Backspace to delete, Enter to " + keyboard.submitLabel.toLowerCase()
            font.pixelSize: keyboard.px(14); color: "#BBC6D3"
        }
        Grid {
            width: parent.width; columns: 10; spacing: keyboard.px(4)
            Repeater {
                model: keyboard.characters
                delegate: Rectangle {
                    required property string modelData
                    required property int index
                    width: (parent.width - keyboard.px(36)) / 10
                    height: keyboard.px(48); radius: keyboard.px(8)
                    color: keyboard.selected === index ? "#E7BE89" : "#273141"
                    MenuText {
                        anchors.centerIn: parent; text: modelData
                        font.pixelSize: keyboard.px(20)
                        color: keyboard.selected === index ? "#211E1B" : "#ECEEF1"
                    }
                    MouseArea { anchors.fill: parent; onClicked: { keyboard.selected = index; keyboard.activate(); } }
                }
            }
        }
        Row {
            width: parent.width; spacing: keyboard.px(4)
            Repeater {
                model: keyboard.actions
                delegate: Rectangle {
                    required property string modelData
                    required property int index
                    width: (parent.width - keyboard.px(4 * (keyboard.actions.length - 1))) / keyboard.actions.length
                    height: keyboard.px(48); radius: keyboard.px(8)
                    color: keyboard.selected === 40 + index ? "#E7BE89" : "#273141"
                    MenuText {
                        anchors.centerIn: parent; text: modelData
                        font.pixelSize: keyboard.px(15)
                        color: keyboard.selected === 40 + index ? "#211E1B" : "#ECEEF1"
                    }
                    MouseArea { anchors.fill: parent; onClicked: { keyboard.selected = 40 + index; keyboard.activate(); } }
                }
            }
        }
    }
}
