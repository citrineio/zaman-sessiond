import QtQuick 2.12

Rectangle {
    property var brand
    property bool selected: false
    radius: 13
    border.width: 1
    border.color: selected ? brand.focusBorder : brand.line
    gradient: Gradient {
        orientation: Gradient.Horizontal
        GradientStop { position: 0; color: selected ? brand.gold : brand.panelTop }
        GradientStop { position: 1; color: selected ? brand.caramel : brand.panel }
    }
}
