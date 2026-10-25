import Testing
import MeguriCoreBindings
@testable import CoreBridge

@Suite struct SkippedIncomingTests {
    private func raw(cards: [Int64] = [], notes: [Int64] = [], decks: [Int64] = []) -> MeguriCoreBindings.ChangeSet {
        .init(
            wholeCollection: false, source: .syncIncoming,
            cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [],
            revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [],
            skippedIncomingCards: cards, skippedIncomingNotes: notes, skippedIncomingDecks: decks,
            collectionRow: false)
    }

    @Test func conversionKeepsNumericDiagnosticsInSummaryAndEvent() {
        let changes = raw(cards: [-2, 70, 901], notes: [70, 9003], decks: [70, 901])
        let summary = CoreBridge.MutationSummary(MeguriCoreBindings.MutationSummary(
            seq: 101, generation: 7, undoName: nil, changes: changes, pastCancelPoint: false))
        #expect(summary.seq == 101 && summary.generation == 7)
        #expect(summary.changes.skippedIncomingCards == [-2, 70, 901])
        #expect(summary.changes.skippedIncomingNotes == [70, 9003])
        #expect(summary.changes.skippedIncomingDecks == [70, 901])
        #expect(summary.changes.cards.isEmpty && summary.changes.notes.isEmpty && summary.changes.decks.isEmpty)
        let event = CoreBridge.CoreEvent(MeguriCoreBindings.CoreEvent(
            seq: 101, generation: 7, causeSeq: nil, kind: .changes(changes: changes)))
        #expect(event.seq == summary.seq && event.generation == summary.generation)
        #expect(event.kind == .changes(summary.changes))
    }

    @Test func emptyConstructorsKeepEveryDiagnosticListEmpty() {
        let changes = CoreBridge.ChangeSet(raw())
        #expect(changes.skippedIncomingCards.isEmpty)
        #expect(changes.skippedIncomingNotes.isEmpty)
        #expect(changes.skippedIncomingDecks.isEmpty)
    }
}
