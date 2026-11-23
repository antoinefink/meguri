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
    /// Each summary read waits this long first.
    var summaryWait: Duration?
    /// Each list read throws the first of these.
    var listErrors: [any Error] = []
    private(set) var listReads = 0
    private(set) var summaryReads = 0
    private(set) var saved: [NotetypeStructureDraft] = []

    func list() async throws -> [NotetypeName] {
        listReads += 1
        if !listErrors.isEmpty { throw listErrors.removeFirst() }
        return names
    }
    func noteCounts() async throws -> [Int64: UInt64] { [7: 20, 8: 5] }
    /// Each record read waits this long first.
    var readWait: Duration?
    private(set) var reads: [Int64] = []
    func read(_ id: Int64) async throws -> NotetypeRecord {
        reads.append(id)
        if let readWait { try? await Task.sleep(for: readWait) }
        guard let record = records[id] else { throw BridgeError.notFound(table: .notetypes, key: "\(id)") }
        return record
    }
    /// Each type count read waits this long first.
    var countsWait: Duration?
    func typeCounts(_ id: Int64) async throws -> NotetypeRemovalCounts {
        if let countsWait { try? await Task.sleep(for: countsWait) }
        return NotetypeRemovalCounts(notes: 20, cards: 31)
    }
    func fieldContentCounts(_ id: Int64) async throws -> NotetypeFieldContentCounts { contentCounts }
    func summary(_ draft: NotetypeStructureDraft) async throws -> NotetypeDraftSummary {
        summaryReads += 1
        if let summaryWait { try? await Task.sleep(for: summaryWait) }
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
    var cardCounts: [UInt64] = [20, 11]
    /// Every issues read returns this; it records the drafts it was asked about.
    var issues: [NotetypeTemplateIssue] = []
    private(set) var issueReads: [NotetypeStructureDraft] = []
    var blankFronts: [NotetypeBlankFront] = []
    private(set) var blankReads = 0
    func templateCardCounts(_ id: Int64) async throws -> [UInt64] { cardCounts }
    func templateIssues(_ draft: NotetypeStructureDraft) async throws -> [NotetypeTemplateIssue] {
        issueReads.append(draft)
        return issues
    }
    func notesWithoutCard(_ draft: NotetypeStructureDraft) async throws -> [NotetypeBlankFront] {
        blankReads += 1
        return blankFronts
    }
    func replaceFieldReferences(_ text: String, from: String, to: String) async throws -> String {
        text.replacingOccurrences(of: "{{\(from)}}", with: "{{\(to)}}")
    }
    func syntax(_ text: String, styling: Bool) async throws -> [NotetypeSyntaxSpan] { [] }
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

    /// First run: no collection is not a failed read, and nothing offers Try Again.
    @Test func withNoCollectionTheListReadsNothingAndHasNotFailed() async {
        let backend = FakeNoteTypesBackend()
        let model = NoteTypesModel(backend: backend)
        let exists = Flag()
        model.hasCollection = { exists.value }
        model.appear()
        await model.settle()
        #expect(model.phase == .noCollection && backend.listReads == 0 && model.items.isEmpty && model.selection == nil)
        // Start Empty (or an import, or sync) opens one: the section reads it at once.
        exists.value = true
        model.invalidate()
        await model.settle()
        #expect(model.phase == .ready && backend.listReads == 1 && model.selection == 7)
        // The collection goes while the list is read: still no failure.
        backend.listErrors = [BridgeError.unavailable(reason: "no collection")]
        model.reloadList()
        exists.value = false
        await model.settle()
        #expect(model.phase == .noCollection && model.items.isEmpty && model.selection == nil)
    }

    /// A real read failure with a collection keeps the failure state and its Try Again.
    @Test func aFailedReadWithACollectionIsAFailure() async {
        let backend = FakeNoteTypesBackend()
        backend.listErrors = [BridgeError.unavailable(reason: "busy")]
        let model = NoteTypesModel(backend: backend)
        model.appear()
        await model.settle()
        #expect(model.phase == .failed)
        model.reloadList()
        await model.settle()
        #expect(model.phase == .ready && backend.listReads == 2)
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
        // Extra (ordinal 2) was empty when Save read it and gained content before the op ran.
        let grown = NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldRemoved(ord: 2, name: "Extra", notesWithContent: 3)],
                                         losesContent: true)
        backend.saveErrors = [BridgeError.notetypeStructureRefused(.lossesChanged(summary: grown))]
        let (model, _) = await open(backend)
        model.deleteField(7, row: 2)
        model.save()
        await model.settle()
        #expect(backend.saved.isEmpty && model.question?.summary == grown)
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
        model.trackCommit(7, "name.0") { await model.commitRename(7, row: 0, typed: "back") }
        // The corrected name replaces the refused one under the same key.
        model.trackCommit(7, "name.0") { await model.commitRename(7, row: 0, typed: "Prompt") }
        model.save()
        await model.settle()
        #expect(backend.saved.last?.fields.first?.name == "Prompt" && backend.saved.last?.fields[1].rtl == true)
        #expect(!model.committingForSave)
    }

    @Test func aRefusalThatEndedBeforeSaveStillHoldsSave() async {
        let (model, backend) = await open()
        model.setOption(7, row: 1) { $0.rtl = true }
        model.trackCommit(7, "name.0") { await model.commitRename(7, row: 0, typed: "back") }
        await model.settle()
        #expect(model.renameRefusal != nil && !model.canSave(7))
        model.save()
        await model.settle()
        #expect(backend.saved.isEmpty)
        model.cancelRename()
        #expect(model.canSave(7))
    }

    @Test func aRefusalForAnotherNoteTypeNeitherShowsNorHoldsSave() async {
        let backend = FakeNoteTypesBackend()
        backend.records[8] = NoteTypeDraftTests.record(fields: ["Word", "Back"])
        let (model, _) = await open(backend)
        model.selection = 8
        await model.settle()
        model.setOption(8, row: 0) { $0.rtl = true }
        // A refused rename of type 7 that ends while type 8 shows.
        let accepted = await model.commitRename(7, row: 0, typed: "back")
        #expect(!accepted && model.renameRefusal == nil && model.canSave(8))
    }

    @Test func anotherNoteTypesRefusalStillRunningDoesNotStopSave() async {
        let backend = FakeNoteTypesBackend()
        backend.records[8] = NoteTypeDraftTests.record(fields: ["Word", "Back"])
        let (model, _) = await open(backend)
        // Type 7's name check is still running when type 8's Save starts, and is refused after.
        var release: CheckedContinuation<Void, Never>?
        model.trackCommit(7, "name.0") {
            await withCheckedContinuation { release = $0 }
            return await model.commitRename(7, row: 0, typed: "back")
        }
        await model.settle()
        model.selection = 8
        await model.settle()
        model.setOption(8, row: 0) { $0.rtl = true }
        model.save(8)
        release?.resume()
        await model.settle()
        #expect(backend.saved.last?.fields.first?.name == "Word" && backend.saved.last?.fields.first?.rtl == true)
        #expect(model.draft(7)?.row(0)?.name == "Front", "type 7's refused name changed nothing")
    }

    @Test func aDraftChangedElsewhereReadsItsTypeCountsButNotTheNewerLayoutsFieldCounts() async {
        let backend = FakeNoteTypesBackend()
        let (model, _) = await open(backend)
        model.renameField()
        backend.records[7] = NotetypeRecord(id: 7, name: "Sample", kind: 0, sortField: 0, css: ".card {}", base: [9, 9],
                                            fields: NoteTypeDraftTests.record().fields, templates: NoteTypeDraftTests.record().templates)
        backend.contentCounts = NotetypeFieldContentCounts(notes: 20, fields: [1, 2, 3])
        model.changesCommitted(.notetypes([7]))
        await model.settle()
        #expect(model.changedElsewhere == [7])
        #expect(model.typeCounts[7]?.cards == 31, "the subtitle's counts read again")
        #expect(model.fieldCounts[7] == nil && model.notesWithContent(7, row: model.draft(7)!.rows[1]) == nil,
                "the newer layout's counts are not put on the draft's rows")
        model.reload(7)
        await model.settle()
        #expect(model.notesWithContent(7, row: model.draft(7)!.rows[1])?.count == 2)
    }

    @Test func keepEditingThenRevertReadsTheNewerVersion() async {
        let backend = FakeNoteTypesBackend()
        let (model, _) = await open(backend)
        model.renameField()
        backend.records[7] = NotetypeRecord(id: 7, name: "Sample", kind: 0, sortField: 0, css: ".card {}", base: [9, 9],
                                            fields: NoteTypeDraftTests.record().fields, templates: NoteTypeDraftTests.record().templates)
        model.changesCommitted(.notetypes([7]))
        await model.settle()
        model.keepEditing(7)
        model.revert(7)
        await model.settle()
        #expect(model.draft(7)?.base.base == [9, 9])
    }

    @Test func aCrossedOutNameIsFreeForAnotherField() async {
        let (model, _) = await open()
        model.deleteField(7, row: 2)
        model.addField(7)
        let row = model.renaming ?? 0
        await model.commitRename(7, row: row, typed: "Extra")
        #expect(model.renameRefusal == nil && model.draft(7)?.row(row)?.name == "Extra")
    }

    /// D3 with Meguri Sync chosen: structural changes ask; Save and Upload runs only with a ready binding.
    @Test func meguriSyncAsksAndRoutesToItsJob() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        let sync = FakeMeguriSync()
        sync.state = .notSetUp
        model.service = { .meguriSync }
        model.meguriSync = sync
        model.addField(7)
        model.cancelRename()
        #expect(model.saveAsks(7))
        model.save()
        await model.settle()
        #expect(model.question?.decision.route == .meguriSync && model.question?.decision.syncParagraph == true)
        #expect(!model.canConfirm && model.meguriSyncState == .notSetUp, "set up Meguri Sync first")
        model.confirm()
        await model.settle()
        #expect(backend.saved.isEmpty && sync.saves.isEmpty, "never D5's local save alone while Meguri Sync is chosen")
        sync.state = .uploadPending
        #expect(!model.canConfirm)
        sync.state = .ready
        #expect(model.canConfirm)
        model.confirm()
        await model.settle()
        #expect(sync.saves.map { $0.draft.fields.last?.name } == ["Field 1"] && sync.saves.first?.reachable == true && backend.saved.isEmpty)
        #expect(!model.isEdited(7) && model.notice == nil && model.failure == nil)
        // A rename-only draft with Meguri Sync: no sheet, a local save that syncs normally.
        backend.summaries = [NotetypeDraftSummary(reasons: [], changes: [.fieldRenamed(from: "Front", to: "Prompt")], losesContent: false)]
        model.renameField()
        model.save()
        await model.settle()
        #expect(model.question == nil && backend.saved.count == 1 && sync.saves.count == 1)
    }

    /// Stage a structural draft and accept its sheet with Meguri Sync.
    private func confirmWithMeguriSync(_ sync: FakeMeguriSync, reachable: Bool = true) async -> (NoteTypesModel, FakeNoteTypesBackend) {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        sync.reachable = reachable
        model.service = { .meguriSync }
        model.meguriSync = sync
        model.addField(7)
        model.cancelRename()
        model.save()
        await model.settle()
        model.confirm()
        await model.settle()
        return (model, backend)
    }

    @Test func eachResultOfTheJobShowsWhatHappened() async {
        // Saved and uploaded: the draft goes, nothing more to say.
        var sync = FakeMeguriSync()
        var (model, _) = await confirmWithMeguriSync(sync)
        #expect(!model.isEdited(7) && model.notice == nil && model.failure == nil && model.progress == nil && model.editable(7))
        // Saved here, not uploaded: the draft goes and the person is told.
        sync = FakeMeguriSync(); sync.results = [.saved(uploaded: false)]
        (model, _) = await confirmWithMeguriSync(sync)
        #expect(!model.isEdited(7) && model.notice == .notUploaded && model.failure == nil)
        // The round stopped: nothing changed, the draft stays, the sync status says why.
        sync = FakeMeguriSync(); sync.results = [.syncStopped]
        (model, _) = await confirmWithMeguriSync(sync)
        #expect(model.isEdited(7) && model.failure == .syncStopped && model.editable(7))
        // Cancelled before the save: the draft stays and nothing is said.
        sync = FakeMeguriSync(); sync.results = [.cancelled]
        (model, _) = await confirmWithMeguriSync(sync)
        #expect(model.isEdited(7) && model.failure == nil && model.notice == nil)
        // The job could not run.
        sync = FakeMeguriSync(); sync.results = [.notSaved]
        (model, _) = await confirmWithMeguriSync(sync)
        #expect(model.isEdited(7) && model.failure == .notSaved)
    }

    /// D4's unreachable case: the sheet says so, its button reads Save, and the upload waits without an alert.
    @Test func offlineTheSheetSaysSoAndTheSaveRunsWithTheUploadPending() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        let sync = FakeMeguriSync()
        sync.reachable = false
        sync.results = [.saved(uploaded: false)]
        model.service = { .meguriSync }
        model.meguriSync = sync
        model.addField(7)
        model.cancelRename()
        model.save()
        await model.settle()
        #expect(model.meguriUnreachable && model.canConfirm)
        model.confirm()
        await model.settle()
        #expect(sync.saves.first?.reachable == false && model.progress == nil)
        #expect(!model.isEdited(7) && model.notice == nil, "the sheet already said the upload waits")
    }

    @Test func meguriSyncsJobLocksTheNoteTypeAndItsRefusalsAreHandled() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        let sync = FakeMeguriSync()
        model.service = { .meguriSync }
        model.meguriSync = sync
        let seen = SyncSeen()
        sync.during = {
            seen.locked = !model.editable(7)
            seen.stages.append(model.progress?.stage)
        }
        sync.errors = [BridgeError.stale(table: .notetypes, key: "7")]
        model.addField(7)
        model.cancelRename()
        model.save()
        await model.settle()
        model.confirm()
        await model.settle()
        #expect(seen.locked && seen.stages == [.uploading] && model.changedElsewhere == [7] && model.isEdited(7) && model.editable(7))
        // LossesChanged from the job: the question again with the new counts; accepting it starts a new job.
        backend.summaries = [Self.losing]
        sync.errors = [BridgeError.notetypeStructureRefused(.lossesChanged(summary: Self.losing))]
        model.keepEditing(7)
        model.reload(7)
        await model.settle()
        model.deleteField(7, row: 1)
        model.save()
        await model.settle()
        model.confirm()
        await model.settle()
        #expect(model.question?.summary == Self.losing && model.isEdited(7))
        model.confirm()
        await model.settle()
        #expect(sync.saves.count == 3 && sync.saves.last?.draft.confirmed.fields[1] == 12 && !model.isEdited(7))
    }

    /// Leaving while Save and Upload reads D5 step 4's summary: no job starts, so leaving does not wait for one.
    @Test func leavingBeforeTheJobStartsStartsNone() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        let sync = FakeMeguriSync()
        model.service = { .meguriSync }
        model.meguriSync = sync
        model.addField(7)
        model.cancelRename()
        model.save()
        await model.settle()
        backend.summaryWait = .milliseconds(100)
        model.confirm()
        await model.cancelAndDrain()
        #expect(sync.saves.isEmpty && model.isEdited(7) && model.progress == nil && model.editable(7))
    }

    @Test func anUnconfirmedSaveKeepsTheDraftAndSaysSo() async {
        let sync = FakeMeguriSync()
        sync.results = [.unconfirmed]
        let (model, _) = await confirmWithMeguriSync(sync)
        #expect(model.isEdited(7) && model.failure == .unconfirmed && model.editable(7))
    }

    /// Leaving (the background, a close) asks the job to stop where MP7 allows, so it never waits for a whole upload.
    @Test func leavingCancelsTheRunningJob() async {
        let backend = FakeNoteTypesBackend()
        backend.summaries = [NotetypeDraftSummary(reasons: [.fieldsChanged], changes: [.fieldAdded(name: "Field 1")], losesContent: false)]
        let (model, _) = await open(backend)
        let sync = FakeMeguriSync()
        sync.waitsForCancel = true
        sync.results = [.saved(uploaded: false)]
        model.service = { .meguriSync }
        model.meguriSync = sync
        model.addField(7)
        model.cancelRename()
        model.save()
        await model.settle()
        model.confirm()
        for _ in 0..<50 { await Task.yield() }
        #expect(model.progress != nil && !model.editable(7))
        await model.cancelAndDrain()
        #expect(sync.cancels == 1 && model.progress == nil && model.editable(7) && !model.isEdited(7))
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

@MainActor private final class Flag { var value = false }

/// A scripted Meguri Sync for the Save sheet: each save takes the first error, else the first result.
@MainActor private final class FakeMeguriSync: NoteTypesMeguriSync {
    var state: NoteTypeSyncState = .ready
    var reachable = true
    var results: [NoteTypeSyncSaveResult] = []
    var errors: [any Error] = []
    var during: (@MainActor () -> Void)?
    var waitsForCancel = false
    private(set) var saves: [(draft: NotetypeStructureDraft, reachable: Bool)] = []
    private(set) var cancels = 0
    private var cancelled: CheckedContinuation<Void, Never>?
    var noteTypeSyncState: NoteTypeSyncState { state }
    func noteTypeSyncReachable() async -> Bool { reachable }
    func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                               progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void)
        async throws -> NoteTypeSyncSaveResult {
        saves.append((draft, reachable))
        progress(.uploading, 0, nil)
        during?()
        if waitsForCancel { await withCheckedContinuation { cancelled = $0 } }
        if !errors.isEmpty { throw errors.removeFirst() }
        return results.isEmpty ? .saved(uploaded: true) : results.removeFirst()
    }
    func cancelNotetypeStructureSave() {
        cancels += 1
        cancelled?.resume()
        cancelled = nil
    }
}

