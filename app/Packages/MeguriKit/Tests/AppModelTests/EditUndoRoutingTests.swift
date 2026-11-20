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
        owner.loadDeckTree = { CountedDeckTree(generation: 11, nextChangeAt: Int64.max, nodes: [deck]) }
        owner.receiveLifecycle(.ready(generation: 11)); await owner.loadCounts()
        return owner
    }
}

/// A history with one step; undo moves it to redo.
private actor FakeUndo: UndoService {
    var undoNameValue: String? = "Rename Deck"
    var redoNameValue: String?
    var undos = 0
    private func status(_ name: String?) -> ReviewUndoStatus {
        name.map { .available(step: ReviewStepInfo(id: 1, name: $0, kind: .other, session: nil, longWrite: nil)) }
            ?? .unavailable(reason: .none)
    }
    private var head: ReviewHead {
        ReviewHead(items: [], counts: .init(new: 0, learning: 0, review: 0), validUntil: Int64.max, terminalState: .finished)
    }
    func peekUndo(scope: ReviewUndoScope) throws(BridgeError) -> ReviewUndoStatus { status(undoNameValue) }
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
