import QtQuick

Rectangle {
    id: card
    required property real toastId
    required property string heading
    required property string message
    required property string sourceName
    required property string kindLabel
    required property string accent
    required property bool closing
    property real uiScale: 1
    property string titleFont: "Sans Serif"
    property string detailFont: "monospace"
    property bool entered: false

    height: Math.ceil((message.length > 0 ? 174 : 122) * uiScale)
    radius: 12 * uiScale
    color: "#ec1b1610"
    border.color: "#886f5336"
    border.width: Math.max(1, uiScale)
    opacity: entered && !closing ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
    transform: Translate {
        x: card.entered && !card.closing ? 0 : 40 * card.uiScale
        Behavior on x { NumberAnimation { duration: 180; easing.type: Easing.OutCubic } }
    }
    Component.onCompleted: entered = true

    Rectangle {
        x: 0; y: 12 * card.uiScale
        width: 4 * card.uiScale
        height: parent.height - 24 * card.uiScale
        radius: 2 * card.uiScale
        color: card.accent
    }
    Rectangle {
        x: 20 * card.uiScale; y: 22 * card.uiScale
        width: 58 * card.uiScale; height: 58 * card.uiScale
        radius: 10 * card.uiScale
        color: "#33100c06"
        border.color: card.accent
        Text {
            anchors.centerIn: parent
            text: card.kindLabel
            textFormat: Text.PlainText
            font.family: card.detailFont
            font.pixelSize: 16 * card.uiScale
            font.bold: true
            color: card.accent
        }
    }
    Column {
        x: 96 * card.uiScale; y: 15 * card.uiScale
        width: card.width - 120 * card.uiScale
        spacing: 3 * card.uiScale
        Text {
            width: parent.width
            text: card.sourceName.toUpperCase()
            textFormat: Text.PlainText
            font.family: card.detailFont
            font.pixelSize: 14 * card.uiScale
            color: card.accent
            elide: Text.ElideRight
        }
        Text {
            width: parent.width
            text: card.heading
            textFormat: Text.PlainText
            font.family: card.titleFont
            font.pixelSize: 32 * card.uiScale
            color: "#f2ece0"
            elide: Text.ElideRight
            maximumLineCount: 1
        }
        Text {
            visible: card.message.length > 0
            width: parent.width
            text: card.message
            textFormat: Text.PlainText
            font.family: card.detailFont
            font.pixelSize: 19 * card.uiScale
            color: "#e0d4c0"
            wrapMode: Text.Wrap
            maximumLineCount: 2
            elide: Text.ElideRight
        }
    }
}
