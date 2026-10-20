import Dispatch
import Foundation
import Testing
import MeguriCoreBindings
@testable import CoreBridge

@Suite struct CardExcerptsTests {
    @Test @MainActor func fiftyIDsUseExactlyOneQueryCallOffMain() async throws {
        let ids = (0..<50).map(Int64.init)
        let calls = Counter()
        let rows = try await Core.cardExcerpts(ids, using: { received in
            calls.add(1)
            #expect(received == ids)
            #expect(!Thread.isMainThread)
            #expect(String(cString: __dispatch_queue_get_label(nil)) == Lane.query.queueLabel)
            return received.map { .missing(cardID: $0) }
        })
        #expect(calls.add(0) == 1)
        #expect(rows == ids.map { .missing(cardID: $0) })
    }
    @Test func emptyListStillReachesProviderAndPropagatesRefusal() async {
        let calls = Counter()
        do {
            _ = try await Core.cardExcerpts([], using: { ids in
                calls.add(1)
                #expect(ids.isEmpty)
                throw BridgeError.unavailable(reason: "stale generation")
            })
            Issue.record("Expected the whole call to refuse")
        } catch { #expect(error == .unavailable(reason: "stale generation")) }
        #expect(calls.add(0) == 1)
    }
    @Test func typedCellsSignedExtremesAndDuplicatesMapWithoutLocalization() {
        let raw: [MeguriCoreBindings.CardExcerpt] = [
            .found(cardId: .min, question: .text(text: ""), deck: "Full::Deck"),
            .found(cardId: 4, question: .missingCloze(kind: .missingCloze, text: "kept"), deck: "D"),
            .found(cardId: 4, question: .templateError(kind: .unknownField, side: .answer), deck: "D"),
            .missing(cardId: .max), .damaged(cardId: -7), .missing(cardId: .max),
        ]
        #expect(raw.map(CoreBridge.CardExcerpt.init) == [
            .found(cardID: .min, question: .text(""), deck: "Full::Deck"),
            .found(cardID: 4, question: .missingCloze(kind: .missingCloze, text: "kept"), deck: "D"),
            .found(cardID: 4, question: .templateError(.unknownField, side: .answer), deck: "D"),
            .missing(cardID: .max), .damaged(cardID: -7), .missing(cardID: .max),
        ])
    }
    @Test func cancellationBeforeQuerySkipsProvider() async {
        let calls = Counter()
        let task = Task { () throws(BridgeError) -> [CoreBridge.CardExcerpt] in
            withUnsafeCurrentTask { $0?.cancel() }
            return try await Core.cardExcerpts([1], using: { _ in
                calls.add(1); return []
            })
        }
        do { _ = try await task.value; Issue.record("Expected cancellation") }
        catch { #expect(error as? BridgeError == .cancelled) }
        #expect(calls.add(0) == 0)
    }
}
