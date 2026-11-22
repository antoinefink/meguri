@testable import AppModel
import CoreBridge
import Foundation
import Testing

/// The Fields tab's draft (`notetypes/workspace` W1, W3, W4, W13;
/// `notetypes/draft-save` D1).
@Suite struct NoteTypeDraftTests {
    static func field(_ ord: UInt32, _ name: String, protected: Bool = false) -> NotetypeField {
        NotetypeField(ord: ord, id: Int64(ord) + 100, name: name, sticky: false, rtl: false, fontName: "Liberation Sans", fontSize: 18,
                      description: "", plainText: true, collapsed: false, excludeFromSearch: true, preventDeletion: protected)
    }

    static func record(fields: [String] = ["Front", "Back", "Extra"], sort: UInt32 = 0, protectedFields: Set<String> = [],
                       templates: [(String, String, String)] = [("Card 1", "{{Front}}{{#Extra}}{{Extra}}{{/Extra}}", "{{FrontSide}}<hr id=answer>{{Back}}")]) -> NotetypeRecord {
        NotetypeRecord(
            id: 7, name: "Sample", kind: 0, sortField: sort, css: ".card {}", base: [1, 2, 3],
            fields: fields.enumerated().map { field(UInt32($0.offset), $0.element, protected: protectedFields.contains($0.element)) },
            templates: templates.enumerated().map {
                NotetypeTemplate(ord: UInt32($0.offset), id: nil, name: $0.element.0, front: $0.element.1, back: $0.element.2,
                                 qFormatBrowser: "", aFormatBrowser: "", targetDeckId: 0, browserFontName: "", browserFontSize: 0)
            })
    }

    @Test func anUnchangedDraftIsNotEditedAndCarriesEveryStoredOption() {
        let draft = NoteTypeDraft(Self.record())
        #expect(!draft.isEdited && !draft.changesStructure && draft.fieldsOnly)
        let structure = draft.structureDraft()
        #expect(structure.base == [1, 2, 3] && structure.notetypeId == 7 && structure.sortField == 0 && structure.skipChecks)
        // W4: options the tab does not show keep their stored values (S1.1).
        #expect(structure.fields.map(\.fontName) == ["Liberation Sans", "Liberation Sans", "Liberation Sans"])
        #expect(structure.fields.allSatisfy { $0.plainText && $0.excludeFromSearch && $0.fontSize == 18 })
        #expect(structure.fields.map(\.origin) == [0, 1, 2])
    }

    @Test func deletingCrossesOutAndRestoreTakesItBack() {
        var draft = NoteTypeDraft(Self.record())
        let changed1 = draft.delete(2)
        #expect(changed1)
        #expect(draft.rows.map(\.deleted) == [false, false, true], "the row stays in the list, crossed out")
        #expect(draft.isEdited && draft.changesStructure)
        #expect(draft.structureDraft().fields.map(\.name) == ["Front", "Back"])
        let changed2 = draft.delete(2)
        #expect(!changed2, "already crossed out")
        let changed3 = draft.restore(2)
        #expect(changed3)
        #expect(!draft.isEdited)
        #expect(draft.undoStep == .restoreField)
    }

    @Test func restoreWaitsWhileAnotherFieldHasTheName() {
        var draft = NoteTypeDraft(Self.record())
        _ = draft.delete(2)
        let added = draft.addField()
        _ = draft.rename(added, to: "extra")
        #expect(!draft.canRestore(2))
        let refused = draft.restore(2)
        #expect(!refused && draft.row(2)?.deleted == true)
        _ = draft.rename(added, to: "Other")
        let restored = draft.restore(2)
        #expect(restored)
    }

    /// W3: Delete is disabled for the last remaining field and for a protected one, with the reason.
    @Test func deleteIsBlockedForTheLastAndForProtectedFields() {
        var draft = NoteTypeDraft(Self.record(fields: ["Text", "Back Extra"], protectedFields: ["Text"]))
        #expect(draft.deleteBlock(0) == .protected)
        let changed4 = draft.delete(0)
        #expect(!changed4)
        #expect(draft.deleteBlock(1) == nil)
        var two = NoteTypeDraft(Self.record(fields: ["Front", "Back"]))
        let changed5 = two.delete(0)
        #expect(changed5)
        #expect(two.deleteBlock(1) == .lastField)
        let changed6 = two.delete(1)
        #expect(!changed6)
        #expect(two.deleteBlock(0) == nil, "a crossed-out row has nothing to block")
    }

    @Test func addingNamesTheFirstFreeFieldAndUsesN52sDefaults() {
        var draft = NoteTypeDraft(Self.record(fields: ["Field 1", "field 3"]))
        let added = draft.addField()
        #expect(draft.row(added)?.name == "Field 2")
        let next = draft.addField()
        #expect(draft.row(next)?.name == "Field 4", "free under case folding")
        let field = draft.structureDraft().fields[2]
        #expect(field.origin == nil && field.fontName == "Arial" && field.fontSize == 20)
        #expect(!field.sticky && !field.rtl && !field.plainText && !field.collapsed && !field.excludeFromSearch && field.description.isEmpty)
        #expect(added < 0 && next < 0 && added != next)
    }

