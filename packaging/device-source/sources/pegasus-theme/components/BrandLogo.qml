import QtQuick 2.12

Item {
    property var brand
    implicitWidth: 292
    implicitHeight: 72
    Image {
        x: 0; y: 5; width: 58; height: 58
        source: "../assets/branding/zaman-bird.png"
        fillMode: Image.PreserveAspectFit
        smooth: true
    }
    Image {
        x: 76; y: 5; width: 182; height: 34
        source: "../assets/branding/zaman-wordmark.png"
        fillMode: Image.PreserveAspectFit
        smooth: true
    }
    Text {
        x: 267; y: 23; text: "OS"
        font.family: "Sans Serif"; font.pixelSize: 13; color: "#C3CEDB"
    }
    Text {
        x: 77; y: 50; text: "PLAY  EXPLORE  BELONG"
        font.family: brand.mono; font.pixelSize: 9
        font.letterSpacing: 1; color: "#9EABB9"
    }
}
