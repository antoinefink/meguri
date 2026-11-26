import CoreGraphics
import SwiftUI
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct MaskColorRenderingTests {
    @Test(arguments: [
        ("red", [255, 0, 0, 255]),
        ("#abc", [170, 187, 204, 255]),
        ("rgba(255, 0, 0, 0.5)", [128, 0, 0, 128]),
        ("rgb(0 100% 0 / 25%)", [0, 64, 0, 64]),
        ("#1234", [5, 9, 14, 68]),
        ("#0000ff80", [0, 0, 128, 128]),
        ("hsl(120deg 100% 50% / .5)", [0, 128, 0, 128]),
        ("transparent", [0, 0, 0, 0])
    ]) func importedCanvasFillRendersColorAndAlphaWithoutRewriting(sample: (String, [Int])) throws {
        let (fill, expected) = sample
        let source = "{{c1::image-occlusion:rect:left=.1:top=.1:width=.8:height=.8:fill=\(fill):oi=1}}"
        let size = CGSize(width: 100, height: 100)
        let session = MaskEditorSession(field: source, isEditing: true)
        session.setReferenceSizeIfNeeded(imageSize: size, area: size, displayScale: 1)
        #expect(session.isReadable)
        #expect(session.referenceSize == size)
        let document = session.document
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        let context = try #require(CGContext(data: nil, width: 100, height: 100, bitsPerComponent: 8,
            bytesPerRow: 400, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGBitmapInfo.byteOrder32Big.rawValue | CGImageAlphaInfo.premultipliedLast.rawValue))
        // Read premultiplied RGBA bytes in sRGB, starting with a clear bitmap.
        context.clear(.init(origin: .zero, size: size))
        let viewport = CanvasViewport(reference: session.referenceSize, area: size)
        // Render the same drawing layer as MaskCanvas, without AppKit representables.
        let canvas = MaskCanvasDrawing(document: session.displayed, viewport: viewport, translucent: false,
            polygon: session.polygon, pointer: nil, selection: session.selection, selectionBox: nil)
        let renderer = ImageRenderer(content: canvas.frame(width: 100, height: 100))
        renderer.scale = 1
        renderer.isOpaque = false
        renderer.colorMode = .nonLinear
        let rendered = try #require(renderer.cgImage)
        context.draw(rendered, in: .init(x: 0, y: 0, width: 100, height: 100))
        let bytes = try #require(context.data).assumingMemoryBound(to: UInt8.self)
        // Away from the number, border, selection and handles.
        let offset = 30 * 400 + 30 * 4
        for channel in 0..<4 {
            #expect(abs(Int(bytes[offset + channel]) - expected[channel]) <= 2)
        }
        // Outside the mask stays transparent, including for opaque imported fills.
        let outside = 5 * 400 + 5 * 4
        #expect((0..<4).allSatisfy { bytes[outside + $0] == 0 })
        #expect(Array(session.field.utf8) == Array(source.utf8))
        #expect(session.document == document && !session.isDirty && !session.canUndo && writes == 0)
    }

    @Test func cssNamesHueUnitsAndInvalidValues() throws {
        #expect(MaskCSSColor("RebeccaPurple") == MaskCSSColor("#663399"))
        #expect(MaskCSSColor(" grey ") == MaskCSSColor("#808080"))
        #expect(MaskCSSColor("hsl(.5turn,100%,50%)") == MaskCSSColor("cyan"))
        #expect(MaskCSSColor("hsl(200grad 100% 50%)") == MaskCSSColor("cyan"))
        #expect(MaskCSSColor("hwb(0 0% 0%)") == MaskCSSColor("red"))
        #expect(MaskCSSColor("rgba(300,-1,0,2)") == MaskCSSColor("red"))
        for value in ["#12", "#ggg", "rgb(NaN 0 0)", "rgba(1 2)", "not-a-color"] {
            #expect(MaskCSSColor(value) == nil)
        }
    }
}
