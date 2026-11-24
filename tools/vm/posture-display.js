// MG guest helper: inspect only the two simctl PNGs; return no pixels.
ObjC.import('AppKit');
function run(paths) {
    return JSON.stringify(paths.map(function (path, index) {
        var image = $.NSBitmapImageRep.imageRepWithData($.NSData.dataWithContentsOfFile(path));
        if (!image || image.pixelsWide <= 32 || image.pixelsHigh <= 32) throw Error('display-decode');
        var lit = false;
        for (var y = 0; y < image.pixelsHigh; y += Math.max(1, Math.floor(image.pixelsHigh / 64))) {
            for (var x = 0; x < image.pixelsWide; x += Math.max(1, Math.floor(image.pixelsWide / 64))) {
                var color = image.colorAtXY(x, y).colorUsingColorSpace($.NSColorSpace.deviceRGBColorSpace);
                if (!color) throw Error('display-color');
                if (Math.max(color.redComponent, color.greenComponent, color.blueComponent) > 0.04) lit = true;
            }
        }
        return {id: index === 0 ? 1 : 3, width: image.pixelsWide, height: image.pixelsHigh, lit: lit};
    }));
}
