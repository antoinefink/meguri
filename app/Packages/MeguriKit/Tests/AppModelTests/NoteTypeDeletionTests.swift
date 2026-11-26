@testable import AppModel
import CoreBridge
import Foundation
import Testing

@MainActor private final class RemovalSync: NoteTypesMeguriSync {
    var noteTypeSyncState: NoteTypeSyncState = .ready
    var bindingPresent: Bool? = true
    func deletionService() async -> NoteTypeSaveService? {
        guard noteTypeSyncState != .busy, noteTypeSyncState != .uploadPending,
              let bindingPresent else { return nil }
        if !bindingPresent { return .other }
        return noteTypeSyncState == .ready ? .meguriSync : nil
    }
    var reachable = true
    var result: NoteTypeSyncSaveResult = .saved(uploaded: true)
    var error: BridgeError?
    var requests: [(NotetypeRemoval, Bool)] = []
    func noteTypeSyncReachable() async -> Bool { reachable }
    func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                               progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void) async throws -> NoteTypeSyncSaveResult {
        Issue.record("Delete must use MP7 Remove, never Save")
        return .notSaved
    }
    func removeNotetypeStructure(_ request: NotetypeRemoval, reachable: Bool,
                                 progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void) async throws -> NoteTypeSyncSaveResult {
        requests.append((request, reachable))
        progress(.saving, 9, nil)
        if let error { throw error }
        return result
    }
    func cancelNotetypeStructureSave() {}
}

/// W12, S12, D4–D8 through the actual model, including zero loss categories.
@MainActor @Suite struct NoteTypeDeletionTests {
    private func open() async -> (NoteTypesModel, FakeNoteTypesBackend) {
        let backend = FakeNoteTypesBackend()
        let model = NoteTypesModel(backend: backend)
        model.summaryDelay = .zero
        model.appear()
        await model.settle()
        return (model, backend)
    }

    @Test func cancelKeepsTheDraftAndCommitsPendingTextWithoutSaving() async {
        let (model, backend) = await open()
        model.pendingEdits["test"] = {
            model.setText(7, card: 0, part: .front, "Pending text")
            return true
        }
        model.delete(7)
        await model.settle()
        #expect(model.deletionQuestion?.counts == .init(notes: 20, cards: 31))
        #expect(!model.editable(7) && !model.canSave(7))
        #expect(model.isEdited(7))
        model.cancelQuestion()
        #expect(model.deletionQuestion == nil && model.isEdited(7))
        #expect(backend.removals.isEmpty && backend.saved.isEmpty)
    }

    @Test func deleteRetiresOnlyItsDraftAndSelectsARemainingType() async {
        let (model, backend) = await open()
        model.setText(7, card: 0, part: .front, "Draft to retire")
        model.delete(7)
        await model.settle()
        backend.beforeRemoval = {
            #expect(model.progress?.removing == true)
            #expect(model.progress?.completed == 12 && model.progress?.total == nil)
            #expect(!model.canRequestDeletion && !model.canSave(7))
        }
        model.filter = "Sample"
        model.confirmDeletion()
        await model.settle()
        #expect(backend.removals == [.init(notetypeId: 7, confirmedNotes: 20, confirmedCards: 31)])
        #expect(!model.items.contains { $0.id == 7 } && model.selection == 8)
        #expect(!model.isEdited(7) && model.draft(7) == nil)
        #expect(model.filter.isEmpty && model.failure == nil && model.progress == nil)
        #expect(backend.saved.isEmpty)
    }