@MainActor private final class SyncSeen {
    var locked = false
    var stages: [NoteTypeSyncStage?] = []
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

/// `notetypes/workspace` W5, W7 and W8 through the section's model.
extension NoteTypesModelTests {
    // MARK: The Cards tab (W5, W7, W8)

    private static let twoCards = NoteTypeCardsTests.record()

    @Test func cardCountsFollowTheBaseOrdinalAndNewCardTypesShowZero() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        let (model, _) = await open(backend)
        let draft = model.draft(7)!
        #expect(draft.cardRows.map { model.cardCount(7, row: $0) } == [20, 11])
        model.addCardType(7)
        let added = model.draft(7)!.cardRows.last!
        #expect(model.cardCount(7, row: added) == 0 && model.renamingCard == added.id && model.shownCard(7) == added.id)
        #expect(model.undoStep == .addCardType)
    }

    @Test func redIssuesBlockACheckedSaveAndShowAtOnce() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        let (model, _) = await open(backend)
        // A stored issue does not hold a Fields tab edit (an unchecked save).
        backend.issues = [NotetypeTemplateIssue(template: 1, part: .front, line: 1, kind: .unknownField(name: "Bakc", suggestion: 1))]
        model.deleteField(7, row: 2)
        await model.settle()
        #expect(model.redIssueCount(7) == 1 && !model.issuesBlockSave(7) && model.canSave(7))
        // Typing in a template makes the save checked: the red issue holds it.
        model.setText(7, card: 1, part: .front, "{{Bakc}}")
        await model.settle()
        #expect(model.issuesBlockSave(7) && !model.canSave(7))
        let line = model.liveIssues(7).first
        #expect(line?.cardName == "Reverse" && line?.fix == .useField(from: "Bakc", to: "Back"))
        // The fix goes through the editor, so it joins the text view's undo.
        model.applyFix(7, line!)
        await model.settle()
        #expect(model.editorRequest?.command == .replaceAll("{{Back}}") && model.selectedCard[7] == 1 && model.cardPart == .front)
    }

    @Test func saveReadsTheIssuesFirstAndStopsOnARedOne() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        let (model, _) = await open(backend)
        model.setText(7, card: 0, part: .back, "{{#Extra}}")
        // Save before the pause's read lands: the save reads the issues itself.
        backend.issues = [NotetypeTemplateIssue(template: 0, part: .back, line: 1, kind: .unclosedSection(tag: "{{#Extra}}", close: "{{/Extra}}"))]
        model.save()
        await model.settle()
        #expect(backend.saved.isEmpty && model.question == nil && model.redIssueCount(7) == 1)
        #expect(model.liveIssues(7).first?.fix == .addClose("{{/Extra}}"))
        // The fix applies only to the text the issue was read on: an earlier
        // close elsewhere does not block it, a text changed since does.
        model.applyFix(7, model.liveIssues(7)[0])
        #expect(model.editorRequest?.command == .append("{{/Extra}}"))
        model.consumeEditorRequest(model.editorRequest!.token)
        model.setText(7, card: 0, part: .back, "{{#Extra}}{{/Extra}}")
        model.applyFix(7, model.liveIssues(7)[0])
        #expect(model.editorRequest == nil)
        backend.issues = []
        model.setText(7, card: 0, part: .back, "{{#Extra}}{{/Extra}}")
        await model.settle()
        model.save()
        await model.settle()
        #expect(backend.saved.count == 1 && backend.saved[0].templates[0].back == "{{#Extra}}{{/Extra}}" && !backend.saved[0].skipChecks)
    }

    @Test func w8ReadsAgainOnlyWhenAFrontOrTheFieldsChange() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        backend.blankFronts = [NotetypeBlankFront(noteIds: [], field: nil), NotetypeBlankFront(noteIds: [3, 4, 5], field: 1)]
        let (model, _) = await open(backend)
        // W8 reads the notes only for the Cards tab, which shows it.
        #expect(backend.blankReads == 0)
        model.tab = .cards
        await model.settle()
        let first = backend.blankReads
        #expect(first == 1 && model.liveIssues(7).map(\.kind) == [.blankFront(notes: 3, field: "Back")])
        model.setText(7, card: 0, part: .back, "changed back")
        model.setText(7, card: 0, part: .styling, ".card { color: red }")
        await model.settle()
        #expect(backend.blankReads == first, "a back or the styling changes no front")
        model.setText(7, card: 0, part: .front, "{{Front}}{{Extra}}")
        await model.settle()
        #expect(backend.blankReads == first + 1)
        // Show Notes is offered only once Browse can take the notes (U287).
        #expect(model.showNotes == nil)
    }

    @Test func reorderingIdenticalCardTypesReadsUsedOnAgain() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = NoteTypeCardsTests.record(templates: [("One", "{{Front}}", "{{Back}}"), ("Two", "{{Front}}", "{{Back}}")])
        let (model, _) = await open(backend)
        #expect(model.usedOn(7) != nil)
        model.moveCardTypes(7, fromOffsets: IndexSet(integer: 0), toOffset: 2)
        await model.settle()
        #expect(model.usedOn(7) == [0: [0, 1], 1: [0, 1], 2: []], "the same texts in another order read again")
    }

    @Test func flipThenSaveCarriesBothTexts() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        let (model, _) = await open(backend)
        model.flip(7, card: 0)
        model.save()
        await model.settle()
        #expect(backend.saved.first?.templates[0].front == "{{Back}}")
        #expect(backend.saved.first?.templates[0].back == "{{FrontSide}}\n\n<hr id=answer>\n\n{{Front}}")
    }

    /// Antoine's "for a moment you will see just this dark page with no rows":
    /// a selection change keeps the shown note type until the new one can draw.
    @Test func aSelectionChangeNeverShowsAnEmptyDetailWhileItsRecordIsRead() async throws {
        let backend = FakeNoteTypesBackend()
        backend.records[9] = NoteTypeCardsTests.record()
        let (model, _) = await open(backend)
        #expect(model.shownSelection == 7)
        backend.readWait = .milliseconds(80)
        model.selection = 9
        #expect(model.shownSelection == 7, "the shown note type stays while 9 is read")
        var checks = 0
        while model.shownSelection != 9, checks < 400 {
            let shown = try #require(model.shownSelection)
            #expect(model.draft(shown) != nil, "never an empty detail")
            #expect(!model.editable(shown) || shown == 9, "the stand-in takes no edits")
            checks += 1
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(model.shownSelection == 9, "the swap happened")
        #expect(checks > 1, "the read was in flight")
        #expect(model.draft(9) != nil, "the detail swaps to 9 in one step, drawn")
    }

    @Test func theTitleAndCountsChangeTogetherOnceTheNewCountsAreIn() async throws {
        let backend = FakeNoteTypesBackend()
        backend.records[9] = NoteTypeCardsTests.record()
        let (model, _) = await open(backend)
        #expect(model.headerSelection == 7 && model.typeCounts[7] != nil)
        backend.readWait = .milliseconds(60)
        backend.countsWait = .milliseconds(60)
        model.selection = 9
        // A change elsewhere drops 7's cached counts: the subtitle shown keeps them.
        model.changesCommitted(.notetypes([7]))
        var checks = 0
        while model.headerSelection != 9, checks < 400 {
            #expect(model.headerSelection != nil && model.headerCounts != nil, "never a title over a blank subtitle")
            checks += 1
            try await Task.sleep(for: .milliseconds(5))
        }
        #expect(model.headerSelection == 9 && model.typeCounts[9] != nil)
        #expect(checks > 1, "the counts were in flight")
    }

    @Test func aFailedReadShowsItsFailureInsteadOfKeepingTheOldNoteType() async {
        let (model, _) = await open()
        model.selection = 9  // the fake has no record for it
        await model.settle()
        #expect(model.shownSelection == 9 && model.recordFailed.contains(9))
        #expect(model.headerSelection == 9, "the title follows a failed read too")
    }

    @Test func theRowsBesideTheSelectionAreReadAheadSoTheyShowAtOnce() async {
        let backend = FakeNoteTypesBackend()
        backend.records[8] = NoteTypeCardsTests.record()
        let (model, _) = await open(backend)
        #expect(model.records[8] != nil, "8 sits beside 7: read ahead")
        #expect(model.typeCounts[8] != nil && model.fieldCounts[8] == nil, "a read ahead reads the subtitle's counts, not the notes")
        model.selection = 8
        #expect(model.shownSelection == 8, "drawn at once")
        await model.settle()
        #expect(backend.reads.filter { $0 == 8 }.count == 1, "read once")
        #expect(model.headerSelection == 8, "its title and counts at once")
        #expect(model.fieldCounts[8] != nil, "selected, it reads its notes' counts")
        #expect(model.usedOn(8) != nil, "and its Used On")
    }

    @Test func aChangeElsewhereDropsARecordOnlyReadAheadInsteadOfCountingIt() async {
        let backend = FakeNoteTypesBackend()
        backend.records[8] = NoteTypeCardsTests.record()
        let (model, _) = await open(backend)
        #expect(model.records[8] != nil)
        model.changesCommitted(.notetypes([8]))
        await model.settle()
        #expect(model.records[8] == nil && model.typeCounts[8] == nil, "read again when needed, never counted ahead")
        model.selection = 8
        await model.settle()
        #expect(model.shownSelection == 8 && model.draft(8) != nil)
    }

    @Test func aReadAheadInFlightDuringAChangeElsewhereIsDropped() async {
        let backend = FakeNoteTypesBackend()
        backend.records[9] = NoteTypeCardsTests.record()
        let (model, _) = await open(backend)
        backend.readWait = .milliseconds(40)
        model.warm(9)
        model.changesCommitted(.notetypes([9]))
        await model.settle()
        #expect(model.records[9] == nil, "the version read before the change is never kept")
    }

    @Test func aNewerReadAheadKeepsItsEntryWhenAnOlderOneEnds() async throws {
        let backend = FakeNoteTypesBackend()
        backend.records[9] = NoteTypeCardsTests.record()
        let (model, _) = await open(backend)
        backend.readWait = .milliseconds(60)
        model.warm(9)
        model.changesCommitted(.notetypes([9]))  // cancels the first read
        try await Task.sleep(for: .milliseconds(10))
        model.warm(9)  // its replacement, still in flight when the first one ends
        try await Task.sleep(for: .milliseconds(55))
        model.changesCommitted(.notetypes([9]))  // must still find and cancel the replacement
        await model.settle()
        #expect(model.records[9] == nil, "no version from before the second change is kept")
    }

    @Test func aRenameBegunOnTheShownNoteTypeEndsWhenTheDetailSwaps() async throws {
        let backend = FakeNoteTypesBackend()
        backend.records[9] = NoteTypeCardsTests.record()
        let (model, _) = await open(backend)
        backend.readWait = .milliseconds(40)
        model.selection = 9
        model.renaming = 0  // begun on the stand-in in the gap
        await model.settle()
        #expect(model.shownSelection == 9 && model.renaming == nil)
    }

    @Test func saveKeepsTheSavedVersionOnScreenUntilItIsReadAgain() async throws {
        let backend = FakeNoteTypesBackend()
        let (model, _) = await open(backend)
        model.setOption(7, row: 0) { $0.rtl = true }
        await model.settle()
        backend.readWait = .milliseconds(60)
        model.save()
        var sawRereading = false
        for _ in 0..<40 {
            #expect(model.draft(7) != nil, "the detail never empties after Save")
            if model.records[7] == nil { sawRereading = true; #expect(!model.editable(7), "read-only until read again") }
            try await Task.sleep(for: .milliseconds(5))
        }
        await model.settle()
        #expect(sawRereading && model.records[7] != nil && model.editable(7))
    }

    @Test func flippedChangesNothingUntilTheEditorWritesIt() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        let (model, _) = await open(backend)
        let flip = model.flipped(7, card: 0)
        #expect(flip == NoteTypeFlip(front: "{{Back}}", back: "{{FrontSide}}\n\n<hr id=answer>\n\n{{Front}}", previousFront: "{{Front}}"))
        #expect(model.draft(7)?.text(0, .front) == "{{Front}}" && !model.isEdited(7))
        model.setText(7, card: 0, part: .back, "<hr id=answer>{{Back}}")
        #expect(model.flipped(7, card: 0) == nil && model.flipRefused, "A15: no text before the separator")
    }

    @Test func replaceTextWritesOnlyAnUnchangedText() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        let (model, _) = await open(backend)
        #expect(model.replaceText(7, card: 0, part: .front, expected: "{{Front}}", with: "{{Back}}"))
        #expect(model.draft(7)?.text(0, .front) == "{{Back}}")
        #expect(!model.replaceText(7, card: 0, part: .front, expected: "{{Front}}", with: "{{Extra}}"), "edited since: kept")
        #expect(model.draft(7)?.text(0, .front) == "{{Back}}")
    }

    @Test func addCloseIgnoresAnEarlierCloseOfTheSameSection() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        let (model, _) = await open(backend)
        backend.issues = [NotetypeTemplateIssue(template: 0, part: .back, line: 1, kind: .unclosedSection(tag: "{{#Extra}}", close: "{{/Extra}}"))]
        model.setText(7, card: 0, part: .back, "{{#Extra}}{{/Extra}}{{#Extra}}")
        await model.settle()
        model.applyFix(7, model.liveIssues(7)[0])
        #expect(model.editorRequest?.command == .append("{{/Extra}}"))
    }

    @Test func aCancelledW8TaskNeverHidesTheNewerRead() async {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = Self.twoCards
        backend.blankFronts = [NotetypeBlankFront(noteIds: [1], field: 0), NotetypeBlankFront(noteIds: [], field: nil)]
        let (model, _) = await open(backend)
        model.tab = .cards
        await model.settle()
        model.summaryDelay = .milliseconds(20)
        // A, then B, then A again before the first task wakes.
        model.setText(7, card: 0, part: .front, "{{Front}} B")
        model.setText(7, card: 0, part: .front, "{{Front}}")
        await model.settle()
        #expect(model.liveIssues(7).contains { $0.kind == .blankFront(notes: 1, field: "Front") })
    }
}

