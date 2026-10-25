import Testing
import MeguriCoreBindings
@testable import CoreBridge

@Suite struct SchedOpsBridgeTests {
    @Test func typedRefusalsKeepKindsAndSortedPresetIDs() {
        let cases: [(MeguriCoreBindings.SchedOpRefusal, CoreBridge.SchedOpRefusal)] = [
            (.invalidDays(kind: .empty), .invalidDays(.empty)),
            (.invalidDays(kind: .malformed), .invalidDays(.malformed)),
            (.invalidDays(kind: .tooLarge), .invalidDays(.tooLarge)),
            (.homeDeckFiltered(deck: -5), .homeDeckFiltered(deck: -5)),
            (.inFilteredDeck(card: 8), .inFilteredDeck(card: 8)),
            (.notFiltered(deck: 91), .notFiltered(deck: 91)),
            (.presetUnsupported(presetIds: [17, 91]), .presetUnsupported(presetIDs: [17, 91])),
        ]
        for (raw, expected) in cases {
            #expect(CoreBridge.SchedOpRefusal(raw) == expected)
            #expect(BridgeError(MeguriCoreBindings.CoreError.SchedOpRefused(refusal: raw)) == .schedOpRefused(expected))
        }
    }
    @Test func operationsPreserveSelectionOrderAndExactText() {
        let op = CoreBridge.SchedOperation.setDue(cards: [9, -1, 2], days: "5-2!", context: .browser)
        guard case .setDue(let cards, let days, let context) = op.raw else { Issue.record("Wrong operation"); return }
        #expect(cards == [9, -1, 2]); #expect(days == "5-2!")
        guard case .some(.browser) = context else { Issue.record("Missing browser context"); return }
        guard case .bury(let targets) = CoreBridge.SchedOperation.bury(.notes([8, 3])).raw,
              case .notes(let notes) = targets else { Issue.record("Lost note targets"); return }
        #expect(notes == [8, 3])
        let mutations: [CoreBridge.SchedOperation] = [
            .suspend(.cards([2, 1])), .unsuspend(cards: [2, 1]), .unburyDeck(deck: 1, mode: .siblings),
            .reset(cards: [2, 1], restorePosition: true, resetCounts: false, context: .reviewer),
            .gradeNow(cards: [2, 1], rating: .good), .emptyFiltered(deck: 42),
        ]
        for mutation in mutations { _ = mutation.raw }
    }
    @Test func resultKeepsCompleteHeadDiffAndOperationMetadata() {
        let raw = MeguriCoreBindings.SchedMutation(
            summary: .init(seq: 17, generation: 3, undoName: "Bury",
                           changes: .init(wholeCollection: false, source: .userUndoable, cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [], revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false), pastCancelPoint: false),
            count: 4, headDiff: ReviewSessionBridgeTests.diff)
        let result = CoreBridge.SchedMutation(raw)
        #expect(result.summary.seq == 17 && result.summary.undoName == "Bury" && result.count == 4)
        #expect(result.headDiff == CoreBridge.ReviewHeadDiff(ReviewSessionBridgeTests.diff))
    }
    @Test func parserUsesTypedProductAPIThroughLane() async throws {
        let service = CoreSchedOpsService()
        #expect(try await service.parseDueDays("5-2!") == CoreBridge.DueDays(minimum: 2, maximum: 5, resetInterval: true))
        await #expect(throws: BridgeError.schedOpRefused(.invalidDays(.malformed))) { try await service.parseDueDays("3--4") }
        await #expect(throws: BridgeError.schedOpRefused(.invalidDays(.tooLarge))) { try await service.parseDueDays("4294967296") }
    }
}
