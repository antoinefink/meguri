@testable import AppModel
@testable import CoreBridge
import Foundation
import HeatmapKit
import Testing

/// The activity heatmap's cache (contract N14.11 rules 1 and 5): revisits hit the
/// cache, each change kind marks the right scopes stale, a width change asks nothing,
/// and a cancelled or superseded read is dropped.
@MainActor
@Suite struct ActivityHeatmapStoreTests {
    /// Counts the reads; a held read waits until the test releases it.
    nonisolated final class FakeService: ActivityService, @unchecked Sendable {
        private let lock = NSLock()
        private var _calls: [Int64?] = []
        private var _historyCalls: [Int64?] = []
        var history: [Int: UInt32]?
        var deckHistory: [Int64: [Int: UInt32]] = [:]
        var historyCalls: [Int64?] { lock.withLock { _historyCalls } }
        private var holds: [CheckedContinuation<Void, Never>] = []
        private var holding = false
        var generation: UInt64 = 1
        var value: UInt32 = 1
        var fails = false

        var calls: [Int64?] { lock.withLock { _calls } }
        func hold() { lock.withLock { holding = true } }
        func release() {
            let waiting = lock.withLock { () -> [CheckedContinuation<Void, Never>] in
                holding = false
                defer { holds.removeAll() }
                return holds
            }
            waiting.forEach { $0.resume() }
        }
        var held: Int { lock.withLock { holds.count } }

        func heatmapHistory(deckID: Int64?) async throws(BridgeError) -> ActivityDays {
            let history = lock.withLock {
                _historyCalls.append(deckID)
                return deckID.flatMap { deckHistory[$0] } ?? self.history
            }
            let bounded = try await heatmap(deckID: deckID)
            return ActivityDays(generation: bounded.generation, schedulerDay: bounded.schedulerDay,
                                nextDayAt: bounded.nextDayAt, today: bounded.today, counts: bounded.counts, history: history)
        }

        func heatmap(deckID: Int64?) async throws(BridgeError) -> ActivityDays {
            let (wait, generation, value, fails) = lock.withLock { () -> (Bool, UInt64, UInt32, Bool) in
                _calls.append(deckID)
                return (holding, self.generation, self.value, self.fails)
            }
            if wait {
                await withCheckedContinuation { continuation in
                    lock.withLock { holds.append(continuation) }
                }
            }
            if fails { throw .unavailable(reason: "exclusive phase") }
            return ActivityDays(generation: generation, schedulerDay: 10, nextDayAt: 0,
                today: .init(year: 2026, month: 10, day: 12, weekday: 2),
                counts: [UInt32](repeating: value, count: 371))
        }
    }

    private static func changes(cards: [ChangeSet.Card] = [], decks: [ChangeSet.Row] = [], keys: [String] = [],
                                added: [Int64] = [], removed: [Int64] = [], whole: Bool = false,
                                source: OpSource = .userUndoable) -> ChangeSet {
        ChangeSet(wholeCollection: whole, source: source, cards: cards, notes: [], notetypes: [], decks: decks, deckConfigs: [],
            configKeys: keys.map { .init(name: $0, change: .updated) }, tags: [], revlogAdded: added, revlogRemoved: removed,
            mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [],
            collectionRow: false)
    }

    private func loaded(_ store: ActivityHeatmapStore, _ scopes: ActivityScope...) async {
        for scope in scopes { await store.load(store.request(scope)) }
    }

    @Test func aRevisitIsServedFromTheCacheWithoutAQuery() async {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        #expect(store.days(.deck(5)) == nil)
        await loaded(store, .collection, .deck(5))
        #expect(service.calls == [nil, 5])
        // Leaving and returning: same key, nothing asked.
        await loaded(store, .deck(5), .collection, .deck(5))
        #expect(service.calls == [nil, 5])
        #expect(store.days(.deck(5))?.counts.first == 1)
    }

