@testable import AppModel
import CoreBridge
import Foundation
import Testing

/// The Cards tab's draft and editor helpers (`notetypes/workspace` W5, W6,
/// W7, W13; `notetypes/editor-actions` A6 to A8, A14, A15).
@Suite struct NoteTypeCardsTests {
    static func record(kind: Int32 = 0, templates: [(String, String, String)] = [
        ("Forward", "{{Front}}", "{{FrontSide}}<hr id=answer>{{Back}}"), ("Reverse", "{{Back}}", "{{FrontSide}}<hr id=answer>{{Front}}"),
    ]) -> NotetypeRecord {
        let base = NoteTypeDraftTests.record(templates: templates)
        let stored = base.templates.enumerated().map { at, t in
            NotetypeTemplate(ord: t.ord, id: nil, name: t.name, front: t.front, back: t.back, qFormatBrowser: at == 0 ? "{{Front}} (browser)" : "",
                             aFormatBrowser: "", targetDeckId: at == 0 ? 42 : 0, browserFontName: at == 0 ? "Arial" : "", browserFontSize: at == 0 ? 12 : 0)
        }
        return NotetypeRecord(id: base.id, name: base.name, kind: kind, sortField: base.sortField, css: base.css, base: base.base,
                              fields: base.fields, templates: stored)
    }

    @Test func addCopiesTheFrontAndBackOnlyAsCardN() {
        var draft = NoteTypeDraft(Self.record())
        #expect(draft.templates == Self.record().templates.map(StructureDraftTemplate.init) && draft.fieldsOnly)
        let added = draft.addCardType(copying: 0)
        #expect(added == -1)
        let row = draft.cardRow(-1)
        #expect(row?.name == "Card 3" && row?.origin == nil)
        let texts = draft.texts(-1)
        // A6: the texts exactly; no browser texts, deck override or browser font.
        #expect(texts?.front == "{{Front}}" && texts?.back == "{{FrontSide}}<hr id=answer>{{Back}}")
        #expect(texts?.browserFront == "" && texts?.targetDeckId == 0 && texts?.browserFontName == "" && texts?.browserFontSize == 0)
        #expect(draft.isEdited && draft.changesStructure && !draft.fieldsOnly)
        #expect(draft.structureDraft().templates.map(\.origin) == [0, 1, nil] && !draft.structureDraft().skipChecks)
        // E10: N is one more than the card types, raised past a taken name.
        draft.renameCardType(-1, to: "Card 4")
        #expect(draft.newCardTypeName == "Card 5")
        #expect(draft.undoStep == .renameCardType)
    }

    @Test func deleteCrossesOutAndIsBlockedForTheLastAndForCloze() {
        var draft = NoteTypeDraft(Self.record())
        let deleted = draft.deleteCardType(1)
        #expect(deleted)
        #expect(draft.cardRows.map(\.deleted) == [false, true] && draft.templates.map(\.name) == ["Forward"])
        let last = draft.deleteCardType(0)
        #expect(draft.cardDeleteBlock(0) == .lastCardType && !last)
        let restored = draft.restoreCardType(1)
        #expect(restored && draft.templates.count == 2)
        let cloze = NoteTypeDraft(Self.record(kind: 1, templates: [("Cloze", "{{cloze:Front}}", "{{cloze:Front}}")]))
        #expect(cloze.addCardTypeBlock == .cloze && cloze.cardDeleteBlock(0) == .cloze)
        var copy = cloze
        let copied = copy.addCardType(copying: 0)
        #expect(copied == nil)
    }

    @Test func renameRemovesQuotesAndIgnoresBlankNames() {
        var draft = NoteTypeDraft(Self.record())
        let blank = draft.renameCardType(0, to: " \" ")
        let renamed = draft.renameCardType(0, to: " \"Re\"call ")
        #expect(!blank && renamed)
        #expect(draft.cardRow(0)?.name == " Recall ", "A8: not trimmed")
        let same = draft.renameCardType(0, to: " Recall ")
        #expect(!same)
    }

    @Test func movesAndTheirUndoKeepWhatWasTyped() {
        var draft = NoteTypeDraft(Self.record())
        #expect(!draft.canMoveCardType(0, by: -1) && draft.canMoveCardType(0, by: 1))
        let moved = draft.moveCardType(0, by: 1)
        #expect(moved)
        #expect(draft.cardRows.map(\.id) == [1, 0] && draft.undoStep == .moveCardType)
        // Typing is the text view's undo: a draft undo does not take it back.
        let typed = draft.setText(0, .front, "{{Front}} typed")
        let styled = draft.setText(0, .styling, ".card { color: red }")
        let undone = draft.undo()
        #expect(typed && styled && undone == .moveCardType)
        #expect(draft.cardRows.map(\.id) == [0, 1] && draft.texts(0)?.front == "{{Front}} typed" && draft.css == ".card { color: red }")
        let again = draft.setText(0, .front, "{{Front}} typed")
        #expect(!again, "the same text changes nothing")
        let added = draft.addCardType(copying: 1)!
        draft.setText(added, .back, "new back")
        let undoAdd = draft.undo()
        #expect(undoAdd == .addCardType && draft.cardRow(added) == nil)
        let redoAdd = draft.redo()
        #expect(redoAdd == .addCardType && draft.texts(added)?.back == "new back")
    }

