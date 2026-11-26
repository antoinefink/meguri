import CoreBridge
import Foundation
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct OrdinalAllocatorTests {
    func model() -> MaskEditorSession {
        let field = [1, 2, 4].map { "{{c\($0)::image-occlusion:rect:left=.1:top=.1:width=.1:height=.1:oi=1}}" }.joined(separator: "<br>")
        return OcclusionDocumentTests().session(field, editing: true)
    }
    @Test func fillsLowestGapsAndKeepsExistingOrdinals() {
        let model = model()
        model.addBox(from: .init(x: 10, y: 10), to: .init(x: 30, y: 30))
        model.addBox(from: .init(x: 50, y: 50), to: .init(x: 80, y: 80))
        #expect(model.document.shapes.map { $0.data.number } == [1, 2, 4, 3, 5])
    }
    @Test func groupingAndUngroupingUseCoreRulesAndStableSelection() {
        let model = model()
        model.selection = Set([model.document.shapes[1].id, model.document.shapes[2].id])
        model.group()
        #expect(model.document.shapes.map { $0.data.number } == [1, 2, 2])
        #expect(!model.canGroup && model.canUngroup)
        #expect(model.document.items.count == 2)
        #expect(!model.canRotate)
        model.ungroup()
        #expect(model.document.shapes.map { $0.data.number } == [1, 2, 3])
        #expect(model.selection.count == 2)
        model.undo()
        #expect(model.document.shapes.map { $0.data.number } == [1, 2, 2])
    }
    @Test func duplicatesGroupWithOneNewOrdinalAndIncrementalOffset() {
        let model = model()
        model.selection = Set([model.document.shapes[1].id, model.document.shapes[2].id]); model.group()
        model.duplicate()
        #expect(model.document.shapes.map { $0.data.number } == [1, 2, 2, 3, 3])
        #expect(model.document.shapes[3].data.sourceIndex == nil)
        #expect(abs(model.document.shapes[3].data.left - 0.11) < 0.000001)
        model.duplicate()
        #expect(abs(model.document.shapes[5].data.left - 0.12) < 0.000001)
        #expect(model.document.shapes[5].data.number == 4)
    }
    @Test func deleteKeepsGapsAndUndoRestoresNumber() {
        let model = model()
        model.selection = [model.document.shapes[1].id]; model.deleteSelection()
        #expect(model.document.shapes.map { $0.data.number } == [1, 4])
        model.undo()
        #expect(model.document.shapes.map { $0.data.number } == [1, 2, 4])
        model.redo()
        model.addBox(from: .init(x: 10, y: 10), to: .init(x: 30, y: 30))
        #expect(model.document.shapes.last?.data.number == 2)
        #expect(!model.canRedo)
    }
}
