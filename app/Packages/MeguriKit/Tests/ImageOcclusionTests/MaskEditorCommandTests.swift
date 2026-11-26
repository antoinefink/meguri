import SwiftUI
import Testing
@testable import ImageOcclusion

@MainActor @Suite struct MaskEditorCommandTests {
    @Test func historyDispatchUsesSameGuardsForKeysMenusAndParentFallback() {
        let session = OcclusionDocumentTests().session()
        var active = true
        var parentUndos = 0
        let context = MaskEditorCommandContext(enabled: { command in
            guard active else { return false }
            switch command {
            case .undo: return session.canUndo
            case .redo: return session.canRedo
            default: return true
            }
        }, action: { command in
            if command == .undo { session.undo() }
            if command == .redo { session.redo() }
        })
        func menuUndo() {
            if !context.performIfEnabled(.undo) { parentUndos += 1 }
        }
        menuUndo()
        #expect(parentUndos == 1)
        #expect(!context.keyPressed(character: "z", modifiers: .command))
        session.addBox(from: .init(x: 100, y: 100), to: .init(x: 200, y: 200))
        #expect(context.keyPressed(character: "z", modifiers: .command))
        #expect(session.document.shapes.isEmpty)
        #expect(context.keyPressed(character: "y", modifiers: .command))
        menuUndo()
        #expect(session.document.shapes.isEmpty && parentUndos == 1)
        #expect(context.keyPressed(character: "z", modifiers: [.command, .shift]))
        // Field view, label sheet, text input and other windows relinquish keys.
        active = false
        #expect(!context.performIfEnabled(.undo))
        #expect(!context.keyPressed(character: "r", modifiers: []))
        #expect(session.document.shapes.count == 1)
    }

    @Test func bindingsAreUniqueAndKeepHostAddEscapeAndReviewModifiers() {
        let bindings = MaskEditorCommand.bindings
        for (index, binding) in bindings.enumerated() {
            #expect(!bindings.dropFirst(index + 1).contains {
                $0.character == binding.character && $0.modifiers == binding.modifiers
            })
        }
        let context = MaskEditorCommandContext(enabled: { _ in true }, action: { _ in })
        #expect(!context.keyPressed(character: "\r", modifiers: .command))
        #expect(!context.keyPressed(character: "\u{1b}", modifiers: []))
        #expect(!context.keyPressed(character: "m", modifiers: [.command, .option, .shift]))
        #expect(!context.keyPressed(character: "r", modifiers: .command))
    }

    @Test(arguments: [
        "rect:width=.2:height=.1", "ellipse:rx=.1:ry=.05",
        "polygon:points=.3,.3 .5,.3 .4,.5", "text:text=label:scale=1:fs=.04"
    ]) func repeatedInspectorAndAccessibilityRotationStepsAdvanceEveryShape(kind: String) throws {
        let parts = kind.split(separator: ":", maxSplits: 1)
        let source = "{{c1::image-occlusion:\(parts[0]):left=.3:top=.3:\(parts[1]):oi=1}}"
        let session = OcclusionDocumentTests().session(source, editing: true)
        session.selectAll()
        let original = session.document
        var writes = 0
        session.onWrite = { _ in writes += 1 }
        for index in 1...3 {
            let before = session.document
            session.rotateStep(increment: true)
            #expect(session.document != before)
            #expect(writes == index)
            if !kind.hasPrefix("polygon") { #expect(session.document.shapes[0].data.angle == Double(index * 5)) }
        }
        session.rotateStep(increment: false)
        #expect(writes == 4)
        for _ in 0..<4 { session.undo() }
        #expect(session.document == original && session.field == source && !session.canUndo)
    }
}
