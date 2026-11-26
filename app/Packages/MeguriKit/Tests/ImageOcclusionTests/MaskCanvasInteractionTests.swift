import CoreGraphics
import Foundation
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct MaskCanvasInteractionTests {
    @Test(arguments: [
        " \t{{c6::image-occlusion:polygon:left=.19:top=.2:points=.2,.2 .4,.2 .3,.4:oi=1}}<BR/>\n",
        "{{c2::image-occlusion:rect:left=-.1:top=.2:width=.3:height=.2:oi=1}}"
    ]) func clickPreservesImportedGeometryAndExactFieldBytes(source: String) throws {
        let session = OcclusionDocumentTests().session(source, editing: true)
        let interaction = MaskCanvasInteraction()
        let original = session.document
        let shape = try #require(original.shapes.first)
        let box = shape.bounds(in: session.referenceSize)
        let point = CGPoint(x: box.midX, y: box.midY)
        var writes: [String] = []
        session.onWrite = { writes.append($0) }
        interaction.changed(start: point, point: point, extending: false, session: session)
        #expect(session.selection == [shape.id])
        #expect(session.preview == nil)
        _ = interaction.ended(point: point, fill: "#000000", closingDistance: 5, session: session)
        #expect(session.document == original)
        #expect(Array(session.field.utf8) == Array(source.utf8))
        #expect(writes.isEmpty && !session.canUndo)
        // A real drag still transforms the imported geometry and writes once.
        let end = CGPoint(x: point.x + 20, y: point.y + 10)
        interaction.changed(start: point, point: end, extending: false, session: session)
        _ = interaction.ended(point: end, fill: "#000000", closingDistance: 5, session: session)
        #expect(session.document != original && writes.count == 1 && session.canUndo)
        session.undo()
        #expect(session.field == source)
    }

    @Test(arguments: [false, true]) func navigationCancelsEveryHandleCallback(rotation: Bool) throws {
        let source = "{{c2::image-occlusion:rect:left=.2:top=.2:width=.3:height=.2:oi=1}}"
        let session = OcclusionDocumentTests().session(source, editing: true)
        session.selectAll()
        let interaction = MaskCanvasInteraction()
        let original = session.document
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        func update(finishing: Bool) {
            if rotation { interaction.rotate(degrees: 30, finishing: finishing, session: session) }
            else { interaction.resize(.bottomRight, delta: .init(width: 80, height: 70), finishing: finishing, session: session) }
        }
        update(finishing: false)
        #expect(session.preview != nil && session.displayed != original)
        // CanvasNavigationInput calls this as soon as the second finger arrives.
        interaction.navigationStarted(session: session)
        #expect(session.preview == nil && session.displayed == original)
        update(finishing: false)
        #expect(session.preview == nil)
        interaction.navigationEnded()
        // Late SwiftUI events from that same gesture must remain cancelled.
        update(finishing: false)
        update(finishing: true)
        #expect(session.document == original && session.field == source)
        #expect(!session.canUndo && writes == 0)
        // Cancellation ends with the gesture; the next handle drag works.
        update(finishing: false)
        update(finishing: true)
        #expect(session.document != original && session.canUndo && writes == 1)
    }

    @Test func cancelledHandleWithoutFinalCallbackDoesNotCancelTheNextGesture() {
        let session = OcclusionDocumentTests().session(OcclusionDocumentTests().rect1, editing: true)
        session.selectAll()
        let interaction = MaskCanvasInteraction()
        interaction.rotate(degrees: 30, finishing: false, session: session)
        interaction.navigationStarted(session: session)
        interaction.handleGestureEnded(session: session)
        interaction.navigationEnded()
        // A late final callback cannot create a new transform.
        interaction.rotate(degrees: 30, finishing: true, session: session)
        #expect(!session.canUndo)
        interaction.rotate(degrees: 15, finishing: false, session: session)
        interaction.rotate(degrees: 15, finishing: true, session: session)
        #expect(session.canUndo && session.document.shapes[0].data.angle == 15)
    }

    @Test func navigationCancelsMoveAndRestoresSelectionWithoutWriting() {
        let session = OcclusionDocumentTests().session(OcclusionDocumentTests().rect1, editing: true)
        let interaction = MaskCanvasInteraction()
        let original = session.document
        interaction.changed(start: .init(x: 150, y: 150), point: .init(x: 190, y: 180), extending: false, session: session)
        #expect(session.preview != nil)
        interaction.navigationStarted(session: session)
        interaction.navigationEnded()
        interaction.changed(start: .init(x: 150, y: 150), point: .init(x: 200, y: 200), extending: false, session: session)
        _ = interaction.ended(point: .init(x: 200, y: 200), fill: "red", closingDistance: 5, session: session)
        #expect(session.document == original && session.preview == nil && session.selection.isEmpty)
        #expect(!session.canUndo)
    }

    @Test(arguments: [2, 3]) func accessibilityGroupKeepsTargetAndEntireSelection(count: Int) throws {
        let source = (1...count).map { number in
            "{{c\(number)::image-occlusion:rect:left=0.\(number):top=.2:width=.05:height=.05:oi=1}}"
        }.joined(separator: "<br>")
        let session = OcclusionDocumentTests().session(source, editing: true)
        session.selectAll()
        let selected = session.selection
        let target = try #require(session.document.accessibilityItems(in: CanvasViewport(reference: session.referenceSize, area: session.referenceSize)).first)
        #expect(session.canGroup(including: target.ids))
        // The production VoiceOver action uses this method, without toggling.
        session.group(including: target.ids)
        #expect(session.selection == selected)
        #expect(session.document.items.count == 1 && session.document.items[0].count == count)
        #expect(!session.canGroup(including: target.ids) && session.canUngroup)
        session.undo()
        #expect(session.field == source)
    }

    @Test func accessibilityGroupIncludesUnselectedTargetAndHidesInapplicableAction() throws {
        let session = OcclusionDocumentTests().session(OcclusionDocumentTests().rect1, editing: true)
        let id = try #require(session.document.shapes.first?.id)
        #expect(!session.canGroup(including: [id]))
        session.group(including: [id])
        #expect(session.selection.isEmpty && !session.canUndo)
        session.addBox(from: .init(x: 400, y: 400), to: .init(x: 450, y: 450))
        #expect(session.canGroup(including: [id]))
        session.group(including: [id])
        #expect(session.selection.count == 2 && session.document.items.count == 1)
    }
}
