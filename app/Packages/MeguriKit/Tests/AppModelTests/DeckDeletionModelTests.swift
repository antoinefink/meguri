@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// N20.7 (decision 4B): Delete Deck asks only when cards would be lost.
@MainActor
@Suite struct DeckDeletionModelTests {
    typealias Message = DeckDeletionModel.Message

    @Test func messageFollowsTheSubdeckAndCardTable() {
        #expect(Message(subdecks: 0, cards: 0) == nil)
        #expect(Message(subdecks: 4, cards: 0) == nil)
        #expect(Message(subdecks: 0, cards: 1) == .oneCard)
        #expect(Message(subdecks: 0, cards: 412) == .cards(412))
        #expect(Message(subdecks: 1, cards: 1) == .subdeckAndOneCard)
        #expect(Message(subdecks: 1, cards: 2) == .subdeckAndCards(2))
        #expect(Message(subdecks: 2, cards: 1) == .subdecksAndOneCard(2))
        #expect(Message(subdecks: 3, cards: 1_204) == .subdecksAndCards(3, 1_204))
    }

    @Test(arguments: [Int64(3), 2])
    func noCardToLoseDeletesAtOnce(deck: Int64) async {
        let owner = await makeOwner(), model = owner.deckDeletion
        var reads: [[Int64]] = [], deletes: [[Int64]] = []
        model.readSummary = { reads.append($0); return DeckDeletionSummary(decks: 1, cards: 0) }
        model.deleteDecks = { deletes.append($0); return deletion(removed: [$0[0]]) }
        model.delete(deckID: deck)
        #expect(model.busy)
        await model.waitForCompletion()
        #expect(reads == [[deck]] && deletes == [[deck]])
        #expect(model.confirmation == nil && model.failure == nil && !model.busy)
    }

    @Test func cardsAskFirstAndConfirmRereadsBeforeDeleting() async throws {
        let owner = await makeOwner(), model = owner.deckDeletion
        var reads = 0, deletes: [[Int64]] = []
        model.readSummary = { _ in reads += 1; return DeckDeletionSummary(decks: 2, cards: 5) }
        model.deleteDecks = { deletes.append($0); return deletion(removed: [1, 2]) }
        model.delete(deckID: 1)
        await model.waitForCompletion()
        let question = try #require(model.confirmation)
        #expect(question.deckID == 1 && question.name == "Parent")
        #expect(question.subdecks == 1 && question.cards == 5 && question.message == .subdeckAndCards(5))
        #expect(deletes.isEmpty && !model.canDelete(deckID: 3))
        model.confirm()
        #expect(model.confirmation == nil && model.busy)
        await model.waitForCompletion()
        #expect(reads == 2 && deletes == [[1]])
        #expect(model.confirmation == nil && model.failure == nil)
        #expect(model.canDelete(deckID: 3))
    }

    @Test func changedSummaryAsksAgainBeforeAnythingIsDeleted() async throws {
        let owner = await makeOwner(), model = owner.deckDeletion
        var summaries = [DeckDeletionSummary(decks: 1, cards: 1), .init(decks: 1, cards: 3), .init(decks: 1, cards: 3)]
        var deletes = 0
        model.readSummary = { _ in summaries.removeFirst() }
        model.deleteDecks = { deletes += 1; return deletion(removed: $0) }
        model.delete(deckID: 2)
        await model.waitForCompletion()
        #expect(model.confirmation?.message == .oneCard && model.confirmation?.name == "Child")
        model.confirm(); await model.waitForCompletion()
        #expect(deletes == 0)
        #expect(model.confirmation?.cards == 3 && model.confirmation?.message == .cards(3))
        model.confirm(); await model.waitForCompletion()
        #expect(deletes == 1 && model.confirmation == nil && summaries.isEmpty)
    }

    @Test func emptiedSummaryDeletesWithoutAskingAgain() async {
        let owner = await makeOwner(), model = owner.deckDeletion
        var summaries = [DeckDeletionSummary(decks: 1, cards: 2), .init(decks: 1, cards: 0)]
        var deletes = 0
        model.readSummary = { _ in summaries.removeFirst() }
        model.deleteDecks = { deletes += 1; return deletion(removed: $0) }
        model.delete(deckID: 2); await model.waitForCompletion()
        model.confirm(); await model.waitForCompletion()
        #expect(deletes == 1 && model.confirmation == nil)
    }

