import Foundation
import Testing

/// The hosted web view helper on a sample card face (R1975): an image
/// snapshot of the rendered page and a text snapshot of its DOM after
/// layout. The page is Meguri's own sample, not a card template; the
/// rendering corpus comes with the rendering units (U66, U67).
@MainActor
@Suite(.serialized, .enabled(if: SnapshotRun.current.group == MatrixCell(device: SnapshotMatrix.referenceDevice(.current)).systemGroup))
struct CardFaceSnapshotTests {
    static let sample = """
        <!doctype html>
        <html><head><meta charset="utf-8"><meta name="color-scheme" content="light dark">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>
        body { font: -apple-system-body; margin: 24px; text-align: center; color: CanvasText; background: Canvas; }
        .front { font-size: 2em; }
        hr { border: none; border-top: 1px solid GrayText; margin: 24px 0; }
        </style></head>
        <body class="card"><div class="front">桜</div><hr id="answer"><div class="back">sakura — cherry blossom</div></body></html>
        """

    nonisolated static var cells: [MatrixCell] {
        Appearance.allCases.map { MatrixCell(appearance: $0, device: SnapshotMatrix.referenceDevice(.current)) }
    }

    @Test(arguments: cells)
    func sampleCardFace(cell: MatrixCell) async throws {
        var dom = ""
        await assertImageSnapshot(subject: "cardface.sample", state: "typical", cell: cell, layout: .screen) {
            let capture = try await WebViewSnapshot.capture(html: Self.sample, cell: cell)
            dom = capture.dom
            return capture.image
        }
        guard !dom.isEmpty else { return }
        assertTextSnapshot(dom, subject: "cardface.sample", state: "typical", cell: cell)
        #expect(dom.contains("<hr id=\"answer\">"), "the DOM keeps the answer separator")
    }
}
