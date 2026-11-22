#if os(macOS)
import AppKit
import AppModel
import CoreBridge
import Testing
@testable import NoteTypes

/// `notetypes/workspace` A15 and W13: Flip, from the Back's context menu, is
/// one step of the Back's own undo that puts back both texts.
@MainActor @Suite struct TemplateEditorFlipTests {
    /// The draft's texts of one card type, as the editor's closures reach them.
    private final class Texts {
        var front = "{{Front}}"
        var back = "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}"
    }

    private struct Editor {
        let coordinator: TemplateCodeEditor.Coordinator
        let textView: CodeTextView
        var manager: UndoManager? { coordinator.undoManager(for: textView) }
    }

    private func editor(_ texts: Texts) -> Editor {
        let key = TemplateEditorKey(notetype: 7, card: 0, part: .back)
        let parent = TemplateCodeEditor(
            key: key, text: texts.back, editable: true, fields: ["Front", "Back"], request: nil, insertMenu: [],
            accessibilityLabel: "Back",
            onChange: { texts.back = $0 },
            onConsume: { _ in },
            onFlip: {
                NoteTypeTemplateEditing.flip(front: texts.front, back: texts.back)
                    .map { NoteTypeFlip(front: $0.front, back: $0.back, previousFront: texts.front) }
            },
            onReplaceFront: { expected, text in
                guard texts.front == expected else { return false }
                texts.front = text
                return true
            },
            syntax: { _, _ in [] })
        let coordinator = TemplateCodeEditor.Coordinator()
        let textView = CodeTextView(usingTextLayoutManager: false)
        textView.allowsUndo = true
        textView.isRichText = false
        textView.delegate = coordinator
        textView.coordinator = coordinator
        coordinator.textView = textView
        coordinator.parent = parent
        coordinator.show(key: key, text: texts.back)
        return Editor(coordinator: coordinator, textView: textView)
    }

    @Test func flipIsOneUndoStepThatPutsBackBothTexts() throws {
        let texts = Texts()
        let editor = editor(texts)
        let manager = try #require(editor.manager)
        editor.coordinator.flip()
        #expect(texts.front == "{{Back}}")
        #expect(texts.back == "{{FrontSide}}\n\n<hr id=answer>\n\n{{Front}}")
        #expect(editor.textView.string == texts.back)
        #expect(manager.undoActionName == "Flip", "Edit › Undo reads Undo Flip")

        manager.undo()
        #expect(texts.front == "{{Front}}", "one undo puts back the front")
        #expect(texts.back == "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}", "and the back")
        #expect(editor.textView.string == texts.back)
        #expect(!manager.canUndo, "Flip was a single step")

        manager.redo()
        #expect(texts.front == "{{Back}}")
        #expect(texts.back == "{{FrontSide}}\n\n<hr id=answer>\n\n{{Front}}")
    }

    @Test func undoKeepsAFrontEditedSinceTheFlip() throws {
        let texts = Texts()
        let editor = editor(texts)
        let manager = try #require(editor.manager)
        editor.coordinator.flip()
        texts.front = "{{Back}} typed later"
        manager.undo()
        #expect(texts.front == "{{Back}} typed later", "what was typed since is never taken back")
        #expect(texts.back == "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}")
    }

    @Test func aRefusedFlipChangesNothingAndRecordsNoStep() throws {
        let texts = Texts()
        texts.back = "<hr id=answer>{{Back}}"
        let editor = editor(texts)
        let manager = try #require(editor.manager)
        editor.coordinator.flip()
        #expect(texts.front == "{{Front}}" && texts.back == "<hr id=answer>{{Back}}")
        #expect(!manager.canUndo)
    }
}

/// W6: a colour swatch stays on its own line at every text size.
@Suite struct SwatchMetricsTests {
    @Test(arguments: [9.0, 11.0, 13.0, 17.0, 28.0, 53.0])
    func aSwatchStaysInsideItsLinesGlyphs(size: Double) throws {
        let font = try #require(NSFont.userFixedPitchFont(ofSize: size))
        let place = SwatchMetrics.place(capHeight: font.capHeight, ascender: font.ascender, descender: font.descender)
        let top = place.centre + place.side / 2, bottom = place.centre - place.side / 2
        #expect(top < font.ascender, "clear of the line above at \(size) pt")
        #expect(bottom > font.descender, "clear of the line below at \(size) pt")
        #expect(place.side >= font.capHeight, "still as tall as the capitals at \(size) pt")
    }
}
#endif