    @Test func cancelChangesNothingAndANewGenerationDropsTheQuestion() async {
        let owner = await makeOwner(), model = owner.deckDeletion
        var deletes = 0
        model.readSummary = { _ in DeckDeletionSummary(decks: 1, cards: 1) }
        model.deleteDecks = { deletes += 1; return deletion(removed: $0) }
        owner.navigation.openDeckPage(DeckID(2))
        model.delete(deckID: 2); await model.waitForCompletion()
        model.cancel()
        #expect(model.confirmation == nil && deletes == 0)
        #expect(owner.navigation.homeDeckPage == DeckID(2))
        model.delete(deckID: 2); await model.waitForCompletion()
        #expect(model.confirmation != nil)
        owner.receiveLifecycle(.ready(generation: 12))
        #expect(model.confirmation == nil)
        model.confirm(); await model.waitForCompletion()
        #expect(deletes == 0)
    }

    @Test func aReplacedCollectionDiscardsAnInFlightResult() async {
        let owner = await makeOwner(), model = owner.deckDeletion, gate = DeletionGate()
        model.readSummary = { _ in DeckDeletionSummary(decks: 1, cards: 0) }
        model.deleteDecks = { _ in await gate.hold(); throw BridgeError.internal("late") }
        owner.navigation.openDeckPage(DeckID(3))
        model.delete(deckID: 3); await gate.entered()
        owner.receiveLifecycle(.ready(generation: 12))
        gate.release(); await model.waitForCompletion()
        #expect(model.failure == nil && model.confirmation == nil && !model.busy)
    }

    @Test(arguments: [false, true])
    func deletingTheShownDeckOrItsAncestorReturnsToTheRoot(ancestor: Bool) async {
        let owner = await makeOwner(), model = owner.deckDeletion
        model.readSummary = { _ in DeckDeletionSummary(decks: 1, cards: 0) }
        // Only the deck's own row is reported removed; the subtree comes from the tree.
        model.deleteDecks = { deletion(removed: [$0[0]]) }
        owner.navigation.openDeckPage(DeckID(2))
        #expect(owner.navigation.sidebarSelection == .deck(DeckID(2)))
        model.delete(deckID: ancestor ? 1 : 2); await model.waitForCompletion()
        #expect(owner.navigation.homeDeckPage == nil && owner.navigation.selectedDeck == nil)
        #expect(owner.navigation.sidebarSelection == .section(.home))
        #expect(owner.navigation.compactPath(.decks).isEmpty)
    }

    @Test func otherDecksPagesStayAndTheEmptiedDefaultDeckLeavesItsPage() async {
        let owner = await makeOwner(), model = owner.deckDeletion
        model.readSummary = { _ in DeckDeletionSummary(decks: 1, cards: 0) }
        model.deleteDecks = { deletion(removed: $0 == [1] ? [] : $0) }
        owner.navigation.openDeckPage(DeckID(2))
        model.delete(deckID: 3); await model.waitForCompletion()
        #expect(owner.navigation.homeDeckPage == DeckID(2))
        // The default deck is kept, renamed and emptied (D8): its page closes too.
        owner.navigation.openDeckPage(DeckID(1))
        model.delete(deckID: 1); await model.waitForCompletion()
        #expect(owner.navigation.homeDeckPage == nil)
        // In another section, Home's state resets without moving there.
        owner.navigation.openDeckPage(DeckID(2)); owner.navigation.show(.browse)
        model.delete(deckID: 2); await model.waitForCompletion()
        #expect(owner.navigation.section == .browse && owner.navigation.selectedDeck == nil)
    }

    @Test(arguments: [false, true])
    func failureWritesNothingAndExplainsTheBackup(backup: Bool) async throws {
        let owner = await makeOwner(), model = owner.deckDeletion
        model.readSummary = { _ in DeckDeletionSummary(decks: 1, cards: 1) }
        model.deleteDecks = { _ in
            throw BridgeError.internal(backup ? "safety backup failed: SafetyBackupFailure { cause: InsufficientSpace }" : "disk I/O")
        }
        owner.navigation.openDeckPage(DeckID(2))
        model.delete(deckID: 2); await model.waitForCompletion()
        model.confirm(); await model.waitForCompletion()
        let failure = try #require(model.failure)
        #expect(failure.deckID == 2 && failure.name == "Child")
        #expect(failure.reason == (backup ? .backupFailed : .other))
        #expect(owner.navigation.homeDeckPage == DeckID(2))
        #expect(!model.canDelete(deckID: 2))
        model.dismissFailure()
        #expect(model.failure == nil && model.canDelete(deckID: 2))
    }

    @Test func aDeckAlreadyGoneWritesNothing() async {
        let owner = await makeOwner(), model = owner.deckDeletion
        var deletes = 0
        model.readSummary = { _ in DeckDeletionSummary(decks: 0, cards: 0) }
        model.deleteDecks = { deletes += 1; return deletion(removed: $0) }
        model.delete(deckID: 2); await model.waitForCompletion()
        #expect(deletes == 0 && model.confirmation == nil && model.failure == nil)
    }

