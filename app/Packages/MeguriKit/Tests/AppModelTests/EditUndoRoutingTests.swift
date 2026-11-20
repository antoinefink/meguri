import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// R173, R174, N20.4: one Undo and one Redo, routed to the text input, the
/// review or the collection, titled after what they undo.
@MainActor
@Suite struct EditUndoRoutingTests {
    @Test func aTextInputThatOwnsUndoKeepsIt() {
        let route = EditUndoRouting.route(.init(textOwnsUndo: true, textAvailable: true, textTitle: "Undo Typing",
            reviewShown: true, reviewOwnsKeys: true, reviewStep: "Answer Card", collectionStep: "Rename Deck"))
        #expect(route == .init(target: .text, title: .text("Undo Typing"), enabled: true))
        let nothing = EditUndoRouting.route(.init(textOwnsUndo: true, textAvailable: false, textTitle: "Undo",
            collectionStep: "Rename Deck"))
        #expect(nothing == .init(target: .text, title: .plain, enabled: false), "The text keeps the key even with nothing to undo")
    }

    @Test func reviewUndoesItsAnswersWhileTheCardOwnsTheKeys() {
        #expect(EditUndoRouting.route(.init(reviewShown: true, reviewOwnsKeys: true, reviewStep: "Answer Card",
            collectionStep: "Rename Deck")) == .init(target: .review, title: .named("Answer Card"), enabled: true))
        #expect(EditUndoRouting.route(.init(reviewShown: true, reviewOwnsKeys: false, reviewStep: "Answer Card"))
            == .init(target: .review, title: .plain, enabled: false))
        #expect(EditUndoRouting.route(.init(reviewShown: true, reviewOwnsKeys: true))
            == .init(target: .review, title: .plain, enabled: false))
    }

    @Test func outsideReviewTheCollectionsHistory() {
        #expect(EditUndoRouting.route(.init(collectionStep: "Rename Deck"))
            == .init(target: .collection, title: .named("Rename Deck"), enabled: true))
        #expect(EditUndoRouting.route(.init()) == .init(target: .collection, title: .plain, enabled: false))
    }

    @Test func collectionUndoIsTitledAdmittedOnceAndRefreshesTheTree() async {
        let owner = await makeOwner(), history = owner.collectionUndo, fake = FakeUndo()
        history.service = fake
        await history.waitForRefresh(); await history.refreshNow()
        #expect(history.undoName == "Rename Deck" && history.redoName == nil)
        #expect(history.canUndo && !history.canRedo)
        var loads = 0
        owner.loadDeckTree = { loads += 1; return CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: []) }
        history.undo(); history.undo()
        #expect(history.busy && !history.canUndo, "Reserved before the first await")
        #expect(!owner.deckActions.canPresent(.create), "Other collection work waits for it")
        await history.waitForCompletion()
        #expect(await fake.undos == 1)
        #expect(loads >= 1, "The tree is read again after the undo")
        #expect(history.undoName == nil && history.redoName == "Rename Deck")
        #expect(history.canRedo)
    }

    @Test func collectionUndoWaitsWhileASheetHoldsTheKeys() async {
        let owner = await makeOwner(), history = owner.collectionUndo
        history.service = FakeUndo()
        await history.waitForRefresh(); await history.refreshNow()
        #expect(history.canUndo)
        owner.navigation.present(.collectionExport)
        #expect(!history.canUndo && history.undoName == "Rename Deck")
        owner.navigation.dismissSheet()
        #expect(history.canUndo)
    }

    @Test(arguments: [false, true])
    func anUndoReopensTheShownPageOnlyIfThePersonStayed(navigates: Bool) async {
        let owner = await makeOwner(), history = owner.collectionUndo
        history.service = FakeUndo()
        await history.waitForRefresh(); await history.refreshNow()
        owner.navigation.openDeckPage(DeckID(2))
        owner.saveSelectedDeck = { _ in 11 }
        let tree = owner.decks
        var held: CheckedContinuation<Void, Never>?
        owner.loadDeckTree = {
            await withCheckedContinuation { held = $0 }
            return CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: tree)
        }
        history.undo()
        while held == nil { await Task.yield() }
        // The person opens another deck while the undo's tree read waits.
        if navigates { owner.navigation.openDeckPage(DeckID(3)) }
        let intent = owner.navigation.intentRevision
        held?.resume()
        await history.waitForCompletion()
        // Reopening the page is a new deck selection, which moves the intent on.
        #expect((owner.navigation.intentRevision != intent) == !navigates,
            "The shown page reads again only where the person still is")
    }

    @Test func aNewGenerationPeeksItsOwnHistory() async {
        let owner = await makeOwner(), history = owner.collectionUndo, fake = FakeUndo()
        history.service = fake
        await history.waitForRefresh(); await history.refreshNow()
        let before = await fake.peeks
        owner.receiveLifecycle(.ready(generation: 12))
        await history.waitForRefresh()
        #expect(await fake.peeks > before, "Same tree, new collection: its undo history is read")
    }

    @Test func findIsDisabledDuringReviewAndOutsideTheDeckWorkspace() async {
        let owner = await makeOwner()
        #expect(owner.deckSearchAvailable(layout: .sidebar))
        owner.navigation.show(.settings)
        #expect(!owner.deckSearchAvailable(layout: .sidebar), "Only while the deck workspace shows")
        owner.navigation.show(.home)
        owner.navigation.present(.collectionExport)
        #expect(!owner.deckSearchAvailable(layout: .sidebar), "Not while a sheet owns the keys")
        owner.navigation.dismissSheet()
        let fixture = ReviewCoordinatorTests.Fixture()
        await fixture.model.start()
        owner.presentReview(fixture.model)
        #expect(!owner.deckSearchAvailable(layout: .sidebar) && !owner.deckSearchAvailable(layout: .compact))
        _ = await fixture.model.end()
    }

    private func makeOwner() async -> ReviewCollection {
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        let counts = ReviewCounts(new: 0, learning: 0, review: 0)
        let deck = CountedDeckNode(id: 2, name: "QZ60", filtered: false, studyCollapsed: true, browserCollapsed: true,
            counts: counts, children: [])
        let other = CountedDeckNode(id: 3, name: "QZ69", filtered: false, studyCollapsed: true, browserCollapsed: true,
            counts: counts, children: [])
        owner.loadDeckTree = { CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: [deck, other]) }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        return owner
    }
}