/// A scripted preview core: each render echoes the front or back text it
/// was given as the side's HTML, so a test reads which draft text rendered.
@MainActor private final class FakePreviewBackend: NoteTypePreviewBackend {
    var notes: [Int64] = [3, 5, 9]
    /// The first render waits this long (a slow render overtaken by a newer one).
    var firstWait: Duration?
    private(set) var renders: [(front: String, note: Int64?, target: UInt32, fillEmpty: Bool)] = []
    private(set) var noteReads = 0

    func render(_ draft: NotetypeStructureDraft, note: Int64?, target: UInt32, fillEmpty: Bool) async throws -> NoteTypePreviewRender {
        let template = draft.templates[Int(target)]
        renders.append((template.front, note, target, fillEmpty))
        if renders.count == 1, let firstWait { try? await Task.sleep(for: firstWait) }
        let broken = template.front.contains("{{#") && !template.front.contains("{{/")
        return NoteTypePreviewRender(question: "Q:" + template.front, answer: "A:" + template.back,
                                     outcome: broken ? .templateError(.unclosedConditional, side: .question) : .rendered, noteID: note)
    }

    func noteIds(_ notetype: Int64) async throws -> [Int64] {
        noteReads += 1
        return notes
    }
}

/// `notetypes/workspace` W9 through the section's model: which note shows,
/// the side following Front | Back, unsaved edits, coalesced renders, the
/// error state, and the toggle kept on the device. The page is replaced by
/// a recorder of what it was given.
@MainActor @Suite struct NoteTypePreviewTests {
    private final class Page { var shown: [String] = [] }