    @Test func backgroundingBeforeTheWriteStopsItAndSaysSo() async {
        let owner = await makeOwner(), model = owner.deckDeletion, gate = DeletionGate()
        var deletes = 0
        model.readSummary = { _ in await gate.hold(); return DeckDeletionSummary(decks: 1, cards: 0) }
        model.deleteDecks = { deletes += 1; return deletion(removed: $0) }
        model.delete(deckID: 2); await gate.entered()
        let drained = Task { await model.cancelAndDrain() }
        await Task.yield()
        gate.release(); await drained.value
        #expect(deletes == 0 && model.failure?.reason == .other && !model.busy)
    }

    @Test func summaryReadFailureShowsTheGenericAlert() async {
        let owner = await makeOwner(), model = owner.deckDeletion
        var deletes = 0
        model.readSummary = { _ in throw BridgeError.notFound(table: .decks, key: "2") }
        model.deleteDecks = { deletes += 1; return deletion(removed: $0) }
        model.delete(deckID: 2); await model.waitForCompletion()
        #expect(model.failure?.reason == .other && deletes == 0 && model.confirmation == nil)
    }

    @Test func reasonRecognizesBothBackupFailureShapes() {
        #expect(DeckDeletionModel.reason(.maintenance(.safetyBackupFailed)) == .backupFailed)
        #expect(DeckDeletionModel.reason(.internal("safety backup failed: x")) == .backupFailed)
        #expect(DeckDeletionModel.reason(.internal("other")) == .other)
        #expect(DeckDeletionModel.reason(.cancelled) == .other)
    }

    @Test func enablementMirrorsOtherDeckWritesAndReservesWhileRunning() async {
        let owner = await makeOwner(), model = owner.deckDeletion, gate = DeletionGate()
        #expect(model.canDelete(deckID: 1) && model.canDelete(deckID: 2) && model.canDelete(deckID: 3))
        #expect(!model.canDelete(deckID: 999))
        #expect(!model.canDelete(deckID: 1, host: .settings))
        owner.navigation.present(.collectionExport)
        #expect(!model.canDelete(deckID: 1))
        owner.navigation.dismissSheet()
        owner.entry.pickerPresented = true
        #expect(!model.canDelete(deckID: 1))
        owner.entry.pickerPresented = false

        var reads = 0
        model.readSummary = { _ in reads += 1; await gate.hold(); return DeckDeletionSummary(decks: 1, cards: 0) }
        model.deleteDecks = { deletion(removed: $0) }
        model.delete(deckID: 3); model.delete(deckID: 3)
        #expect(model.busy && !model.canDelete(deckID: 1))
        #expect(!owner.deckActions.canPresent(.create) && !owner.deckActions.canPresent(.rename(1)))
        #expect(!owner.canStudy)
        await gate.entered()
        gate.release(); await model.waitForCompletion()
        #expect(reads == 1 && !model.busy && model.canDelete(deckID: 1) && owner.deckActions.canPresent(.create))
    }

    @Test func theQuestionKeepsWhereItWasAskedAcrossARereadAndTheMenuBarAsksInTheWindow() async throws {
        let owner = await makeOwner(), model = owner.deckDeletion
        var summaries = [DeckDeletionSummary(decks: 1, cards: 1), .init(decks: 1, cards: 2)]
        model.readSummary = { _ in summaries.removeFirst() }
        model.deleteDecks = { deletion(removed: $0) }
        model.delete(deckID: 2, anchor: .row(place: "sidebar")); await model.waitForCompletion()
        #expect(model.confirmation?.anchor == .row(place: "sidebar"))
        model.confirm(); await model.waitForCompletion()
        #expect(model.confirmation?.anchor == .row(place: "sidebar") && model.confirmation?.cards == 2)
        model.cancel()
        summaries = [DeckDeletionSummary(decks: 1, cards: 1)]
        model.delete(deckID: 2); await model.waitForCompletion()
        #expect(try #require(model.confirmation).anchor == .window)
    }

    @Test func editUndoIsTitledDeleteDeckAfterADeleteAndWaitsWhileItRuns() async {
        let owner = await makeOwner(), model = owner.deckDeletion, history = owner.collectionUndo
        let undo = DeletionUndo(), gate = DeletionGate()
        history.service = undo
        await history.waitForRefresh()
        model.readSummary = { _ in DeckDeletionSummary(decks: 1, cards: 0) }
        model.deleteDecks = { ids in
            await gate.hold(); await undo.record("Delete Deck"); return deletion(removed: ids)
        }
        model.delete(deckID: 3); await gate.entered()
        #expect(!history.canUndo, "The history waits for the delete")
        gate.release(); await model.waitForCompletion(); await history.waitForRefresh()
        #expect(history.undoName == "Delete Deck" && history.canUndo)
        let route = EditUndoRouting.route(.init(collectionStep: history.undoName))
        #expect(route == .init(target: .collection, title: .named("Delete Deck"), enabled: true))
        history.undo(); await history.waitForCompletion()
        #expect(history.redoName == "Delete Deck" && history.undoName == nil)
    }

