import QtQuick 2.12

QtObject {
    // Recovered from Quickshell menu revision 11 (2026-10-08).
    readonly property color ink: "#080C13"
    readonly property color gold: "#EAC28D"
    readonly property color ivory: "#F0E5D6"
    readonly property color text: "#ECEEF1"
    readonly property color caramel: "#CB986C"
    readonly property color panel: "#12161E"
    readonly property color panelTop: "#171D28"
    readonly property color panelBottom: "#17191F"
    readonly property color muted: "#AEB8C5"
    readonly property color dim: "#97A2B1"
    readonly property color line: "#405D718C"
    readonly property color divider: "#265C6E85"
    readonly property color focusBorder: "#F4D2A4"
    readonly property color selectedText: "#211E1B"
    readonly property color selectedDetail: "#594433"
    readonly property color blue: "#88A8C9"
    readonly property color logo: "#EEE1CF"
    readonly property string heading: "Sans Serif"
    readonly property string body: "Sans Serif"
    readonly property string mono: monoFont.name
    property FontLoader monoFont: FontLoader { source: "../assets/fonts/IBMPlexMono-Regular.ttf" }
}