    @Test(arguments: [NotetypeRemovalCounts(notes: 21, cards: 30), .init(notes: 19, cards: 32)])
    func eachGrowingCategoryReconfirmsEvenWhenTheTotalShrinks(_ latest: NotetypeRemovalCounts) async {
        let (model, backend) = await open()
        model.delete(7)
        await model.settle()
        backend.removalCounts = latest
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.counts == latest && backend.removals.isEmpty)
        model.confirmDeletion()
        await model.settle()
        #expect(backend.removals.first?.confirmedNotes == latest.notes)
        #expect(backend.removals.first?.confirmedCards == latest.cards)
    }

    @Test func zeroNotesStillConfirmAndGainingNotesAsksAgain() async {
        let (model, backend) = await open()
        backend.removalCounts = .init(notes: 0, cards: 0)
        model.delete(7)
        await model.settle()
        #expect(model.deletionQuestion?.counts == .init(notes: 0, cards: 0))
        backend.removalCounts = .init(notes: 1, cards: 1)
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.counts == .init(notes: 1, cards: 1))
        #expect(backend.removals.isEmpty)
    }

    @Test func theTransactionRecountRefusalShowsTheNewCountsAndKeepsTheDraft() async {
        let (model, backend) = await open()
        model.setText(7, card: 0, part: .front, "Keep on refusal")
        model.delete(7)
        await model.settle()
        backend.removalErrors = [BridgeError.notetypeStructureRefused(.removalLossesChanged(notes: 22, cards: 35))]
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.counts == .init(notes: 22, cards: 35))
        #expect(model.isEdited(7) && model.failure == nil)
    }

    @Test func aServiceChangeReopensTheQuestionBeforeAnySubmission() async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        model.meguriSync = sync
        model.delete(7)
        await model.settle()
        model.service = { .meguriSync }
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.service == .meguriSync)
        #expect(backend.removals.isEmpty && sync.requests.isEmpty)
        model.service = { .other }
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.service == .other && backend.removals.isEmpty)
    }

    @Test(arguments: [NoteTypeSyncState.busy, .uploadPending])
    func meguriChosenWithoutReadyAdmissionNeverFallsBackToLocal(_ state: NoteTypeSyncState) async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        sync.noteTypeSyncState = state
        sync.bindingPresent = false
        model.meguriSync = sync
        model.service = { .meguriSync }
        model.deletionService = { await sync.deletionService() }
        model.delete(7)
        await model.settle()
        #expect(!model.canConfirmDeletion)
        model.confirmDeletion()
        await model.settle()
        #expect(backend.removals.isEmpty && sync.requests.isEmpty)
    }

    @Test func selectedMeguriWithoutABindingDeletesLocally() async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        sync.noteTypeSyncState = .notSetUp
        sync.bindingPresent = false
        model.meguriSync = sync
        model.service = { .meguriSync }
        model.deletionService = { await sync.deletionService() }
        model.delete(7)
        await model.settle()
        #expect(model.deletionQuestion?.service == .other && model.canConfirmDeletion)
        model.confirmDeletion()
        await model.settle()
        #expect(backend.removals == [.init(notetypeId: 7, confirmedNotes: 20, confirmedCards: 31)])
        #expect(sync.requests.isEmpty && model.failure == nil)
    }

    @Test(arguments: [true, false])
    func bindingAvailabilityChangesReconfirmInBothDirections(_ initiallyBound: Bool) async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        sync.bindingPresent = initiallyBound
        sync.noteTypeSyncState = initiallyBound ? .ready : .notSetUp
        model.meguriSync = sync
        model.service = { .meguriSync }
        model.deletionService = { await sync.deletionService() }
        model.delete(7)
        await model.settle()
        #expect(model.deletionQuestion?.service == (initiallyBound ? .meguriSync : .other))
        sync.bindingPresent = !initiallyBound
        sync.noteTypeSyncState = initiallyBound ? .notSetUp : .ready
        backend.removalCounts = .init(notes: 19, cards: 30)
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.service == (initiallyBound ? .other : .meguriSync))
        #expect(model.deletionQuestion?.counts == backend.removalCounts)
        #expect(backend.removals.isEmpty && sync.requests.isEmpty)
        model.confirmDeletion()
        await model.settle()
        let accepted = NotetypeRemoval(notetypeId: 7, confirmedNotes: 19, confirmedCards: 30)
        #expect(backend.removals == (initiallyBound ? [accepted] : []))
        #expect(sync.requests.map { $0.0 } == (initiallyBound ? [] : [accepted]))
    }

    @Test(arguments: [true, false])
    func unknownOrStaleBindingNeverCountsAsOff(_ unknown: Bool) async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        sync.noteTypeSyncState = .notSetUp
        sync.bindingPresent = unknown ? nil : true
        model.meguriSync = sync
        model.service = { .meguriSync }
        model.deletionService = { await sync.deletionService() }
        model.delete(7)
        await model.settle()
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.service == .meguriSync)
        #expect(backend.removals.isEmpty && sync.requests.isEmpty)
    }

    @Test func bindingReadAfterRecountUsesTheNewAvailability() async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        model.meguriSync = sync
        model.service = { .meguriSync }
        model.deletionService = { await sync.deletionService() }
        model.delete(7)
        await model.settle()
        backend.beforeRemovalCounts = {
            sync.bindingPresent = false
            sync.noteTypeSyncState = .notSetUp
        }
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion?.service == .other)
        #expect(backend.removals.isEmpty && sync.requests.isEmpty)
    }

    @Test func collectionReplacementDuringBindingReadCannotRemove() async {
        let (model, backend) = await open()
        model.delete(7)
        await model.settle()
        model.deletionService = {
            await Task.yield()
            model.invalidate()
            return .other
        }
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion == nil && backend.removals.isEmpty)
    }

    @Test(arguments: [true, false])
    func meguriRemovalCarriesConfirmedCountsAndReachability(_ reachable: Bool) async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        sync.reachable = reachable
        sync.result = .saved(uploaded: reachable)
        model.meguriSync = sync
        model.service = { .meguriSync }
        model.setText(7, card: 0, part: .front, "To retire")
        model.delete(7)
        await model.settle()
        #expect(model.meguriUnreachable == !reachable)
        model.confirmDeletion()
        await model.settle()
        #expect(sync.requests.count == 1 && sync.requests.first?.1 == reachable)
        #expect(sync.requests.first?.0 == .init(notetypeId: 7, confirmedNotes: 20, confirmedCards: 31))
        #expect(backend.removals.isEmpty && !model.isEdited(7))
    }

    @Test func failedSyncAndRemovalRefusalKeepDraftsAndDoNotRemove() async {
        let (model, backend) = await open()
        let sync = RemovalSync()
        sync.result = .syncStopped
        model.meguriSync = sync
        model.service = { .meguriSync }
        model.setText(7, card: 0, part: .front, "Keep")
        model.delete(7)
        await model.settle()
        model.confirmDeletion()
        await model.settle()
        #expect(model.isEdited(7) && model.failure == .syncStopped)
        #expect(backend.removals.isEmpty)
        model.failure = nil
        model.service = { .other }
        backend.removalErrors = [BridgeError.unavailable(reason: "Safety backup failed")]
        model.delete(7)
        await model.settle()
        model.confirmDeletion()
        await model.settle()
        #expect(model.isEdited(7) && model.failure == .notSaved)
    }

    @Test func theLastTypeIsProtectedByBothListAndFreshConfirmation() async {
        let (model, backend) = await open()
        model.delete(7)
        await model.settle()
        backend.names = [backend.names[0]]
        model.confirmDeletion()
        await model.settle()
        #expect(model.items.count == 1 && !model.canDelete(7))
        #expect(model.deletionQuestion == nil && backend.removals.isEmpty)
        model.delete(7)
        await model.settle()
        #expect(model.deletionQuestion == nil)
    }

    @Test func collectionReplacementDuringCountsCannotSubmitOrPresentOldWork() async {
        let (model, backend) = await open()
        backend.beforeRemovalCounts = {
            backend.beforeRemovalCounts = nil
            model.invalidate()
        }
        model.delete(7)
        await model.settle()
        #expect(model.deletionQuestion == nil && backend.removals.isEmpty)
        #expect(model.progress == nil)
    }

    @Test func collectionReplacementDuringReconfirmationCannotRemoveTheNewCollection() async {
        let (model, backend) = await open()
        model.delete(7)
        await model.settle()
        backend.beforeRemovalCounts = {
            backend.beforeRemovalCounts = nil
            model.invalidate()
        }
        model.confirmDeletion()
        await model.settle()
        #expect(model.deletionQuestion == nil && backend.removals.isEmpty)
        #expect(model.progress == nil)
    }

    @Test func anInflightDraftCommitFinishesBeforeTheQuestionAndSurvivesCancel() async {
        let (model, _) = await open()
        model.trackCommit(7, "name") {
            await Task.yield()
            return await model.commitRename(7, row: 0, typed: "Changed field")
        }
        model.delete(7)
        await model.settle()
        #expect(model.deletionQuestion != nil && model.isEdited(7))
        model.cancelQuestion()
        #expect(model.draft(7)?.rows.first?.name == "Changed field")
    }
}
