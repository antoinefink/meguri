@testable import AppModel
@testable import CoreBridge
import Foundation
import Testing

/// A scripted core for the Note Types section.
@MainActor private final class FakeNoteTypesBackend: NoteTypesBackend {
    var names = [NotetypeName(id: 7, name: "Sample"), NotetypeName(id: 8, name: "Vocabulary"), NotetypeName(id: 9, name: "basic")]
    var records: [Int64: NotetypeRecord] = [7: NoteTypeDraftTests.record()]
    var contentCounts = NotetypeFieldContentCounts(notes: 20, fields: [20, 12, 0])
    /// Each summary read takes the first of these; the last one stays.
    var summaries: [NotetypeDraftSummary] = []
    var saveErrors: [any Error] = []
    private(set) var summaryReads = 0
    private(set) var saved: [NotetypeStructureDraft] = []

    func list() async throws -> [NotetypeName] { names }
    func noteCounts() async throws -> [Int64: UInt64] { [7: 20, 8: 5] }
    func read(_ id: Int64) async throws -> NotetypeRecord {
        guard let record = records[id] else { throw BridgeError.notFound(table: .notetypes, key: "\(id)") }
        return record
    }
    func typeCounts(_ id: Int64) async throws -> NotetypeRemovalCounts { NotetypeRemovalCounts(notes: 20, cards: 31) }
    func fieldContentCounts(_ id: Int64) async throws -> NotetypeFieldContentCounts { contentCounts }
    func summary(_ draft: NotetypeStructureDraft) async throws -> NotetypeDraftSummary {
        summaryReads += 1
        guard let first = summaries.first else { return NotetypeDraftSummary(reasons: [], changes: [], losesContent: false) }
        if summaries.count > 1 { summaries.removeFirst() }
        return first
    }
    func save(_ draft: NotetypeStructureDraft, progress: @escaping @MainActor (UInt64, UInt64?) -> Void) async throws {
        progress(1, 20)
        if !saveErrors.isEmpty { throw saveErrors.removeFirst() }
        saved.append(draft)
    }
    func checkFieldName(_ typed: String, fields: [String], renaming: UInt32?) async throws -> NotetypeFieldNameCheck {
        if typed.hasPrefix("#") { return .refused(.badFirstCharacter) }
        if fields.enumerated().contains(where: { $0.offset != renaming.map(Int.init) && $0.element.lowercased() == typed.lowercased() }) {
            return .refused(.duplicate)
        }
        let name = typed.trimmingCharacters(in: .whitespaces)
        return name.isEmpty ? .ignored : .accepted(name)
    }
    func fieldReferences(_ texts: [String]) async throws -> [[String]] {
        texts.map { text in text.matches(of: /\{\{[#^\/]?([^}:]*)\}\}/).map { String($0.1) } }
    }
}

/// `notetypes/workspace` W2, W3, W13 and `notetypes/draft-save` D1, D3, D5, D8 through the section's model.
@MainActor @Suite struct NoteTypesModelTests {
    private static let losing = NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldRemoved(ord: 1, name: "Back", notesWithContent: 12)],
                                                     losesContent: true)

    private func open(_ backend: FakeNoteTypesBackend = FakeNoteTypesBackend()) async -> (NoteTypesModel, FakeNoteTypesBackend) {
        let model = NoteTypesModel(backend: backend)
        model.summaryDelay = .zero
        model.appear()
        await model.settle()
        return (model, backend)
    }

    @Test func theListSelectsTheFirstTypeAndFiltersByName() async {
        let (model, _) = await open()
        #expect(model.phase == .ready && model.selection == 7)
        #expect(model.items.map(\.notes) == [20, 5, 0])
        #expect(model.records[7]?.name == "Sample" && model.typeCounts[7]?.cards == 31)
        model.filter = "BAS"
        #expect(model.filteredItems.map(\.id) == [9], "a case-folded substring")
        model.filter = "  a "
        #expect(model.filteredItems.map(\.id) == [7, 8, 9])
        model.filter = "zzz"
        #expect(model.filteredItems.isEmpty)
    }

    @Test func aDraftIsEditedSurvivesSwitchingAndRevertDiscardsIt() async {
        let backend = FakeNoteTypesBackend()
        backend.records[8] = NoteTypeDraftTests.record(fields: ["Word"])
        let (model, _) = await open(backend)
        #expect(!model.isEdited(7))
        model.deleteField(7, row: 2)
        #expect(model.isEdited(7))
        model.selection = 8
        await model.settle()
        model.selection = 7
        #expect(model.isEdited(7) && model.draft(7)?.rows[2].deleted == true, "D1: the draft outlives the selection")
        model.revert(7)
        #expect(!model.isEdited(7) && model.draft(7)?.rows[2].deleted == false)
    }

    @Test func undoNamesTheDraftStepAndFallsBackWhenThereIsNone() async {
        let (model, _) = await open()
        #expect(model.undoStep == nil)
        model.deleteField(7, row: 1)
        model.setSortField(7, row: 2)
        #expect(model.undoStep == .changeSortField)
        model.undo()
        #expect(model.undoStep == .deleteField && model.redoStep == .changeSortField)
        model.undo()
        #expect(model.undoStep == nil && !model.isEdited(7))
        model.redo()
        #expect(model.isEdited(7))
        // Edit › Undo: the draft's step while Note Types shows, else the collection's.
        var inputs = EditUndoRouting.Inputs(collectionStep: "Update Note Type")
        inputs.draftStep = model.undoStep?.rawValue
        #expect(EditUndoRouting.route(inputs) == .init(target: .draft, title: .named("Delete Field"), enabled: true))
        inputs.draftStep = nil
        #expect(EditUndoRouting.route(inputs) == .init(target: .collection, title: .named("Update Note Type"), enabled: true))
        inputs.draftStep = "Delete Field"; inputs.textOwnsUndo = true
        #expect(EditUndoRouting.route(inputs).target == .text, "a text view keeps its own undo")
    }

    @Test func renameChecksV5AndARefusalKeepsTheOldName() async throws {
        let (model, _) = await open()
        model.addField(7)
        let renaming: Int? = model.renaming
        let added = try #require(renaming)
        #expect(model.selectedField[7] == added)
        await model.commitRename(7, row: added, typed: "back")
        #expect(model.renameRefusal == .init(row: added, refusal: .duplicate))
        #expect(model.renaming == added, "the row stays in rename mode")
        #expect(model.draft(7)?.row(added)?.name == "Field 1")
        await model.commitRename(7, row: added, typed: "Origin")
        #expect(model.renameRefusal == nil && model.renaming == nil)
        #expect(model.draft(7)?.row(added)?.name == "Origin")
        // A case-only rename of the field itself is accepted.
        await model.commitRename(7, row: 1, typed: "BACK")
        #expect(model.draft(7)?.row(1)?.name == "BACK")
    }

    @Test func usedOnAndTheNotesColumnComeFromTheCore() async {
        let (model, _) = await open()
        #expect(model.usedOn(7) == [0: [0], 1: [0], 2: [0]])
        let row = model.draft(7)!.rows[1]
        #expect(model.notesWithContent(7, row: row)! == (12, 20))
        model.addField(7)
        let added = model.draft(7)!.rows.last!
        #expect(model.notesWithContent(7, row: added)! == (0, 20), "a new field shows 0")
    }

    // MARK: Save

    @Test func aSaveWithoutLossesSavesAtOnceWithNothingConfirmed() async {
        let (model, backend) = await open()
        model.setOption(7, row: 0) { $0.rtl = true }
        await model.settle()
        #expect(!model.saveAsks(7))
        model.save()
        await model.settle()
        #expect(model.question == nil)
        #expect(backend.saved.count == 1 && backend.saved[0].confirmed == NotetypeConfirmedLosses())
        #expect(backend.saved[0].fields[0].rtl && backend.saved[0].skipChecks, "the field editor's save is unchecked (V1)")
        #expect(!model.isEdited(7), "D5 step 6: the draft is gone")
    }

    @Test func aLossAsksAndTheSaveCarriesTheConfirmedCounts() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [Self.losing]
        let (model, _) = await open(backend)
        var presented = 0
        model.presentQuestion = { presented += 1; return true }
        model.deleteField(7, row: 1)
        await model.settle()
        #expect(model.saveAsks(7), "Save… while the sheet will show")
        model.save()
        await model.settle()
        #expect(presented == 1 && model.question?.decision.destructive == true && model.question?.decision.syncParagraph == false)
        #expect(backend.saved.isEmpty, "nothing before the sheet is accepted")
        #expect(!model.editable(7))
        model.confirm()
        await model.settle()
        #expect(model.question == nil)
        #expect(backend.saved.count == 1 && backend.saved[0].confirmed.fields == [1: 12])
    }

    @Test func cancelLeavesTheDraftAndWritesNothing() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [Self.losing]
        let (model, _) = await open(backend)
        model.deleteField(7, row: 1)
        model.save()
        await model.settle()
        model.cancelQuestion()
        #expect(model.question == nil && backend.saved.isEmpty && model.isEdited(7) && model.editable(7))
    }

    /// D5 step 4: the read right before submitting shows the sheet again with the grown counts.
    @Test func lossesThatGrowBeforeSubmittingAskAgain() async {
        let backend = FakeNoteTypesBackend()
        let grown = NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldRemoved(ord: 1, name: "Back", notesWithContent: 15)],
                                         losesContent: true)
        let (model, _) = await open(backend)
        model.deleteField(7, row: 1)
        await model.settle()
        backend.summaries = [Self.losing, grown]
        let reads = backend.summaryReads
        model.save()
        await model.settle()
        model.confirm()
        await model.settle()
        #expect(backend.summaryReads == reads + 2)
        #expect(backend.saved.isEmpty)
        #expect(model.question?.summary == grown, "the sheet shows the new counts")
        model.confirm()
        await model.settle()
        #expect(backend.saved.count == 1 && backend.saved[0].confirmed.fields == [1: 15])
    }

    /// D8: `LossesChanged` saves nothing and shows the question again with the op's counts.
    @Test func lossesChangedShowsTheQuestionAgainAndSavesNothing() async {
        let backend = FakeNoteTypesBackend()
        let live = NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldRemoved(ord: 1, name: "Back", notesWithContent: 30)],
                                        losesContent: true)
        backend.summaries = [Self.losing]
        backend.saveErrors = [BridgeError.notetypeStructureRefused(.lossesChanged(summary: live))]
        let (model, _) = await open(backend)
        model.deleteField(7, row: 1)
        model.save()
        await model.settle()
        model.confirm()
        await model.settle()
        #expect(backend.saved.isEmpty && model.isEdited(7))
        #expect(model.question?.summary == live && model.progress == nil)
        model.confirm()
        await model.settle()
        #expect(backend.saved.count == 1 && backend.saved[0].confirmed.fields == [1: 30])
    }

    @Test func aSaveWithoutASheetThatMeetsLossesChangedAsks() async {
        let backend = FakeNoteTypesBackend()
        backend.saveErrors = [BridgeError.notetypeStructureRefused(.lossesChanged(summary: Self.losing))]
        let (model, _) = await open(backend)
        model.deleteField(7, row: 2)
        model.save()
        await model.settle()
        #expect(backend.saved.isEmpty && model.question?.summary == Self.losing)
    }

    /// D1: `Stale` shows the changed-elsewhere row; Reload discards the draft.
    @Test func staleShowsTheRowAndReloadReadsAgain() async {
        let backend = FakeNoteTypesBackend()
        backend.saveErrors = [BridgeError.stale(table: .notetypes, key: "7")]
        let (model, _) = await open(backend)
        model.renameField()
        model.save()
        await model.settle()
        #expect(model.changedElsewhere == [7] && model.isEdited(7))
        model.keepEditing(7)
        #expect(model.changedElsewhere.isEmpty && model.isEdited(7))
        model.reload(7)
        await model.settle()
        #expect(!model.isEdited(7))
    }

    @Test func otherRefusalsKeepTheDraftAndSayNothingChanged() async {
        let backend = FakeNoteTypesBackend()
        backend.saveErrors = [BridgeError.internal("backup failed")]
        let (model, _) = await open(backend)
        model.renameField()
        model.save()
        await model.settle()
        #expect(model.failure == .notSaved && model.isEdited(7) && backend.saved.isEmpty)
    }

    /// D3 with a ready Meguri Sync binding: structural changes ask, and until U269's job exists Save and Upload cannot run.
    @Test func meguriSyncAsksAndRoutesToItsJob() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        model.service = { .meguriSync }
        model.addField(7)
        model.cancelRename()
        #expect(model.saveAsks(7))
        model.save()
        await model.settle()
        #expect(model.question?.decision.route == .meguriSync && model.question?.decision.syncParagraph == true)
        #expect(!model.canConfirm)
        model.confirm()
        await model.settle()
        #expect(backend.saved.isEmpty, "never D5's local save alone with a ready binding")
        var handed: NotetypeStructureDraft?
        model.meguriSyncSave = { draft, _ in handed = draft }
        #expect(model.canConfirm)
        model.confirm()
        await model.settle()
        #expect(handed?.fields.last?.name == "Field 1" && backend.saved.isEmpty)
        // A rename-only draft with Meguri Sync: no sheet, a local save that syncs normally.
        model.revert(7)
        backend.summaries = [NotetypeDraftSummary(reasons: [], changes: [.fieldRenamed(from: "Front", to: "Prompt")], losesContent: false)]
        model.renameField()
        model.save()
        await model.settle()
        #expect(model.question == nil && backend.saved.count == 1)
    }

    @Test func aChangeElsewhereToADraftedTypeShowsTheRow() async {
        let backend = FakeNoteTypesBackend()
        let (model, _) = await open(backend)
        model.renameField()
        backend.records[7] = NotetypeRecord(id: 7, name: "Sample", kind: 0, sortField: 0, css: ".card {}", base: [9, 9],
                                            fields: NoteTypeDraftTests.record().fields, templates: NoteTypeDraftTests.record().templates)
        model.changesCommitted(.notetypes([7]))
        await model.settle()
        #expect(model.changedElsewhere == [7])
        #expect(model.draft(7)?.base.base == [1, 2, 3], "the draft keeps its base, so Save meets Stale")
    }
}