    @Test func aWidthChangeOnlyRecomputesTheGrid() async throws {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        await loaded(store, .collection)
        let request = store.request(.collection)
        let days = try #require(store.days(.collection))
        // Rotation, window resizes and folds: other week counts from the same counts.
        let weeks = [CGFloat(322), 400, 700, 1_100].map {
            let geometry = ActivityGrid.Geometry(width: $0, square: .touch)
            return ActivityGrid(counts: days.counts, weeks: geometry.weeks, todayWeekday: days.today.weekday, firstWeekday: 1).weeks
        }
        #expect(weeks == [17, 21, 37, 53])
        #expect(store.request(.collection) == request, "the load key has no width in it")
        await store.load(store.request(.collection))
        #expect(service.calls.count == 1)
    }

    #if os(macOS)
    @Test func sparseOlderCountsSurviveScopedCachingAndResizeWithoutAnotherRead() async throws {
        let service = FakeService()
        service.value = 0
        service.history = [371: 3, 500: 5, Int.max / 100: 1]
        service.deckHistory = [5: [371: 2], 6: [:]]
        let store = ActivityHeatmapStore(service: service)
        await loaded(store, .collection, .deck(5), .deck(6))
        #expect(service.historyCalls == [nil, 5, 6])
        for (scope, totals) in [(ActivityScope.collection, [0, 0, 3, 8, 0]), (.deck(5), [0, 0, 2, 2, 0]),
                                (.deck(6), [0, 0, 0, 0, 0])] {
            let days = try #require(store.days(scope))
            let history = try #require(days.history)
            let request = store.request(scope)
            let grids = [CGFloat(634), 645.99, 646, 1_000, 634].map {
                let geometry = ActivityGrid.Geometry(width: $0, square: .mac, maximumWeeks: nil)
                return ActivityGrid(history: history, weeks: geometry.weeks,
                                    todayWeekday: days.today.weekday, firstWeekday: 1)
            }
            #expect(grids.map(\.total) == totals)
            #expect(store.request(scope) == request)
            await store.load(request)
        }
        #expect(service.historyCalls == [nil, 5, 6], "resize and revisits slice the cached scoped result")
        store.changesCommitted(Self.changes(cards: [.init(id: 1, noteID: 1, change: .updated, memoryStateOnly: false)]))
        service.deckHistory[5] = [500: 4]
        await loaded(store, .collection, .deck(5))
        #expect(service.historyCalls == [nil, 5, 6, 5])
        #expect(store.days(.deck(5))?.history == [500: 4])
        #expect(store.days(.collection)?.history == service.history)
    }
    #endif

    @Test func reviewsAddedOrRemovedRefreshEveryScope() async {
        for change in [Self.changes(added: [1]), Self.changes(removed: [1]), Self.changes(whole: true),
                       Self.changes(added: [2], source: .syncIncoming), Self.changes(added: [3], source: .syncIncoming)] {
            let service = FakeService()
            let store = ActivityHeatmapStore(service: service)
            await loaded(store, .collection, .deck(5))
            service.value = 2
            store.changesCommitted(change)
            await loaded(store, .collection, .deck(5))
            #expect(service.calls == [nil, 5, nil, 5])
            // The stale value stays until the new one replaces it: no blank in between.
            #expect(store.days(.collection)?.counts.first == 2)
        }
    }

    @Test func cardAndDeckChangesRefreshOnlyDeckScopes() async {
        let moved = ChangeSet.Card(id: 1, noteID: 1, change: .updated, memoryStateOnly: false)
        let memory = ChangeSet.Card(id: 1, noteID: 1, change: .updated, memoryStateOnly: true)
        for (change, refreshes) in [(Self.changes(cards: [moved]), true),
                                    (Self.changes(decks: [.init(id: 5, change: .updated)]), true),
                                    (Self.changes(cards: [memory]), false),
                                    (Self.changes(keys: ["curDeck"]), false)] {
            let service = FakeService()
            let store = ActivityHeatmapStore(service: service)
            await loaded(store, .collection, .deck(5))
            store.changesCommitted(change)
            await loaded(store, .collection, .deck(5))
            #expect(service.calls == (refreshes ? [nil, 5, 5] : [nil, 5]))
        }
    }

