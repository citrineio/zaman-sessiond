import QtQuick

Text {
    // Render at the actual font size rather than scaling a text texture.
    renderType: Text.NativeRendering
    font.hintingPreference: Font.PreferVerticalHinting
    font.family: "Sans Serif"
    font.pixelSize: 22
    color: "#F1F2F4"
    height: Math.ceil(implicitHeight)
    textFormat: Text.PlainText
}