    private func open(_ preview: FakePreviewBackend = FakePreviewBackend(),
                      defaults: UserDefaults? = nil) async -> (NoteTypesModel, FakePreviewBackend, Page) {
        let backend = FakeNoteTypesBackend()
        backend.records[7] = NoteTypeDraftTests.record(templates: [("Card 1", "{{Front}}", "{{FrontSide}}<hr id=answer>{{Back}}"),
                                                                   ("Card 2", "{{Back}}", "{{FrontSide}}<hr id=answer>{{Front}}")])
        let model = NoteTypesModel(backend: backend, preview: NoteTypePreview(backend: preview))
        model.summaryDelay = .zero
        model.preview.typingDelay = .milliseconds(40)
        model.preview.defaults = defaults
        let page = Page()
        model.preview.present = { payload in page.shown.append(payload.question); return true }
        model.appear()
        await model.settle()
        model.tab = .cards
        model.preview.visible = true
        await model.preview.settled()
        return (model, preview, page)
    }

    private func settle(_ model: NoteTypesModel) async {
        try? await Task.sleep(for: .milliseconds(80))
        await model.preview.settled()
    }

    @Test func theNewestNoteShowsAndTheMenuStepsThroughTheTypesNotes() async {
        let (model, core, page) = await open()
        #expect(core.renders.last?.note == 9 && model.preview.notePosition! == (3, 3))
        #expect(page.shown == ["Q:{{Front}}"] && !model.preview.fillEmpty)
        model.preview.step(by: -1)
        await model.preview.settled()
        #expect(core.renders.last?.note == 5 && model.preview.notePosition! == (2, 3) && model.preview.canStep == (true, true))
        model.preview.step(by: -5)
        #expect(model.preview.notePosition! == (2, 3), "a step past the ends does nothing")
        // A way in names its note (W14): that one shows.
        let (named, namedCore, _) = await open(FakePreviewBackend())
        named.preview.preferredNote[7] = 3
        named.preview.notesChanged()
        await named.preview.settled()
        #expect(namedCore.renders.last?.note == 3 && named.preview.notePosition! == (1, 3))
    }

