import QtQuick 2.12

Rectangle {
    property var brand
    radius: 24
    border.width: 1
    border.color: brand.line
    gradient: Gradient {
        GradientStop { position: 0; color: brand.panelTop }
        GradientStop { position: 0.45; color: brand.panel }
        GradientStop { position: 1; color: brand.panelBottom }
    }
}
