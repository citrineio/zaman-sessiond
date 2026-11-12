import QtQuick 2.12
import "../lib/Library.js" as Library

// Advance through usable roles when a file is missing or a remote shot fails.
Item {
    id: artwork
    property var game
    property var roles: []
    property bool enabledArtwork: true
    readonly property var candidates: enabledArtwork ? Library.artwork(game, roles) : []
    property int candidateIndex: 0
    readonly property bool ready: image.status === Image.Ready
    readonly property bool fitted: candidates.length > candidateIndex && candidates[candidateIndex].fit
    onCandidatesChanged: candidateIndex = 0
    clip: true
    Image {
        id: image
        anchors.fill: parent
        anchors.margins: artwork.fitted ? 16 : 0
        source: artwork.candidates.length > artwork.candidateIndex ? artwork.candidates[artwork.candidateIndex].url : ""
        sourceSize.width: Math.ceil(artwork.width * 1.5)
        sourceSize.height: Math.ceil(artwork.height * 1.5)
        asynchronous: true
        fillMode: artwork.fitted ? Image.PreserveAspectFit : Image.PreserveAspectCrop
        visible: status === Image.Ready
        onStatusChanged: {
            if (status === Image.Error && artwork.candidateIndex + 1 < artwork.candidates.length)
                artwork.candidateIndex++;
        }
    }
}