    @Test func aTypeWithoutNotesPreviewsANewNoteWithFillEmptyFieldsOn() async {
        let core = FakePreviewBackend()
        core.notes = []
        let (model, _, _) = await open(core)
        #expect(core.renders.last?.note == nil && core.renders.last?.fillEmpty == true && model.preview.notePosition == nil)
        model.preview.setFillEmpty(false)
        await model.preview.settled()
        #expect(core.renders.last?.fillEmpty == false && !model.preview.fillEmpty)
    }

    @Test func theSideFollowsFrontAndBackAndStylingKeepsTheLastOne() async {
        let (model, core, page) = await open()
        model.cardPart = .back
        await model.preview.settled()
        #expect(model.preview.side == .answer && page.shown.last == "A:{{FrontSide}}<hr id=answer>{{Back}}")
        // The same draft and note: the side comes from the render already read.
        #expect(core.renders.count == 1)
        model.cardPart = .styling
        await model.preview.settled()
        #expect(model.preview.side == .answer && page.shown.count == 2)
        model.cardPart = .front
        await model.preview.settled()
        #expect(model.preview.side == .question && page.shown.last == "Q:{{Front}}")
        // Another card type shows its own template, on the side shown.
        model.selectedCard[7] = model.draft(7)!.cardRows[1].id
        await model.preview.settled()
        #expect(core.renders.last?.target == 1 && page.shown.last == "Q:{{Back}}")
    }