    @Test func aNewDayARolloverKeyChangeAndASessionEndRefresh() async {
        for trigger in [{ (store: ActivityHeatmapStore) in store.dayChanged() },
                        { $0.changesCommitted(Self.changes(keys: ["rollover"])) },
                        { $0.sessionEnded() }] {
            let service = FakeService()
            let store = ActivityHeatmapStore(service: service)
            await loaded(store, .collection, .deck(5))
            trigger(store)
            await loaded(store, .collection, .deck(5))
            #expect(service.calls == [nil, 5, nil, 5])
        }
    }

    @Test func aReplacedCollectionDropsEverythingAndRejectsOldResults() async {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        store.generationChanged(to: 1)
        await loaded(store, .collection)
        store.generationChanged(to: 2)
        #expect(store.days(.collection) == nil)
        // A result read from the old file (generation 1) is not kept.
        await loaded(store, .collection)
        #expect(store.days(.collection) == nil)
        service.generation = 2
        await loaded(store, .collection)
        #expect(store.days(.collection)?.generation == 2)
        // The same generation again (a repeated ready event) keeps the cache.
        store.generationChanged(to: 2)
        #expect(store.days(.collection) != nil)
        store.generationChanged(to: nil)
        #expect(store.days(.collection) == nil)
    }

    @Test func leavingThePageDropsItsResult() async {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        service.hold()
        let task = Task { await store.load(store.request(.deck(5))) }
        while service.held == 0 { await Task.yield() }
        task.cancel()
        service.release()
        await task.value
        #expect(store.days(.deck(5)) == nil)
    }

    @Test func aResultForASupersededKeyIsDropped() async {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        service.hold()
        let first = Task { await store.load(store.request(.collection)) }
        while service.held == 0 { await Task.yield() }
        // An answer lands while the read is out: its result is already old.
        store.changesCommitted(Self.changes(added: [1]))
        service.release()
        await first.value
        #expect(store.days(.collection) == nil)
        await loaded(store, .collection)
        #expect(store.days(.collection) != nil)
        #expect(service.calls == [nil, nil])
    }

    @Test func aFailedReadIsAskedAgainWhenTheCollectionIsReady() async {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        service.fails = true
        let failed = store.request(.collection)
        await store.load(failed)
        #expect(store.days(.collection) == nil)
        service.fails = false
        store.collectionReady()
        #expect(store.request(.collection) != failed, "the page's task runs again")
        await loaded(store, .collection)
        #expect(store.days(.collection) != nil)
        // Ready again with nothing failed: nothing to ask.
        let current = store.request(.collection)
        store.collectionReady()
        #expect(store.request(.collection) == current)
    }

    @Test func aReadyCollectionDuringAFailingReadAsksAgainAtOnce() async {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        service.fails = true
        service.hold()
        let first = store.request(.collection)
        let task = Task { await store.load(first) }
        while service.held == 0 { await Task.yield() }
        // Ready arrives while the read is out, before it fails: no later signal will come.
        store.collectionReady()
        service.release()
        await task.value
        #expect(store.request(.collection) != first, "the page's task runs again")
        service.fails = false
        await loaded(store, .collection)
        #expect(store.days(.collection) != nil)
    }

    @Test func onlyAFewScopesAreKept() async {
        let service = FakeService()
        let store = ActivityHeatmapStore(service: service)
        await loaded(store, .collection)
        for deck in 1...Int64(ActivityHeatmapStore.capacity) { await loaded(store, .deck(deck)) }
        #expect(store.days(.collection) == nil, "the least recently used scope goes")
        #expect(store.days(.deck(1)) != nil)
    }
}