    // MARK: Completion (W6)

    @Test func completionListsFieldsThenSpecialNamesAfterTwoBraces() throws {
        let fields = ["Front", "Back", "Extra"]
        let text = "<b>{{"
        let context = try #require(NoteTypeTemplateEditing.completionContext(text, caret: 5))
        #expect(!context.section && !context.afterFilter && context.typed.isEmpty)
        #expect(NoteTypeTemplateEditing.completions(context, fields: fields)
            == ["Front", "Back", "Extra", "FrontSide", "Tags", "Deck", "Subdeck", "Card", "CardFlag", "CardID", "Type"])
        let typed = try #require(NoteTypeTemplateEditing.completionContext("{{fr", caret: 4))
        #expect(NoteTypeTemplateEditing.completions(typed, fields: fields) == ["Front", "FrontSide"])
        let edit = NoteTypeTemplateEditing.accept("Front", context: typed, in: "{{fr")
        #expect(edit == NoteTypeTextEdit(range: 2..<4, text: "Front}}", caret: 9))
        // Inside a name already written, the rest of it is replaced too.
        let inside = try #require(NoteTypeTemplateEditing.completionContext("{{Front}}", caret: 3))
        #expect(NoteTypeTemplateEditing.accept("Front", context: inside, in: "{{Front}}") == NoteTypeTextEdit(range: 2..<7, text: "Front", caret: 9))
        // Nothing to complete outside a tag or on another line.
        #expect(NoteTypeTemplateEditing.completionContext("plain", caret: 5) == nil)
        #expect(NoteTypeTemplateEditing.completionContext("{{\nx", caret: 4) == nil)
    }

    @Test func afterAFilterOnlyFieldsAndASectionAddsItsClose() throws {
        let fields = ["Front", "Back"]
        let hint = try #require(NoteTypeTemplateEditing.completionContext("{{hint:", caret: 7))
        #expect(hint.afterFilter && NoteTypeTemplateEditing.completions(hint, fields: fields) == fields)
        #expect(NoteTypeTemplateEditing.completionContext("{{nope:", caret: 7) == nil, "not a filter prefix")
        let section = try #require(NoteTypeTemplateEditing.completionContext("{{#B", caret: 4))
        #expect(section.section)
        let edit = NoteTypeTemplateEditing.accept("Back", context: section, in: "{{#B")
        #expect(edit == NoteTypeTextEdit(range: 3..<4, text: "Back}}{{/Back}}", caret: 9))
        // An open tag already closed keeps its `}}` once.
        let closed = try #require(NoteTypeTemplateEditing.completionContext("{{#B}}", caret: 4))
        #expect(NoteTypeTemplateEditing.accept("Back", context: closed, in: "{{#B}}") == NoteTypeTextEdit(range: 3..<6, text: "Back}}{{/Back}}", caret: 9))
    }

    // MARK: Insert (W6, A14)

    @Test func insertItemsWriteTheirTagsAndConditionalsWrapTheSelection() {
        let text = "ab"
        #expect(NoteTypeTemplateEditing.insert(.field("Front"), in: text, selection: 1..<1) == NoteTypeTextEdit(range: 1..<1, text: "{{Front}}", caret: 10))
        #expect(NoteTypeTemplateEditing.insert(.filter("hint", "Back"), in: text, selection: 0..<2).text == "{{hint:Back}}")
        #expect(NoteTypeTemplateEditing.insert(.showIfContent("Extra"), in: text, selection: 0..<2)
            == NoteTypeTextEdit(range: 0..<2, text: "{{#Extra}}ab{{/Extra}}", caret: 12))
        #expect(NoteTypeTemplateEditing.insert(.showIfEmpty("Extra"), in: text, selection: 1..<1).text == "{{^Extra}}{{/Extra}}")
        #expect(NoteTypeTemplateEditing.insert(.answerDivider, in: text, selection: 2..<2).text == "<hr id=answer>")
        // A14: alternate mode writes its delimiters, and never inside the lead.
        let alternate = " {{=<% %>=}}x"
        #expect(NoteTypeTemplateEditing.alternateLead(alternate) == 12)
        #expect(NoteTypeTemplateEditing.insert(.field("Front"), in: alternate, selection: 3..<5) == NoteTypeTextEdit(range: 12..<12, text: "<%Front%>", caret: 21))
        #expect(!NoteTypeTemplateEditing.canReference("a%>b", in: alternate) && NoteTypeTemplateEditing.canReference("a%>b", in: "x"))
        // Styling's Dark Appearance Rule at the end.
        let dark = NoteTypeTemplateEditing.insert(.darkRule, in: ".card {}", selection: 0..<0)
        #expect(dark == NoteTypeTextEdit(range: 8..<8, text: "\n\n.card.nightMode {\n}", caret: 8 + 20))
    }

    @Test func commandSlashWrapsAndUnwrapsEachLine() {
        let text = "  {{Front}}\n\n{{Back}}\nkept"
        let wrapped = NoteTypeTemplateEditing.toggleComment(text, selection: 3..<15, styling: false)
        #expect(wrapped.range == 0..<22)
        #expect(wrapped.text == "  <!-- {{Front}} -->\n\n<!-- {{Back}} -->\n")
        let applied = (text as NSString).replacingCharacters(in: NSRange(location: 0, length: 22), with: wrapped.text)
        let unwrapped = NoteTypeTemplateEditing.toggleComment(applied, selection: 0..<1, styling: false)
        #expect(unwrapped.text == "  {{Front}}\n")
        #expect(NoteTypeTemplateEditing.toggleComment("color: red;", selection: 0..<0, styling: true).text == "/* color: red; */")
        // A CR LF line keeps its CR after the close, and uncomments again.
        let crlf = NoteTypeTemplateEditing.toggleComment("{{Front}}\r\nkept", selection: 0..<0, styling: false)
        #expect(crlf.text == "<!-- {{Front}} -->\r\n")
        #expect(crlf.caret == 18, "the caret ends the line, before its CR LF")
        #expect(NoteTypeTemplateEditing.toggleComment("<!-- {{Front}} -->\r\n", selection: 0..<0, styling: false).text == "{{Front}}\r\n")
    }

    @Test func flipFollowsA15() {
        let flipped = NoteTypeTemplateEditing.flip(front: "{{Front}}", back: "{{FrontSide}}\n\n<hr id=answer>\n\n{{Back}}")
        #expect(flipped?.front == "{{Back}}" && flipped?.back == "{{FrontSide}}\n\n<hr id=answer>\n\n{{Front}}")
        #expect(NoteTypeTemplateEditing.flip(front: "Q", back: "A<hr id=answer>B<hr id=answer>")?.front == "B<hr id=answer>")
        #expect(NoteTypeTemplateEditing.flip(front: "Q", back: "<hr id=answer>{{Back}}") == nil)
        #expect(NoteTypeTemplateEditing.flip(front: "Q", back: "{{FrontSide}}<hr id=\"answer\">{{Back}}") == nil)
    }

    @Test func linesStartWhereTheCoreCountsThem() {
        let text = "a\nb\r\nc\rd"
        #expect([1, 2, 3, 4, 9].map { NoteTypeTemplateEditing.offset(ofLine: $0, in: text) } == [0, 2, 5, 7, 8])
    }

    // MARK: Live issues (W7)

    @Test func issueLinesKeepTheirCardTypesAndOfferTheirFixes() {
        var draft = NoteTypeDraft(Self.record())
        let red = NoteTypeIssueRead(cards: [0, 1], fields: ["Front", "Back", "Extra"], issues: [
            NotetypeTemplateIssue(template: 1, part: .front, line: nil, kind: .duplicateFront(earlier: 0)),
            NotetypeTemplateIssue(template: 0, part: .back, line: 3, kind: .unknownField(name: "Bakc", suggestion: 1)),
            NotetypeTemplateIssue(template: 1, part: .back, line: 1, kind: .unclosedSection(tag: "{{#Extra}}", close: "{{/Extra}}")),
        ])
        let yellow = NoteTypeBlankRead(cards: [0, 1], fields: ["Front", "Back", "Extra"], fronts: [
            NotetypeBlankFront(noteIds: [], field: nil), NotetypeBlankFront(noteIds: [5, 6], field: 1),
        ])
        let lines = NoteTypeLiveIssue.lines(red: red, yellow: yellow, draft: draft)
        #expect(lines.map(\.kind) == [.duplicateFront(earlier: "Forward"), .unknownField(name: "Bakc"), .unclosedSection(tag: "{{#Extra}}"),
                                      .blankFront(notes: 2, field: "Back")])
        #expect(lines.map(\.fix) == [nil, .useField(from: "Bakc", to: "Back"), .addClose("{{/Extra}}"), .showNotes([5, 6])])
        #expect(lines.map(\.severity) == [.red, .red, .red, .yellow] && lines[1].line == 3 && lines[1].part == .back)
        // A card type crossed out since the read shows none of its lines.
        _ = draft.deleteCardType(1)
        #expect(NoteTypeLiveIssue.lines(red: red, yellow: yellow, draft: draft).map(\.card) == [0])
    }
}