    @Test func unsavedEditsRenderAndRevertShowsTheSavedTemplateAgain() async {
        let (model, _, page) = await open()
        model.setText(7, card: 0, part: .front, "<b>{{Front}}</b>")
        await settle(model)
        #expect(page.shown.last == "Q:<b>{{Front}}</b>")
        model.revert(7)
        await settle(model)
        #expect(page.shown.last == "Q:{{Front}}")
    }

    @Test func rapidTypingRendersTheLastTextOnce() async {
        let (model, core, page) = await open()
        let before = core.renders.count
        for text in ["{", "{{", "{{B", "{{Ba", "{{Back}}"] {
            model.setText(7, card: 0, part: .front, text)
            try? await Task.sleep(for: .milliseconds(5))
        }
        await settle(model)
        #expect(core.renders.count == before + 1 && core.renders.last?.front == "{{Back}}")
        #expect(page.shown.last == "Q:{{Back}}" && page.shown.count == 2)
    }

    @Test func aSlowRenderOvertakenByANewerEditNeverShows() async {
        let core = FakePreviewBackend()
        core.firstWait = .milliseconds(150)
        let backend = FakeNoteTypesBackend()
        let model = NoteTypesModel(backend: backend, preview: NoteTypePreview(backend: core))
        model.summaryDelay = .zero
        model.preview.typingDelay = .zero
        model.preview.defaults = nil
        let page = Page()
        model.preview.present = { payload in page.shown.append(payload.question); return true }
        model.appear()
        await model.settle()
        model.tab = .cards
        model.preview.visible = true
        try? await Task.sleep(for: .milliseconds(20))
        // The first render is still running when the front changes.
        model.setText(7, card: 0, part: .front, "{{Back}}")
        try? await Task.sleep(for: .milliseconds(250))
        await model.preview.settled()
        #expect(page.shown == ["Q:{{Back}}"], "the older render's result is dropped")
    }

