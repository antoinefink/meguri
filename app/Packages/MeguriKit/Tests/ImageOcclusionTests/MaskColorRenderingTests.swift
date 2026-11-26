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
        let session = OcclusionDocumentTests().session(source, editing: true)
        #expect(session.isReadable)
        let context = try #require(CGContext(data: nil, width: 100, height: 100, bitsPerComponent: 8,
            bytesPerRow: 400, space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let blank = try #require(context.makeImage())
        let viewport = CanvasViewport(reference: .init(width: 100, height: 100), area: .init(width: 100, height: 100))
        let canvas = MaskCanvas(image: Image(decorative: blank, scale: 1), session: session,
            viewport: .constant(viewport), translucent: false, fill: "#ffffff", editLabel: { _ in },
            focusCanvas: {})
        let renderer = ImageRenderer(content: canvas.frame(width: 100, height: 100))
        renderer.scale = 1
        let rendered = try #require(renderer.cgImage)
        context.draw(rendered, in: .init(x: 0, y: 0, width: 100, height: 100))
        let bytes = try #require(context.data).assumingMemoryBound(to: UInt8.self)
        // Away from the number, border, selection and handles.
        let offset = 30 * 400 + 30 * 4
        for channel in 0..<4 {
            #expect(abs(Int(bytes[offset + channel]) - expected[channel]) <= 2)
        }
        #expect(Array(session.field.utf8) == Array(source.utf8) && !session.canUndo)
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