/// A history with one step; undo moves it to redo.
private actor FakeUndo: UndoService {
    var undoNameValue: String? = "Rename Deck"
    var redoNameValue: String?
    var undos = 0
    var peeks = 0
    private func status(_ name: String?) -> ReviewUndoStatus {
        name.map { .available(step: ReviewStepInfo(id: 1, name: $0, kind: .other, session: nil, longWrite: nil)) }
            ?? .unavailable(reason: .none)
    }
    private var head: ReviewHead {
        ReviewHead(items: [], counts: .init(new: 0, learning: 0, review: 0), validUntil: Int64.max, terminalState: .finished)
    }
    func peekUndo(scope: ReviewUndoScope) throws(BridgeError) -> ReviewUndoStatus { peeks += 1; return status(undoNameValue) }
    func peekRedo(scope: ReviewUndoScope) throws(BridgeError) -> ReviewUndoStatus { status(redoNameValue) }
    func startUndo(scope: ReviewUndoScope) throws(BridgeError) -> CoreUndoStart {
        undos += 1; redoNameValue = undoNameValue; undoNameValue = nil
        return .restored(head: head)
    }
    func startRedo(scope: ReviewUndoScope) throws(BridgeError) -> CoreUndoStart {
        undoNameValue = redoNameValue; redoNameValue = nil
        return .restored(head: head)
    }
}