    @Test func anInvalidTemplateKeepsThePageAndShowsTheErrorUntilFixed() async {
        let (model, _, page) = await open()
        model.setText(7, card: 0, part: .front, "{{#Front}}{{Front}}")
        await settle(model)
        #expect(model.preview.problem == .templateError(side: .question))
        #expect(page.shown == ["Q:{{Front}}"], "the last card stays on the page")
        model.setText(7, card: 0, part: .front, "{{#Front}}{{Front}}{{/Front}}")
        await settle(model)
        #expect(model.preview.problem == nil && page.shown.last == "Q:{{#Front}}{{Front}}{{/Front}}")
    }

    @Test func aHiddenPreviewReadsNothing() async {
        let (model, core, _) = await open()
        model.preview.visible = false
        let before = core.renders.count
        model.setText(7, card: 0, part: .front, "{{Back}}")
        await settle(model)
        #expect(core.renders.count == before)
        model.preview.visible = true
        await model.preview.settled()
        #expect(core.renders.last?.front == "{{Back}}")
    }

    @Test func theToggleIsKeptOnTheDeviceAndTheMacHidesThePreviewWhenNarrow() async {
        let name = "meguri.tests.notetypes.preview.\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defer { defaults.removePersistentDomain(forName: name) }
        let (model, _, _) = await open(defaults: defaults)
        #expect(model.preview.requested, "shown until turned off")
        model.preview.requested = false
        let again = NoteTypePreview(backend: FakePreviewBackend())
        again.defaults = defaults
        #expect(!again.requested)
        let enough = NoteTypePreviewLayout.listWidth + NoteTypePreviewLayout.editorMinimum + NoteTypePreviewLayout.previewMinimum
        #expect(NoteTypePreviewLayout.macShows(requested: true, width: enough))
        #expect(!NoteTypePreviewLayout.macShows(requested: true, width: enough - 1))
        #expect(!NoteTypePreviewLayout.macShows(requested: false, width: 2_000))
    }
}