    @Test func movingKeepsOriginsAndTheSortFieldFollowsItsRow() {
        var draft = NoteTypeDraft(Self.record(sort: 1))
        let changed7 = draft.move(fromOffsets: [2], toOffset: 0)
        #expect(changed7)
        let structure = draft.structureDraft()
        #expect(structure.fields.map(\.origin) == [2, 0, 1])
        #expect(structure.sortField == 2, "Back stays the sort field at its new position")
        let changed8 = draft.move(fromOffsets: [0], toOffset: 0)
        #expect(!changed8, "a move to the same place changes nothing")
        #expect(draft.undoStep == .moveField)
    }

    @Test func theSortRadioAndACrossedOutSortField() {
        var draft = NoteTypeDraft(Self.record())
        let set = draft.setSortField(2)
        #expect(set && draft.structureDraft().sortField == 2 && draft.changesStructure)
        let again = draft.setSortField(2)
        #expect(!again)
        // Crossing out the sort field moves the radio, in the same step, to the
        // field the save would sort by (S3 step 4: the base's index 0 here).
        let deleted = draft.delete(2)
        #expect(deleted && draft.sortRow == 0 && draft.structureDraft().sortField == 0)
        let crossed = draft.setSortField(2)
        #expect(!crossed, "a crossed-out row takes no radio")
        let undone = draft.undo()
        #expect(undone == .deleteField && draft.sortRow == 2)
        // A base sort field past the end of the kept fields falls to the last one.
        var last = NoteTypeDraft(Self.record(sort: 2))
        _ = last.delete(2)
        #expect(last.sortRow == 1 && last.structureDraft().sortField == 1)
    }

    @Test func renamesAndOptionsAreNotStructural() {
        var draft = NoteTypeDraft(Self.record())
        let changed13 = draft.rename(0, to: "Prompt")
        #expect(changed13)
        let changed14 = draft.rename(0, to: "Prompt")
        #expect(!changed14)
        let changed15 = draft.setOption(1) { $0.collapsed = true }
        #expect(changed15)
        let changed16 = draft.setOption(1) { $0.description = "Optional" }
        #expect(changed16)
        let changed17 = draft.setOption(1) { $0.collapsed = true }
        #expect(!changed17, "no change")
        #expect(draft.isEdited && !draft.changesStructure && draft.fieldsOnly)
        #expect(draft.structureDraft().fields.map(\.name) == ["Prompt", "Back", "Extra"])
    }

    /// W13: the editor's own undo steps back through draft changes, with their names.
    @Test func theDraftsOwnUndoAndRedo() {
        var draft = NoteTypeDraft(Self.record())
        draft.delete(1)
        draft.rename(0, to: "Prompt")
        draft.move(fromOffsets: [2], toOffset: 0)
        draft.setSortField(2)
        draft.setOption(0) { $0.rtl = true }
        #expect(draft.undoStep == .changeOption)
        let step1 = draft.undo()
        #expect(step1 == .changeOption)
        let step2 = draft.undo()
        #expect(step2 == .changeSortField)
        let step3 = draft.undo()
        #expect(step3 == .moveField)
        #expect(draft.undoStep == .renameField)
        #expect(draft.redoStep == .moveField)
        let step4 = draft.redo()
        #expect(step4 == .moveField)
        #expect(draft.rows.map(\.name) == ["Extra", "Prompt", "Back"])
        draft.addField()
        #expect(draft.redoStep == nil, "a new change ends the redo steps")
        while draft.undo() != nil {}
        #expect(!draft.isEdited && draft.rows.map(\.name) == ["Front", "Back", "Extra"])
        #expect(draft.undoStep == nil)
    }

    /// W1: Used On from T11.1's references, with the draft's renames applied as the save would.
    @Test func usedOnFollowsTheReferencesThroughRenames() {
        let record = Self.record(templates: [("Card 1", "{{Front}}", "{{Back}}"), ("Card 2", "{{Back}}", "{{Front}} {{Extra}}")])
        var draft = NoteTypeDraft(record)
        #expect(draft.referenceTexts == ["{{Front}}", "{{Back}}", "{{Back}}", "{{Front}} {{Extra}}"])
        let references = [["Front"], ["Back"], ["Back"], ["Front", "Extra"]]
        #expect(draft.usedOn(references: references) == [0: [0, 1], 1: [0, 1], 2: [1]])
        // Renaming Front to Prompt keeps its uses; a new field named Front is not used.
        draft.rename(0, to: "Prompt")
        let added = draft.addField()
        draft.rename(added, to: "Front")
        let used = draft.usedOn(references: references)
        #expect(used[0] == [0, 1] && used[added] == [])
        // A key that names no base field refers to the added field of that name.
        #expect(draft.usedOn(references: [["Field 9"], [], [], []])[added] == [])
        let named = draft.addField()
        #expect(draft.usedOn(references: [[draft.row(named)!.name], [], [], []])[named] == [0])
    }
}
