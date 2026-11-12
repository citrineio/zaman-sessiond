import QtQuick 2.12
import "../lib/Library.js" as Library

SelectionSurface {
    id: card
    property var game
    property bool reducedMotion: true
    signal chosen()
    signal opened()
    Rectangle {
        id: artwork
        x: 8; y: 8; width: parent.width - 16; height: parent.height - 88
        radius: 8; color: brand.panelBottom; clip: true
        Text {
            objectName: "cardArtworkFallback"
            visible: !cardArt.ready
            anchors.centerIn: parent; width: parent.width - 36
            text: game ? game.title : ""; wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter; maximumLineCount: 3; elide: Text.ElideRight
            font.family: brand.heading; font.weight: Font.DemiBold; font.pixelSize: Math.min(30, parent.width / 8)
            color: brand.ivory
        }
        GameArtwork {
            id: cardArt
            objectName: "cardArtwork"
            anchors.fill: parent; anchors.margins: 2
            game: card.game
            roles: ["banner", "steam", "screenshot", "background", "logo", "boxFront", "poster", "tile"]
        }
        Rectangle {
            visible: game ? game.favorite : false
            anchors.right: parent.right; anchors.top: parent.top; anchors.margins: 10
            width: 32; height: 32; radius: 10; color: brand.panel
            border.width: 1; border.color: brand.line
            Text { anchors.centerIn: parent; text: "★"; color: brand.gold; font.pixelSize: 21 }
        }
    }
    Text {
        x: 16; y: artwork.y + artwork.height + 11; width: parent.width - 32
        text: game ? game.title : ""; elide: Text.ElideRight
        font.family: brand.heading; font.weight: Font.DemiBold; font.pixelSize: 22
        color: card.selected ? brand.selectedText : brand.text
    }
    Text {
        x: 16; y: artwork.y + artwork.height + 44; width: parent.width - 32
        text: Library.collection(game).toUpperCase(); elide: Text.ElideRight
        font.family: brand.mono; font.pixelSize: 14
        color: card.selected ? brand.selectedDetail : brand.dim
    }
    MouseArea { anchors.fill: parent; onClicked: card.chosen(); onDoubleClicked: card.opened() }
}
