import QtQuick

// Render regions of the original supplied PNG. The source file is unchanged;
// black becomes transparent and white takes the requested UI color at draw time.
Canvas {
    id: mark
    property string part: "bird"
    property color ink: "#EEE1CF"
    // Keep assets inside the shell root: Quickshell blocks ../ traversal.
    readonly property url artwork: Qt.resolvedUrl("assets/zaman-logo-source.png")
    // Tight bounds plus four source pixels of padding to retain edge coverage.
    readonly property rect sourceRegion: part === "wordmark"
        ? Qt.rect(256, 862, 763, 141)
        : Qt.rect(367, 176, 665, 662)
    implicitWidth: part === "wordmark" ? 166 : 50
    implicitHeight: part === "wordmark" ? 31 : 48
    contextType: "2d"
    renderTarget: Canvas.Image
    renderStrategy: Canvas.Immediate
    smooth: true
    antialiasing: true
    property string paintStage: "source"
    property var maskedPixels: null
    // Do not expose the opaque source image during the first paint pass.
    opacity: paintStage === "ready" ? 1 : 0

    function invalidateMask() {
        maskedPixels = null;
        paintStage = "source";
        requestPaint();
    }

    Component.onCompleted: loadImage(artwork)
    onImageLoaded: {
        if (isImageError(artwork)) console.error("Zaman logo failed to load: " + artwork);
        else invalidateMask();
    }
    onAvailableChanged: {
        if (available) {
            if (!isImageLoaded(artwork) && !isImageLoading(artwork)) loadImage(artwork);
            invalidateMask();
        }
    }
    onPartChanged: invalidateMask()
    onInkChanged: invalidateMask()
    onWidthChanged: invalidateMask()
    onHeightChanged: invalidateMask()

    onPaint: dirtyRegion => {
        var ctx = getContext("2d");
        if (!ctx || width <= 0 || height <= 0) return;
        ctx.reset();
        ctx.clearRect(0, 0, width, height);
        if (!isImageLoaded(artwork)) return;
        if (maskedPixels !== null) {
            // Qt's implementation requires the full seven-argument form.
            ctx.putImageData(maskedPixels, 0, 0, 0, 0,
                maskedPixels.width, maskedPixels.height);
            return;
        }

        // Use the qualified source rectangle, not Canvas.paint's dirty region.
        var crop = mark.sourceRegion;
        var scale = Math.min(width / crop.width, height / crop.height);
        var w = crop.width * scale;
        var h = crop.height * scale;
        paintStage = "read";
        ctx.drawImage(artwork.toString(), crop.x, crop.y, crop.width, crop.height,
            (width - w) / 2, (height - h) / 2, w, h);
    }

    onPainted: {
        if (paintStage === "mask") {
            paintStage = "ready";
            return;
        }
        if (paintStage !== "read") return;
        // Read only after the source draw has completed. Setting this guard
        // first also prevents a nested painted signal from reading it again.
        paintStage = "processing";
        var ctx = getContext("2d");
        var pixels = ctx.getImageData(0, 0, Math.ceil(width), Math.ceil(height));
        if (!pixels || pixels.width <= 0 || pixels.height <= 0) {
            console.error("Zaman logo pixel read returned an empty image.");
            return;
        }
        var data = pixels.data;
        var red = Math.round(ink.r * 255);
        var green = Math.round(ink.g * 255);
        var blue = Math.round(ink.b * 255);
        for (var i = 0; i < data.length; i += 4) {
            var coverage = (data[i] + data[i + 1] + data[i + 2]) / 3;
            var alpha = Math.round(coverage * (data[i + 3] / 255) * ink.a);
            data[i] = red;
            data[i + 1] = green;
            data[i + 2] = blue;
            data[i + 3] = alpha;
        }
        maskedPixels = pixels;
        paintStage = "mask";
        Qt.callLater(() => mark.requestPaint());
    }
}
