import QtQuick

Item {
    id: value
    property string text: ""
    property color color: "#BBC6D3"
    property real fontSize: 14
    property real uiScale: 1
    property int direction: 1
    property bool animate: true
    property bool initialized: false
    property real slide: 0
    implicitHeight: label.implicitHeight
    implicitWidth: label.implicitWidth
    clip: true
    MenuText {
        id: label
        width: parent.width
        x: value.slide
        text: value.text
        color: value.color
        font.pixelSize: value.fontSize
        elide: Text.ElideRight
    }
    onTextChanged: if (initialized && visible && animate) {
        settle.stop();
        slide = (direction < 0 ? -1 : 1) * Math.round(7 * uiScale);
        settle.restart();
    }
    NumberAnimation {
        id: settle
        target: value; property: "slide"; to: 0
        duration: 110; easing.type: Easing.OutCubic
    }
    Component.onCompleted: initialized = true
}