extension NoteTypesModelTests {
    @Test func editsWaitWhileASaveRunsAndAForeignSheetAsksNothing() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [Self.losing]
        let (model, _) = await open(backend)
        model.deleteField(7, row: 1)
        model.presentQuestion = { false }
        model.save()
        await model.settle()
        #expect(model.question == nil && model.failure == .notSaved && backend.saved.isEmpty && model.editable(7))
    }

    @Test func textNotCommittedYetGoesIntoTheDraftBeforeSave() async {
        let (model, backend) = await open()
        model.addField(7)
        let row = model.renaming ?? 0
        model.pendingEdits["rename"] = { await model.commitRename(7, row: row, typed: "Typed") }
        model.save()
        await model.settle()
        #expect(backend.saved.last?.fields.last?.name == "Typed")
    }

    @Test func aRefusedPendingNameStopsSaveAndPendingTextAloneEnablesIt() async {
        let (model, backend) = await open()
        model.pendingEdits["inspector"] = { await model.commitRename(7, row: 0, typed: "Prompt") }
        #expect(model.canSave(7), "text not yet committed enables Save")
        model.pendingEdits["inspector"] = { await model.commitRename(7, row: 0, typed: "back") }
        model.save()
        await model.settle()
        #expect(backend.saved.isEmpty && model.renameRefusal == .init(row: 0, refusal: .duplicate))
        model.pendingEdits["inspector"] = { await model.commitRename(7, row: 0, typed: "Prompt") }
        model.save()
        await model.settle()
        #expect(backend.saved.last?.fields.first?.name == "Prompt")
    }

    @Test func saveWaitsForACommitStartedWhenFocusLeft() async {
        let (model, backend) = await open()
        model.setOption(7, row: 1) { $0.rtl = true }
        model.trackCommit("name.0") { await model.commitRename(7, row: 0, typed: "back") }
        // The corrected name replaces the refused one under the same key.
        model.trackCommit("name.0") { await model.commitRename(7, row: 0, typed: "Prompt") }
        model.save()
        await model.settle()
        #expect(backend.saved.last?.fields.first?.name == "Prompt" && backend.saved.last?.fields[1].rtl == true)
        #expect(!model.committingForSave)
    }

    @Test func aCrossedOutNameIsFreeForAnotherField() async {
        let (model, _) = await open()
        model.deleteField(7, row: 2)
        model.addField(7)
        let row = model.renaming ?? 0
        await model.commitRename(7, row: row, typed: "Extra")
        #expect(model.renameRefusal == nil && model.draft(7)?.row(row)?.name == "Extra")
    }

    @Test func meguriSyncsJobLocksTheNoteTypeAndItsRefusalsAreHandled() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        model.service = { .meguriSync }
        var locked = false
        model.meguriSyncSave = { _, _ in
            locked = !model.editable(7)
            throw BridgeError.stale(table: .notetypes, key: "7")
        }
        model.addField(7)
        model.cancelRename()
        model.save()
        await model.settle()
        model.confirm()
        await model.settle()
        #expect(locked && model.changedElsewhere == [7] && model.isEdited(7) && model.editable(7))
    }

    @Test func anUneditedDraftTakesTheNewerRecord() async {
        let backend = FakeNoteTypesBackend()
        let (model, _) = await open(backend)
        model.renameField()
        model.undo()
        backend.records[7] = NotetypeRecord(id: 7, name: "Sample", kind: 0, sortField: 0, css: ".card {}", base: [9, 9],
                                            fields: NoteTypeDraftTests.record().fields, templates: NoteTypeDraftTests.record().templates)
        model.changesCommitted(.notetypes([7]))
        await model.settle()
        #expect(model.changedElsewhere.isEmpty && model.draft(7)?.base.base == [9, 9])
    }
}

private extension NoteTypesModel {
    func renameField() { edit(7) { $0.rename(0, to: "Prompt") } }
}

private extension ChangeSet {
    static func notetypes(_ ids: [Int64]) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: .syncIncoming, cards: [], notes: [], notetypes: ids.map { .init(id: $0, change: .updated, schemaChanged: false) },
                  decks: [], deckConfigs: [], configKeys: [], tags: [], revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [],
                  skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false)
    }
}
