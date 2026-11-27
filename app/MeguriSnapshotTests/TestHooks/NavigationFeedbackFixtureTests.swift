#if MEGURI_TEST_HOOKS
import AppModel
import CoreBridge
import Testing

@testable import Meguri

@MainActor
@Suite struct NavigationFeedbackFixtureTests {
    @Test func selectionStaysHeldUntilFixtureDestructionCancelsIt() async throws {
        var fixture: TestHooks.NavigationFeedbackFixture? = .init(configuration: LaunchConfiguration(), preparedHome: true)
        weak let owner = fixture
        let navigation = try #require(fixture?.navigation)
        let gate = try #require(fixture?.deck)
        let collection = try #require(fixture?.collection)
        let gates = try #require(fixture.map { [$0.home, $0.deck, $0.browse, $0.list, $0.record, $0.stats] })
        let deck = DeckID(1_700_000_000_101)
        navigation.select(.deck(deck))
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(2))
        while gate.calls == 0, clock.now < deadline { try await Task.sleep(for: .milliseconds(1)) }
        #expect(gate.calls == 1)
        #expect(gate.held)
        #expect(!gate.cancelled)
        #expect(navigation.pendingDeckPage == deck)
        #expect(navigation.selectedDeck == nil)

        // Even a retained collection/task must not retain the fixture owner, and
        // destroying that owner must abort the save rather than commit the deck.
        fixture = nil
        #expect(owner == nil)
        #expect(gates.allSatisfy { $0.cancelled })
        #expect(gate.held)
        await collection.deckPageOpened()
        #expect(navigation.selectedDeck == nil)
    }

    @Test func explicitReleaseStillSucceedsButTeardownIsTerminal() async throws {
        let gate = TestHooks.NavigationReadGate()
        let read = Task { try await gate.wait() }
        let clock = ContinuousClock()
        let deadline = clock.now.advanced(by: .seconds(2))
        while gate.calls == 0, clock.now < deadline { try await Task.sleep(for: .milliseconds(1)) }
        #expect(gate.calls == 1)
        #expect(gate.held)
        gate.release()
        try await read.value
        #expect(!gate.held)
        gate.hold()
        gate.cancel()
        gate.release()
        do {
            try await gate.wait()
            Issue.record("A torn-down fixture must never start another successful read")
        } catch {
            guard case BridgeError.cancelled = error else {
                Issue.record("Teardown should report cancellation: \(error)")
                return
            }
        }
    }
}
#endif
