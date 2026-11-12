import QtQuick 2.12

SelectionSurface {
    id: button
    property string label
    property bool primary: false
    readonly property bool highlighted: selected || tap.pressed
    signal activated()
    width: 220; height: 66
    gradient: Gradient {
        orientation: Gradient.Horizontal
        GradientStop { position: 0; color: button.highlighted ? brand.gold : brand.panelTop }
        GradientStop { position: 1; color: button.highlighted ? brand.caramel : brand.panel }
    }
    border.width: selected ? 2 : 1
    border.color: highlighted ? brand.focusBorder : brand.line
    opacity: enabled ? 1 : 0.6
    Text {
        anchors.centerIn: parent
        width: parent.width - 30; horizontalAlignment: Text.AlignHCenter; elide: Text.ElideRight
        text: button.label
        font.family: brand.heading; font.pixelSize: 23
        color: button.highlighted ? brand.selectedText : brand.text
    }
    TapHandler {
        id: tap
        acceptedButtons: Qt.LeftButton
        gesturePolicy: TapHandler.ReleaseWithinBounds
        onTapped: button.activated()
    }
    MouseArea {
        id: hoverArea
        anchors.fill: parent
        acceptedButtons: Qt.NoButton
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
    }
}
