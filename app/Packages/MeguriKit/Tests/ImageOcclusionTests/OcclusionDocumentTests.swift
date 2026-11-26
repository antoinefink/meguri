import CoreBridge
import Foundation
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct OcclusionDocumentTests {
    let rect1 = "{{c1::image-occlusion:rect:left=.1:top=.1:width=.2:height=.1:oi=1}}"
    let protected = "{{c4::image-occlusion:rect:left=0.30:top=0.10:fill=#ffeba2:width=0.20:height=0.08:zz=q\\:r:oi=1}}"

    func session(_ field: String = "", editing: Bool = false) -> MaskEditorSession {
        let model = MaskEditorSession(field: field, isEditing: editing)
        model.setReferenceSizeIfNeeded(imageSize: .init(width: 1000, height: 1000),
                                       area: .init(width: 1000, height: 1000), displayScale: 1)
        return model
    }

    @Test func openingAndReturningToOriginalPreservesAllBytes() throws {
        let source = " \t" + protected + "<BR/>\n"
        let model = session(source, editing: true)
        #expect(model.isReadable)
        #expect(!model.isDirty && !model.canUndo)
        var writes: [String] = []
        model.onWrite = { writes.append($0) }
        model.selectAll()
        #expect(writes.isEmpty)
        model.commit(model.moved(dx: 10, dy: 20, finishing: true))
        #expect(writes.count == 1)
        model.undo()
        #expect(model.field == source)
        #expect(writes.count == 2)
        #expect(!model.isDirty)
        model.redo()
        #expect(writes.count == 3 && model.isDirty)
    }

    @Test func movingOneShapeLeavesProtectedSourceAndNumbersUnchanged() throws {
        let source = rect1 + "<br>" + protected + "<BR/>"
        let model = session(source, editing: true)
        let first = try #require(model.document.shapes.first)
        model.select(first.id)
        model.commit(model.moved(dx: 25, dy: 0, finishing: true))
        #expect(model.field.contains(protected))
        #expect(model.document.shapes.map { $0.data.number } == [1, 4])
        #expect(model.document.shapes[1].data.sourceIndex == 1)
        #expect(model.document.shapes[1].data.unknownProperties == [.init(key: "zz", value: "q:r")])
    }

    @Test func invalidAndUnsupportedInputIsProtected() {
        for source in ["hello", "{{c1::image-occlusion:star:left=.1:top=.2}}", rect1 + "junk", "{{c1,2::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1}}"] {
            let model = session(source, editing: true)
            #expect(!model.isReadable)
            #expect(model.errorKey != nil)
            model.addBox(from: .init(x: 100, y: 100), to: .init(x: 200, y: 200))
            model.setMode(false); model.undo(); model.redo()
            #expect(model.field == source)
            #expect(!model.canUndo)
        }
    }

    @Test func refusedWriterRollsBackWithoutHistoryOrBindingWrite() {
        let source = "{{c1::image-occlusion:rect:left=.1:top=.1:width=.2:height=.1:zz=a}:oi=1}}"
        let model = session(source, editing: true)
        #expect(model.isReadable)
        var writes = 0
        model.onWrite = { _ in writes += 1 }
        model.setMode(false)
        #expect(model.document.hideAll)
        #expect(model.field == source)
        #expect(!model.canUndo && writes == 0)
        #expect(model.errorKey == "io.editor.unwritable")
    }

    @Test func drawingHasReferencePointThresholdAndSynchronousUpdates() {
        let model = session()
        #expect(model.tool == .rectangle && model.document.hideAll)
        var binding = ""
        model.onWrite = { binding = $0 }
        model.addBox(from: .init(x: 0, y: 0), to: .init(x: 4.9, y: 30))
        #expect(model.document.shapes.isEmpty && !model.canUndo)
        model.addBox(from: .init(x: 30, y: 30), to: .init(x: 25, y: 25))
        #expect(model.document.shapes.count == 1)
        #expect(binding == model.field && binding.contains("{{c1::"))
        #expect(model.document.shapes[0].data.left == 0.025)
        model.addBox(from: .init(x: -1, y: 0), to: .init(x: 50, y: 50))
        #expect(model.document.shapes.count == 1)
        model.addBox(from: .init(x: 100, y: 100), to: .init(x: 140, y: 160), ellipse: true)
        #expect(model.document.shapes[1].data.kind == .ellipse(rx: 0.02, ry: 0.03))
    }

    @Test func independentWriterSurvivesSurfaceWriterRemovalAndRefusesUnacceptedHistory() {
        var accepted = true
        var field = ""
        let model = MaskEditorSession(field: "", isEditing: false,
            acceptsWrite: { _ in accepted }, write: { field = $0 })
        model.setReferenceSizeIfNeeded(imageSize: .init(width: 1000, height: 1000),
            area: .init(width: 1000, height: 1000), displayScale: 1)
        model.onWrite = { _ in Issue.record("A surface must not replace the host writer") }
        model.addBox(from: .init(x: 10, y: 10), to: .init(x: 50, y: 40))
        let edited = model.field, document = model.document
        #expect(!edited.isEmpty && field == edited && model.canUndo)
        model.onWrite = nil
        accepted = false
        model.addBox(from: .init(x: 60, y: 10), to: .init(x: 100, y: 40))
        model.undo()
        #expect(model.document == document && model.field == edited && field == edited)
        #expect(model.canUndo && !model.canRedo)
        accepted = true
        model.undo()
        #expect(model.field.isEmpty && field.isEmpty && !model.canUndo && model.canRedo)
        accepted = false
        model.redo()
        #expect(model.field.isEmpty && field.isEmpty && model.canRedo)
        accepted = true
        model.redo()
        #expect(model.document == document && model.field == edited && field == edited)
    }

    @Test func polygonNeedsThreePointsAndUndoDiscardsOnlyPreview() {
        let model = session(rect1)
        model.tool = .polygon
        model.addPolygonPoint(.init(x: 200, y: 200), closingDistance: 5)
        model.addPolygonPoint(.init(x: 250, y: 210), closingDistance: 5)
        model.closePolygon()
        #expect(model.document.shapes.count == 1)
        for point in [CGPoint(x: 200, y: 200), .init(x: 250, y: 210), .init(x: 220, y: 260)] {
            model.addPolygonPoint(point, closingDistance: 5)
        }
        model.closePolygon()
        #expect(model.document.shapes.count == 2)
        model.addPolygonPoint(.init(x: 100, y: 100), closingDistance: 5)
        let saved = model.field
        model.undo()
        #expect(model.polygon.isEmpty && model.field == saved)
        model.undo()
        #expect(model.document.shapes.count == 1)
    }

    @Test func labelsAreC0AndInvalidBracesNeverWrite() throws {
        let model = session()
        model.addLabel(at: .init(x: 300, y: 300))
        let shape = try #require(model.document.shapes.first)
        #expect(shape.data.number == 0)
        #expect(shape.data.kind == .text(text: "text", scale: 1, fs: 0.04))
        #expect(model.tool == .select)
        let field = model.field
        model.editLabel(shape.id, text: "a}}b")
        #expect(model.field == field)
        #expect(model.errorKey == "io.label.braces")
        model.editLabel(shape.id, text: "")
        #expect(model.field.isEmpty && model.document.shapes.count == 1)
    }

    @Test func ownBindingEchoKeepsHistoryButExternalFieldRebuildsWithoutWriting() {
        let model = session()
        model.addBox(from: .init(x: 10, y: 10), to: .init(x: 100, y: 100))
        model.receive(model.field, isEditing: false)
        #expect(model.canUndo)
        var writes = 0
        model.onWrite = { _ in writes += 1 }
        model.receive(protected, isEditing: true)
        #expect(model.field == protected && writes == 0)
        #expect(!model.isDirty && !model.canUndo)
        #expect(model.document.shapes.first?.data.number == 4)
    }

    @Test func modeChangesHaveHistoryAndViewChangesNeverWrite() {
        let model = session(rect1)
        var writes = 0
        model.onWrite = { _ in writes += 1 }
        model.selectAll(); model.tool = .ellipse
        #expect(writes == 0)
        model.setMode(false)
        #expect(writes == 1 && !model.field.contains("oi=1"))
        model.undo()
        #expect(model.field == rect1 && writes == 2)
    }

    @Test func replacingImageClearsGeometryAndHistoryButKeepsDocumentToolAndSourceBytes() throws {
        let model = session(rect1 + " \t<br/>" + protected, editing: true)
        model.selectAll(); model.commit(model.moved(dx: 20, dy: 0, finishing: true))
        model.tool = .ellipse
        let before = model.document, field = model.field
        #expect(model.canUndo)
        var writes = 0
        model.onWrite = { _ in writes += 1 }
        model.resetForImageChange()
        model.setReferenceSizeIfNeeded(imageSize: .init(width: 200, height: 800),
            area: .init(width: 600, height: 400), displayScale: 2)
        #expect(model.referenceSize == CGSize(width: 100, height: 400))
        #expect(model.document == before && model.field == field && model.tool == .ellipse)
        #expect(!model.canUndo && !model.canRedo && model.selection.isEmpty)
        model.undo(); model.redo()
        #expect(model.field == field && writes == 0)
    }

    @Test func retainedCardNoticeRequiresLastNumberedMaskRemovalAndShowsOncePerEditSession() throws {
        let high = rect1.replacingOccurrences(of: "c1::", with: "c600::")
        let label = "{{c9::image-occlusion:text:left=.1:top=.1:text=hi:scale=1}}"
        let zero = rect1.replacingOccurrences(of: "c1::", with: "c0::")
        let model = session(high + "<br>" + high + "<br>" + label + "<br>" + zero, editing: true)
        model.deleteSelection()
        model.selectAll()
        #expect(model.retainedCardNotice == nil && model.removedNumbers.isEmpty && !model.canUndo)
        model.selection = [model.document.shapes[0].id]
        model.deleteSelection() // One member remains for 600.
        #expect(model.retainedCardNotice == nil && model.removedNumbers.isEmpty)
        model.selection = Set(model.document.shapes.filter { $0.isLabel || $0.data.number == 0 }.map(\.id))
        model.deleteSelection()
        #expect(model.retainedCardNotice == nil && model.removedNumbers.isEmpty)
        model.selectAll(); model.deleteSelection()
        #expect(model.retainedCardNotice == 500 && model.removedNumbers == [600])
        model.retainedCardNotice = nil
        model.undo(); model.redo(); model.undo()
        #expect(model.retainedCardNotice == nil)
        model.selectAll(); model.deleteSelection()
        #expect(model.retainedCardNotice == nil)
        model.receive(rect1, isEditing: true)
        model.selectAll(); model.deleteSelection()
        #expect(model.retainedCardNotice == nil && model.removedNumbers == [1, 600])
        let add = session(rect1)
        add.selectAll(); add.deleteSelection(); add.undo(); add.redo()
        #expect(add.retainedCardNotice == nil)
    }

    @Test func groupingRemovedNumberShowsNoticeButUndoAndRedoDoNot() {
        let model = session(rect1 + "<br>" + protected, editing: true)
        model.selectAll(); model.group()
        #expect(model.removedNumbers == [4] && model.retainedCardNotice == 4)
        model.retainedCardNotice = nil
        model.undo(); model.redo()
        #expect(model.retainedCardNotice == nil)
    }

    @Test func strictCoreCodecReadsNumberedMasksLabelsAndEmptyDocuments() throws {
        #expect(try OcclusionCodec(field: rect1).shapes.map(\.number) == [1])
        #expect(try OcclusionCodec(field: "{{c0::image-occlusion:text:left=.1:top=.1:text=hi}}").shapes.map(\.number) == [0])
        #expect(try OcclusionCodec(field: "").shapes.isEmpty)
        #expect(throws: (any Error).self) { try OcclusionCodec(field: "unsupported") }
    }
}
