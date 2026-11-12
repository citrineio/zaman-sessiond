import QtQuick 2.12

Row {
    id: legend
    property var brand
    property var entries: []
    property string keyFont: brand ? brand.mono : "Sans Serif"
    spacing: 24
    height: 36
    Repeater {
        model: legend.entries
        delegate: Row {
            spacing: 8
            height: legend.height
            MenuButtonDiamond {
                visible: modelData.kind === "face"
                width: visible ? 28 : 0; height: 28
                anchors.verticalCenter: parent.verticalCenter
                activeFaces: [modelData.face || ""]
            }
            MenuShoulderGlyph {
                visible: modelData.kind === "shoulder"
                width: visible ? 40 : 0; height: 30
                anchors.verticalCenter: parent.verticalCenter
                side: modelData.side || "left"
            }
            Text {
                visible: modelData.kind === "key" || modelData.kind === "arrows"
                text: modelData.text || ""
                anchors.verticalCenter: parent.verticalCenter
                font.family: legend.keyFont; font.pixelSize: 18
                color: legend.brand.gold
            }
            Text {
                text: modelData.label
                anchors.verticalCenter: parent.verticalCenter
                font.family: legend.brand.body; font.pixelSize: 18
                color: legend.brand.muted
            }
        }
    }
}