    @Test func aConfirmedDeleteThatCannotStartSaysNothingWasDeleted() async {
        let owner = await makeOwner(), model = owner.deckDeletion, gate = DeletionGate()
        var deletes = 0
        model.readSummary = { _ in DeckDeletionSummary(decks: 1, cards: 3) }
        model.deleteDecks = { ids in deletes += 1; return deletion(removed: ids) }
        model.delete(deckID: 2); await model.waitForCompletion()
        #expect(model.confirmation != nil)
        // Another deck write starts while the question is up.
        owner.deckActions.startRename = { _, _ in await gate.hold(); throw BridgeError.cancelled }
        owner.deckActions.present(.rename(3)); owner.deckActions.name = "Renamed"
        owner.deckActions.submit(); await gate.entered()
        model.confirm(); await model.waitForCompletion()
        #expect(deletes == 0 && model.confirmation == nil)
        #expect(model.failure?.reason == .other && model.failure?.deckID == 2)
        gate.release(); await owner.deckActions.waitForCompletion()
    }

    @Test func aRunningRenameBlocksDeletion() async {
        let owner = await makeOwner(), gate = DeletionGate()
        owner.deckActions.startRename = { _, _ in await gate.hold(); throw BridgeError.cancelled }
        owner.deckActions.present(.rename(2)); owner.deckActions.name = "Renamed"
        owner.deckActions.submit(); await gate.entered()
        owner.navigation.dismissSheet()
        #expect(!owner.deckDeletion.canDelete(deckID: 3))
        gate.release(); await owner.deckActions.waitForCompletion()
    }

    private func makeOwner() async -> ReviewCollection {
        var configuration = LaunchConfiguration()
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let counts = ReviewCounts(new: 0, learning: 0, review: 0)
        let child = CountedDeckNode(id: 2, name: "Child", filtered: false, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        let parent = CountedDeckNode(id: 1, name: "Parent", filtered: false, studyCollapsed: false,
            browserCollapsed: true, counts: counts, children: [child])
        let filtered = CountedDeckNode(id: 3, name: "Filtered", filtered: true, studyCollapsed: true,
            browserCollapsed: true, counts: counts, children: [])
        owner.loadDeckTree = { CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: [parent, filtered]) }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        return owner
    }
}

private func deletion(removed: [Int64], generation: UInt64 = 11) -> DeckDeletion {
    let changes = ChangeSet(wholeCollection: false, source: .userUndoable, cards: [], notes: [], notetypes: [],
        decks: removed.map { .init(id: $0, change: .removed) }, deckConfigs: [], configKeys: [], tags: [],
        revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [],
        skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false)
    return DeckDeletion(summary: MutationSummary(seq: 1, generation: generation, undoName: "Delete Deck",
        changes: changes, pastCancelPoint: true), deleted: DeckDeletionSummary(decks: UInt64(removed.count), cards: 0))
}

private actor DeletionUndo: UndoService {
    private var undoName: String?
    private var redoName: String?
    func record(_ name: String) { undoName = name; redoName = nil }
    private func status(_ name: String?) -> ReviewUndoStatus {
        name.map { .available(step: ReviewStepInfo(id: 1, name: $0, kind: .other, session: nil, longWrite: nil)) }
            ?? .unavailable(reason: .none)
    }
    private var head: ReviewHead {
        ReviewHead(items: [], counts: .init(new: 0, learning: 0, review: 0), validUntil: Int64.max, terminalState: .finished)
    }
    func peekUndo(scope: ReviewUndoScope) throws(BridgeError) -> ReviewUndoStatus { status(undoName) }
    func peekRedo(scope: ReviewUndoScope) throws(BridgeError) -> ReviewUndoStatus { status(redoName) }
    func startUndo(scope: ReviewUndoScope) throws(BridgeError) -> CoreUndoStart {
        redoName = undoName; undoName = nil
        return .restored(head: head)
    }
    func startRedo(scope: ReviewUndoScope) throws(BridgeError) -> CoreUndoStart {
        undoName = redoName; redoName = nil
        return .restored(head: head)
    }
}

@MainActor
private final class DeletionGate {
    private var didEnter = false
    private var entry: CheckedContinuation<Void, Never>?
    private var continuation: CheckedContinuation<Void, Never>?
    func entered() async {
        if didEnter { return }
        await withCheckedContinuation { entry = $0 }
    }
    func hold() async {
        await withCheckedContinuation {
            continuation = $0; didEnter = true; entry?.resume(); entry = nil
        }
    }
    func release() { continuation?.resume(); continuation = nil }
}
