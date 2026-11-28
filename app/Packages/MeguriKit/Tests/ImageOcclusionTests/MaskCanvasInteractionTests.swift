import CoreGraphics
import Foundation
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct MaskCanvasInteractionTests {
    @Test(arguments: [MaskEditorTool.rectangle, .ellipse], [false, true])
    func drawingToolClickDeselectsWithoutEditingAndCanDrawAgain(tool: MaskEditorTool, native: Bool) throws {
        let session = OcclusionDocumentTests().session()
        session.tool = tool
        let interaction = MaskCanvasInteraction()
        let viewport = CanvasViewport(reference: session.referenceSize, area: session.referenceSize)
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        func gesture(from start: CGPoint, through points: [CGPoint], to end: CGPoint) {
            if native {
                interaction.nativeBegan(at: start, viewport: viewport, extending: false, session: session)
                for point in points { interaction.nativeChanged(at: point, viewport: viewport, session: session) }
                _ = interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session)
            } else {
                interaction.changed(start: start, point: start, extending: false, session: session)
                for point in points { interaction.changed(start: start, point: point, extending: false, session: session) }
                _ = interaction.ended(point: end, fill: "#000000", closingDistance: 5, session: session)
            }
        }
        let firstEnd = CGPoint(x: 250, y: 230)
        gesture(from: .init(x: 100, y: 100), through: [firstEnd], to: firstEnd)
        let first = try #require(session.document.shapes.first)
        #expect(session.document.shapes.count == 1 && session.selection == [first.id])
        #expect(session.tool == tool && session.canUndo && !session.canRedo && writes == 1)
        let document = session.document, field = session.field
        let blank = CGPoint(x: 700, y: 700), tiny = CGPoint(x: 702, y: 703)
        let outside = CGPoint(x: -20, y: 700)
        // Sub-minimum drags, including an out-and-back path or movement only
        // at release, and clicks outside the image must keep the selection.
        let noOps: [(CGPoint, [CGPoint], CGPoint)] = [
            (blank, [tiny], tiny), (blank, [tiny, blank], blank),
            (blank, [], tiny), (outside, [], outside)
        ]
        for (start, points, end) in noOps {
            gesture(from: start, through: points, to: end)
            #expect(session.selection == [first.id])
            #expect(session.document == document && session.field == field && writes == 1)
        }
        gesture(from: blank, through: [], to: blank)
        #expect(session.selection.isEmpty && session.preview == nil)
        #expect(session.document == document && session.field == field && writes == 1)
        #expect(session.tool == tool && session.canUndo && !session.canRedo)
        let secondEnd = CGPoint(x: 850, y: 830)
        gesture(from: blank, through: [secondEnd], to: secondEnd)
        let second = try #require(session.document.shapes.last)
        #expect(session.document.shapes.count == 2 && second.id != first.id)
        #expect(session.selection == [second.id] && session.tool == tool && writes == 2)
        // Exactly two undo steps: neither the no-ops nor deselection added one.
        session.undo()
        #expect(session.document == document && session.field == field && session.canUndo)
        session.undo()
        #expect(session.document.shapes.isEmpty && session.field.isEmpty && !session.canUndo)
    }

    @Test(arguments: [false, true], [false, true])
    func boxDraftShowsCommittedOrdinalIncludingGap(ellipse: Bool, gap: Bool) throws {
        let session = gap ? OrdinalAllocatorTests().model() : OcclusionDocumentTests().session()
        session.tool = ellipse ? .ellipse : .rectangle
        let interaction = MaskCanvasInteraction()
        let original = session.document, field = session.field
        let expected: UInt16 = gap ? 3 : 1
        let start = CGPoint(x: 700, y: 700), end = CGPoint(x: 850, y: 830)
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        interaction.changed(start: start, point: start, extending: false, session: session)
        #expect(session.preview == nil && !session.canUndo && writes == 0)
        for point in [CGPoint(x: 720, y: 720), end] {
            interaction.changed(start: start, point: point, extending: false, session: session)
            #expect(try #require(session.preview?.shapes.last).data.number == expected)
            #expect(session.document == original && session.field == field && !session.canUndo && writes == 0)
        }
        _ = interaction.ended(point: end, fill: "#000000", closingDistance: 5, session: session)
        #expect(session.document.shapes.last?.data.number == expected)
        #expect(session.preview == nil && session.canUndo && writes == 1)
        #expect(session.document.shapes.dropLast() == original.shapes[...])
    }

    @Test(arguments: [false, true])
    func tinyAndCancelledBoxDraftsDoNotReserveAnOrdinal(ellipse: Bool) throws {
        let session = OcclusionDocumentTests().session()
        session.tool = ellipse ? .ellipse : .rectangle
        let interaction = MaskCanvasInteraction()
        let start = CGPoint(x: 700, y: 700), tiny = CGPoint(x: 702, y: 703), end = CGPoint(x: 800, y: 800)
        interaction.changed(start: start, point: tiny, extending: false, session: session)
        #expect(try #require(session.preview?.shapes.last).data.number == 1)
        _ = interaction.ended(point: tiny, fill: "#000000", closingDistance: 5, session: session)
        #expect(session.document.shapes.isEmpty && !session.canUndo && session.field.isEmpty)
        interaction.changed(start: start, point: end, extending: false, session: session)
        #expect(try #require(session.preview?.shapes.last).data.number == 1)
        interaction.navigationStarted(session: session)
        interaction.navigationEnded()
        _ = interaction.ended(point: end, fill: "#000000", closingDistance: 5, session: session)
        #expect(session.preview == nil && session.document.shapes.isEmpty && !session.canUndo)
        // A completed edit after cancellation takes 1. The next draft must not
        // reuse the cancelled gesture's cached allocation.
        session.addBox(from: .init(x: 10, y: 10), to: .init(x: 50, y: 50))
        interaction.changed(start: start, point: end, extending: false, session: session)
        #expect(try #require(session.preview?.shapes.last).data.number == 2)
        _ = interaction.ended(point: end, fill: "#000000", closingDistance: 5, session: session)
        #expect(session.document.shapes.map { $0.data.number } == [1, 2])
    }

    @Test func boxReleaseReallocatesIfDocumentChangedDuringDraft() throws {
        let session = OcclusionDocumentTests().session()
        let interaction = MaskCanvasInteraction()
        let start = CGPoint(x: 700, y: 700), end = CGPoint(x: 800, y: 800)
        interaction.changed(start: start, point: end, extending: false, session: session)
        #expect(try #require(session.preview?.shapes.last).data.number == 1)
        session.addBox(from: .init(x: 10, y: 10), to: .init(x: 50, y: 50))
        _ = interaction.ended(point: end, fill: "#000000", closingDistance: 5, session: session)
        #expect(session.document.shapes.map { $0.data.number } == [1, 2])
    }

    @Test(arguments: ["tool", "source", "image"], [false, true])
    func nativeInputChangeCancelsLateEvents(change: String, drag: Bool) throws {
        let session = OcclusionDocumentTests().session(OcclusionDocumentTests().rect1, editing: true)
        session.tool = .rectangle
        session.selectAll()
        let size = session.referenceSize
        let viewport = CanvasViewport(reference: size, area: size)
        let interaction = MaskCanvasInteraction()
        let start = CGPoint(x: 700, y: 700), end = CGPoint(x: 800, y: 800)
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        interaction.nativeBegan(at: start, viewport: viewport, extending: false, session: session)
        interaction.nativeChanged(at: end, viewport: viewport, session: session)
        #expect(session.preview != nil)
        switch change {
        case "tool": session.tool = .ellipse
        case "source":
            session.receive("{{c2::image-occlusion:rect:left=.2:top=.2:width=.1:height=.1:oi=1}}", isEditing: true)
        case "image": session.resetForImageChange()
        default: Issue.record("Unknown input change"); return
        }
        let document = session.document, field = session.field, selection = session.selection
        if drag {
            interaction.nativeChanged(at: end, viewport: viewport, session: session)
            #expect(session.preview == nil && interaction.nativeCue == nil)
            #expect(session.document == document && session.field == field && session.selection == selection && writes == 0)
        }
        #expect(interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session) == nil)
        #expect(session.document == document && session.field == field && writes == 0)
        #expect(session.preview == nil && session.selection == selection && !session.canUndo)
        #expect(interaction.selectionBox == nil && interaction.nativeCue == nil)

        session.setReferenceSizeIfNeeded(imageSize: size, area: size, displayScale: 1)
        interaction.nativeBegan(at: start, viewport: viewport, extending: false, session: session)
        interaction.nativeChanged(at: end, viewport: viewport, session: session)
        #expect(session.preview != nil)
        _ = interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session)
        #expect(session.document.shapes.count == document.shapes.count + 1 && writes == 1 && session.canUndo)
        let added = try #require(session.document.shapes.last)
        switch added.data.kind {
        case .ellipse: #expect(change == "tool")
        case .rect: #expect(change != "tool")
        default: Issue.record("Expected a box")
        }
    }

    @Test(arguments: [false, true])
    func nativeLabelDoubleClickWaitsForStationaryRelease(drag: Bool) throws {
        let session = OcclusionDocumentTests().session(
            "{{c0::image-occlusion:text:left=.2:top=.2:text=Label:scale=1}}", editing: true)
        session.tool = .select
        let shape = try #require(session.document.shapes.first)
        let box = shape.bounds(in: session.referenceSize)
        let point = CGPoint(x: box.midX, y: box.midY)
        let viewport = CanvasViewport(reference: session.referenceSize, area: session.referenceSize)
        let interaction = MaskCanvasInteraction()
        let original = session.document, field = session.field
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        interaction.nativeBegan(at: point, viewport: viewport, extending: false, session: session)
        #expect(interaction.nativeEnded(at: point, viewport: viewport, fill: "#000000", session: session) == nil)
        #expect(session.selection == [shape.id])
        interaction.nativeBegan(at: point, viewport: viewport, extending: false, clickCount: 2, session: session)
        #expect(session.document == original && session.field == field && writes == 0)
        let end = drag ? CGPoint(x: point.x + 30, y: point.y + 20) : point
        let expected = session.moved(dx: 30, dy: 20, finishing: true)
        if drag { interaction.nativeChanged(at: end, viewport: viewport, session: session) }
        let label = interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session)
        #expect(label == (drag ? nil : shape.id))
        #expect(session.selection == [shape.id] && session.preview == nil)
        #expect(session.document == (drag ? expected : original))
        #expect(writes == (drag ? 1 : 0) && session.canUndo == drag)
        // Completion and cancellation both discard the pending label edit.
        #expect(interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session) == nil)
        interaction.nativeBegan(at: end, viewport: viewport, extending: false, clickCount: 2, session: session)
        interaction.nativeCancelled(session: session)
        #expect(interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session) == nil)
    }

    @Test func nativeHandlePressIsReadOnlyAndUsesNearestSharedGeometry() throws {
        let session = OcclusionDocumentTests().session(OcclusionDocumentTests().rect1, editing: true)
        session.selectAll()
        let viewport = CanvasViewport(reference: session.referenceSize, area: session.referenceSize)
        let original = session.document, field = session.field
        let interaction = MaskCanvasInteraction()
        for geometry in session.canvasHandles(in: viewport) {
            #expect(session.canvasHandle(at: geometry.center, in: viewport)?.target == geometry.target)
            interaction.nativeBegan(at: geometry.center, viewport: viewport, extending: false, session: session)
            _ = interaction.nativeEnded(at: geometry.center, viewport: viewport, fill: "#000000", session: session)
            #expect(session.document == original && session.field == field && !session.canUndo)
        }
        let corner = try #require(session.canvasHandles(in: viewport).first { $0.target == .resize(.bottomRight) })
        let end = CGPoint(x: corner.center.x + 30, y: corner.center.y + 20)
        let expected = session.resized(handle: .bottomRight, delta: .init(width: 30, height: 20), finishing: true)
        interaction.nativeBegan(at: corner.center, viewport: viewport, extending: false, session: session)
        interaction.nativeChanged(at: end, viewport: viewport, session: session)
        #expect(session.preview != nil && !session.canUndo)
        _ = interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session)
        #expect(session.document == expected)
        #expect(session.document != original && session.canUndo)
    }

    @Test func importedPolygonHandlesAndCursorsStayAxisAligned() throws {
        let session = OcclusionDocumentTests().session(
            "{{c1::image-occlusion:polygon:left=.3:top=.3:points=.3,.3 .6,.3 .35,.5:angle=90:oi=1}}", editing: true)
        let viewport = CanvasViewport(reference: session.referenceSize, area: session.referenceSize)
        #expect(session.canvasHandles(in: viewport).isEmpty)
        session.selectAll()
        let box = try #require(session.document.bounds(of: session.selection, in: session.referenceSize))
        for geometry in session.canvasHandles(in: viewport) {
            if case let .resize(handle) = geometry.target {
                #expect(geometry.center == viewport.screenPoint(handle.point(in: box)))
                #expect(session.resizeCursorHandle(handle) == handle)
            }
        }
    }

    @Test func nativeRotationUsesCommittedPivotAndCancellationFencesLateCallbacks() throws {
        let session = OcclusionDocumentTests().session(
            "{{c1::image-occlusion:polygon:left=.3:top=.3:points=.3,.3 .6,.3 .35,.5:oi=1}}", editing: true)
        session.selectAll()
        let viewport = CanvasViewport(reference: session.referenceSize, area: session.referenceSize)
        let interaction = MaskCanvasInteraction()
        let handle = try #require(session.canvasHandles(in: viewport).first { $0.target == .rotate })
        let pivot = try #require(session.rotationPivot)
        let expected = session.rotated(degrees: 40, finishing: true)
        let original = session.document
        let radians = 40.0 * .pi / 180
        let dx = handle.center.x - pivot.x, dy = handle.center.y - pivot.y
        let end = CGPoint(x: pivot.x + dx * cos(radians) - dy * sin(radians),
                          y: pivot.y + dx * sin(radians) + dy * cos(radians))
        interaction.nativeBegan(at: handle.center, viewport: viewport, extending: false, session: session)
        #expect(interaction.nativeCue == .rotate)
        interaction.nativeChanged(at: end, viewport: viewport, session: session)
        #expect(session.preview != nil)
        interaction.nativeCancelled(session: session)
        interaction.nativeChanged(at: end, viewport: viewport, session: session)
        _ = interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session)
        #expect(session.document == original && session.preview == nil && !session.canUndo)
        interaction.nativeBegan(at: handle.center, viewport: viewport, extending: false, session: session)
        interaction.nativeChanged(at: end, viewport: viewport, session: session)
        _ = interaction.nativeEnded(at: end, viewport: viewport, fill: "#000000", session: session)
        // Trigonometric round-off is harmless; compare drawn points within a
        // reference-point tolerance rather than requiring bitwise equality.
        guard case let .polygon(actualPoints) = session.document.shapes[0].data.kind,
              case let .polygon(expectedPoints) = expected.shapes[0].data.kind else {
            Issue.record("Expected rotated polygons"); return
        }
        #expect(actualPoints.count == expectedPoints.count)
        for (actual, expected) in zip(actualPoints, expectedPoints) {
            #expect(abs(actual.x - expected.x) < 0.000001 && abs(actual.y - expected.y) < 0.000001)
        }
        #expect(session.canUndo)
    }

    @Test func overlappingHandlesKeepSmallMaskInteriorMovableAndMultiSelectionMovesTogether() throws {
        let session = OcclusionDocumentTests().session(
            "{{c1::image-occlusion:rect:left=.1:top=.1:width=.01:height=.01:oi=1}}", editing: true)
        session.selectAll()
        let viewport = CanvasViewport(reference: session.referenceSize, area: session.referenceSize)
        #expect(session.canvasHandle(at: .init(x: 105, y: 105), in: viewport) == nil)
        #expect(session.canvasHandle(at: .init(x: 110, y: 110), in: viewport)?.target == .resize(.bottomRight))
        #expect(session.canvasHandle(at: .init(x: 110, y: 105), in: viewport)?.target == .resize(.right))
        session.addBox(from: .init(x: 400, y: 400), to: .init(x: 500, y: 500))
        session.selectAll()
        let original = session.document, selected = session.selection
        let interaction = MaskCanvasInteraction()
        interaction.nativeBegan(at: .init(x: 105, y: 105), viewport: viewport, extending: false, session: session)
        #expect(session.selection == selected)
        interaction.nativeChanged(at: .init(x: 125, y: 135), viewport: viewport, session: session)
        let expected = session.moved(dx: 20, dy: 30, finishing: true)
        _ = interaction.nativeEnded(at: .init(x: 125, y: 135), viewport: viewport, fill: "#000000", session: session)
        #expect(session.document == expected && session.document != original && session.selection == selected)
    }

    @Test(arguments: [
        " \t{{c6::image-occlusion:polygon:left=.19:top=.2:points=.2,.2 .4,.2 .3,.4:oi=1}}<BR/>\n",
        "{{c2::image-occlusion:rect:left=-.1:top=.2:width=.3:height=.2:oi=1}}",
        "{{c0::image-occlusion:rect:left=.1:top=.2:width=.3:height=.2:oi=1}}"
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
        #expect(session.preview == nil && writes.isEmpty && !session.canUndo)
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

    @Test func asymmetricPolygonRotationKeepsCommittedPivotThroughPreviewAndFinish() throws {
        let session = OcclusionDocumentTests().session(
            "{{c1::image-occlusion:polygon:left=.3:top=.3:points=.3,.3 .6,.3 .35,.5:oi=1}}", editing: true)
        session.selectAll()
        let pivot = try #require(session.rotationPivot)
        #expect(pivot == CGPoint(x: 450, y: 400))
        let expected = session.rotated(degrees: 40, finishing: true)
        let interaction = MaskCanvasInteraction()
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        for degrees in [20.0, 40.0] {
            interaction.rotate(degrees: degrees, finishing: false, session: session)
            let previewBox = try #require(session.displayed.bounds(of: session.selection, in: session.referenceSize))
            #expect(CGPoint(x: previewBox.midX, y: previewBox.midY) != pivot)
            #expect(session.rotationPivot == pivot)
        }
        interaction.rotate(degrees: 40, finishing: true, session: session)
        #expect(session.document == expected && session.preview == nil)
        #expect(writes == 1 && session.canUndo)
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
