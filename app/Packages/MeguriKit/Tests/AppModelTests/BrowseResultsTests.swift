@testable import AppModel
@testable import CoreBridge
import Foundation
import Synchronization
import Testing

/// Browse's results table (U270a): paging around the rows on screen and
/// supersession by seq (`browser/result-sets` H3, H4, H7), resort and
/// toggle without a new search (H8, H11), the selection mapped to a new
/// list and kept by Cards / Notes (H12, `browser/workspace` V2.1), change
/// sets (H10), row errors (C3.1), and what the status line counts (V11),
/// against a scripted session.
@MainActor @Suite struct BrowseResultsTests {
    private func results(_ session: FakeSearchSession, backend: FakeBrowseBackend = FakeBrowseBackend()) -> BrowseResults {
        backend.searchSession = session
        let results = BrowseResults(backend: backend)
        results.progressDelay = .seconds(60)
        return results
    }

    private static let deckQuery = FilterQuery(tokens: [.value([.state(.suspended)], negated: false)])

    // MARK: Paging and supersession

    @Test func theFirstPageShowsOnlyOnceItsRowsAreReadAndOnlyTheRowsNearTheScreenAreRead() async throws {
        let session = FakeSearchSession()
        session.results[""] = Array(1...150_000)
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        let list = try #require(results.list)
        #expect(list.count == 150_000 && list.isComplete && list.columns == FakeBrowseBackend.standard.cards)
        // The first page came with the list, then the page after it; nothing else was read.
        #expect(results.row(at: 0)?.id == 1 && results.row(at: 199)?.id == 200)
        try await until { session.rowCalls.count == 2 }
        #expect(session.rowCalls.map(\.range) == [0..<200, 200..<400])
        // A row far down appears: its page and the pages beside it, nothing in between.
        results.rowAppeared(100_010)
        try await until { results.row(at: 100_010) != nil && session.rowCalls.count == 5 }
        #expect(Set(session.rowCalls.dropFirst(2).map(\.range)) == [100_000..<100_200, 100_200..<100_400, 99_800..<100_000])
        #expect(session.rowCalls.allSatisfy { $0.range.count <= 1_000 })
        #expect(results.row(at: 50_000) == nil)
        #expect(results.content == .table)
    }

    @Test func aNewerQuerySupersedesAnOlderOneAndItsLateEventsAreDropped() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        session.results["run:1"] = [2]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list?.count == 3 }
        let shown = try #require(results.list)
        // The second search answers only when told to: the first list stays meanwhile.
        session.holdEvents = true
        results.run(Self.deckQuery, mode: .cards)
        try await until { session.starts.count == 2 }
        #expect(results.list == shown && results.isSearching)
        // An event of the first request, late, changes nothing.
        session.emit(.complete(seq: 1, handle: shown.handle, count: 99, revision: 0))
        try await Task.sleep(for: .milliseconds(20))
        #expect(results.list?.count == 3)
        session.releaseEvents()
        try await until { results.list?.count == 1 }
        #expect(results.row(at: 0)?.id == 2 && results.list?.token != shown.token && !results.isSearching)
    }

    @Test func eventsThatComeBeforeTheirStartReturnsAreKept() async throws {
        let session = FakeSearchSession()
        session.results[""] = [5, 6]
        session.eventsBeforeReturn = true
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list?.count == 2 }
        #expect(results.row(at: 1)?.id == 6)
    }

    @Test func aStreamingSearchGrowsInPlaceAndIsSearchingUntilItIsWhole() async throws {
        let session = FakeSearchSession()
        session.results[""] = Array(1...500)
        session.streams = true
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        #expect(results.list?.count == 200 && results.list?.isComplete == false && results.isSearching)
        let token = results.list?.token
        session.emit(.progress(seq: 1, matched: 350, scanned: 600, revision: 0))
        try await until { results.list?.count == 350 }
        session.emit(.complete(seq: 1, handle: BrowseHandle(rawValue: 1), count: 500, revision: 0))
        try await until { results.list?.isComplete == true }
        #expect(results.list?.count == 500 && results.list?.token == token && !results.isSearching)
    }

    @Test func searchingShowsOnlyAfterTheDelay() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1]
        session.holdEvents = true
        let backend = FakeBrowseBackend()
        backend.searchSession = session
        let results = BrowseResults(backend: backend)
        results.progressDelay = .milliseconds(50)
        results.run(FilterQuery(), mode: .cards)
        try await until { session.starts.count == 1 }
        #expect(!results.showsProgress)
        try await until { results.showsProgress }
        session.releaseEvents()
        try await until { results.list != nil }
        #expect(!results.showsProgress)
    }

    @Test func aSearchTheTextCannotRunKeepsTheRowsAndAStorageFailureShowsTryAgain() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list?.count == 2 }
        session.failNext = .browse(.search(BrowseSearchError(kind: "search_failed", provided: nil, context: nil,
                                                              message: "x", utf16Range: 0..<1)))
        results.run(Self.deckQuery, mode: .cards)
        try await until { session.starts.count == 2 }
        try await Task.sleep(for: .milliseconds(20))
        #expect(results.list?.count == 2 && results.content == .table)
        // Another failure keeps the rows read, dimmed (H12, R1108).
        session.failNext = .unavailable(reason: "sample")
        results.run(Self.deckQuery, mode: .cards)
        try await until { results.isDimmed }
        #expect(results.list?.count == 2 && results.content == .table)
        session.results["run:1"] = [1]
        results.retry()
        try await until { results.list?.count == 1 }
        #expect(!results.isDimmed)
    }

    @Test func aFailureAfterAnEmptyResultShowsTryAgain() async throws {
        let session = FakeSearchSession()
        session.results[""] = []
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.content == .emptyCollection }
        session.failNext = .unavailable(reason: "sample")
        results.run(Self.deckQuery, mode: .cards)
        try await until { results.content == .failed }
    }

    @Test func aFirstPageThatCannotBeReadAfterAnEmptyResultShowsTryAgain() async throws {
        let session = FakeSearchSession()
        session.results[""] = []
        session.results["run:1"] = [1, 2]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.content == .emptyCollection }
        session.rowsFail = true
        results.run(Self.deckQuery, mode: .cards)
        try await until { results.content == .failed }
    }

    @Test func aResortWhoseHandleWentShowsTheSearchItRunsInstead() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        session.forgetsHandles = true
        results.sort(by: "cardDue")
        try await until { results.list?.sort.key == "cardDue" }
        #expect(session.starts.count == 2 && results.row(at: 0)?.id == 3)
    }

    @Test func aHeaderClickWhileTheRunTextIsReadWritesTheNewQuerysSet() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2]
        session.results["run:1"] = [1]
        let backend = FakeBrowseBackend()
        backend.ownSets[7] = ColumnSetShown(set: .notetype(7), own: true, cards: ["noteFld", "cardDue"], notes: ["noteFld"],
                                            cardsSort: BrowseSort(key: "noteFld", direction: .ascending),
                                            notesSort: BrowseSort(key: "noteFld", direction: .ascending))
        let results = results(session, backend: backend)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        backend.runTextDelay = .milliseconds(150)
        results.run(FilterQuery(tokens: [.value([.notetype(id: 7, name: "Vocabulary")], negated: false)]), mode: .cards)
        results.sort(by: "cardDue")
        try await until { results.list?.sort.key == "cardDue" }
        try await until { backend.columnChangeSets.count == 1 }
        #expect(backend.columnChangeSets == [.notetype(7)] && results.list?.columns == ["noteFld", "cardDue"])
    }

    @Test func aMappingTakenBeforeRowsWereChosenNeverReturnsAtCompletion() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        session.results["run:1"] = [1, 2, 3]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.row(at: 2) != nil }
        results.select([2])
        session.streams = true
        session.rowsDelay = .milliseconds(150)
        results.run(Self.deckQuery, mode: .cards)
        try await until { session.rowCalls.count >= 2 }
        // Rows chosen while the new list's first rows are read.
        results.select([0])
        try await until { results.list?.token == 2 }
        #expect(results.selection.isEmpty)
        session.rowsDelay = nil
        session.emit(.complete(seq: 2, handle: BrowseHandle(rawValue: 2), count: 3, revision: 0))
        try await until { results.list?.isComplete == true }
        try await Task.sleep(for: .milliseconds(50))
        #expect(results.selection.isEmpty)
    }

    @Test func aQuestionCellReadThatKeepsFailingIsTriedOnlyTwiceMore() async throws {
        let session = FakeSearchSession()
        session.results["run:1"] = [1, 2, 3]
        session.qaFail = true
        let backend = FakeBrowseBackend()
        backend.ownSets[7] = ColumnSetShown(set: .notetype(7), own: true, cards: ["noteFld", "question"], notes: ["noteFld"],
                                            cardsSort: BrowseSort(key: "noteFld", direction: .ascending),
                                            notesSort: BrowseSort(key: "noteFld", direction: .ascending))
        let results = results(session, backend: backend)
        results.run(FilterQuery(tokens: [.value([.notetype(id: 7, name: "Vocabulary")], negated: false)]), mode: .cards)
        try await until { results.list != nil }
        try await Task.sleep(for: .seconds(1))
        #expect(session.qaCalls == 3)
    }

    @Test func aFailureWithNoRowsShowsTryAgain() async throws {
        let session = FakeSearchSession()
        session.results[""] = [4]
        session.failNext = .unavailable(reason: "sample")
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.content == .failed }
        results.retry()
        try await until { results.content == .table && results.list?.count == 1 }
    }

    @Test func theEventsOfAStartStillOnItsWayNeverGoToTheRequestQueuedBehindIt() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        session.results["run:1"] = [9]
        session.eventsBeforeReturn = true
        session.startDelay = .milliseconds(150)
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { session.starts.count == 1 }
        // The first start has sent its events and not returned; the second query waits behind it.
        results.run(Self.deckQuery, mode: .cards)
        try await until { session.starts.count == 2 }
        try await until { results.list?.count == 1 }
        #expect(results.row(at: 0)?.id == 9 && results.list?.handle == BrowseHandle(rawValue: 2))
    }

    @Test func cardsNotesAndAHeaderClickWhileASearchIsPendingApplyToThatSearch() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        session.results["run:1"] = [2, 3]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list?.count == 3 }
        // A new query, not shown yet, then Notes: the new query runs in Notes; nothing toggles the old list.
        session.holdEvents = true
        results.run(Self.deckQuery, mode: .cards)
        try await until { session.starts.count == 2 }
        results.setMode(.notes, query: Self.deckQuery)
        try await until { session.starts.count == 3 }
        #expect(session.toggles.isEmpty && session.starts.last?.text == "run:1" && session.starts.last?.mode == .notes)
        // A header click before it shows: the same search again, sorted.
        results.sort(by: "noteCrt")
        try await until { session.starts.count == 4 }
        #expect(session.resorts.isEmpty && session.starts.last?.mode == .notes
            && session.starts.last?.sort == BrowseSort(key: "noteCrt", direction: .descending))
        session.releaseEvents()
        try await until { results.list?.mode == .notes }
        #expect(results.list?.sort.key == "noteCrt" && results.list?.count == 2)
    }

    @Test func rowsChosenWhileASearchRunsAreTheOnesMapped() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        session.results["run:1"] = [1, 2, 3]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.row(at: 2) != nil }
        results.select([0])
        session.holdEvents = true
        results.run(Self.deckQuery, mode: .cards)
        try await until { session.starts.count == 2 }
        results.select([2])
        session.releaseEvents()
        try await until { results.list?.token == 2 }
        #expect(results.selection == [2] && results.currentIndex == 2)
    }

    @Test func emptyResultsAreNoResultsOrAnEmptyCollection() async throws {
        let session = FakeSearchSession()
        session.results[""] = []
        session.results["run:1"] = []
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        #expect(results.content == .emptyCollection)
        results.run(Self.deckQuery, mode: .cards)
        try await until { results.list?.token == 2 }
        #expect(results.content == .noResults)
    }

    // MARK: Sorting and modes

    @Test func aHeaderClickResortsTheShownHandleInItsFirstDirectionThenReverses() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        let backend = FakeBrowseBackend()
        let results = results(session, backend: backend)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        #expect(results.list?.sort == BrowseSort(key: "noteFld", direction: .ascending))
        // Due's first direction is descending (S1); the shown handle is sorted, no new search.
        results.sort(by: "cardDue")
        try await until { results.list?.sort.key == "cardDue" }
        #expect(results.list?.sort.direction == .descending && session.starts.count == 1 && session.resorts.count == 1)
        #expect(results.row(at: 0)?.id == 3)
        // The set keeps the order shown, written in click order (V4.3).
        try await until { backend.columnChanges.count == 1 }
        #expect(backend.columnChanges == [.setSort(BrowseSort(key: "cardDue", direction: .descending))])
        // Again: reversed.
        results.sort(by: "cardDue")
        try await until { results.list?.sort.direction == .ascending }
        #expect(results.row(at: 0)?.id == 1 && session.resorts.count == 2)
        try await until { backend.columnChanges.count == 2 }
        #expect(backend.columnChanges.last == .setSort(BrowseSort(key: "cardDue", direction: .ascending)))
        // A column that cannot sort does nothing.
        results.sort(by: "question")
        try await Task.sleep(for: .milliseconds(20))
        #expect(session.resorts.count == 2)
    }

    @Test func aSortOnAStreamingHandleSearchesAgainWithTheNewSort() async throws {
        let session = FakeSearchSession()
        session.results[""] = Array(1...300)
        session.streams = true
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        session.streams = false
        results.sort(by: "deck")
        try await until { results.list?.sort.key == "deck" }
        #expect(session.resorts.isEmpty && session.starts.count == 2 && session.starts.last?.sort.key == "deck")
    }

    @Test func cardsNotesTogglesTheHandleAndTheSelectionFollows() async throws {
        let session = FakeSearchSession()
        session.results[""] = [10, 11, 12]
        session.notesOf = [10: 100, 11: 100, 12: 101]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        try await until { results.row(at: 2) != nil }
        results.select([2])
        results.setMode(.notes, query: FilterQuery())
        try await until { results.list?.mode == .notes }
        #expect(session.toggles.count == 1 && session.starts.count == 1)
        #expect(results.list?.columns == FakeBrowseBackend.standard.notes && results.list?.sort == FakeBrowseBackend.standard.notesSort)
        // Card 12's note is 101, the second note.
        #expect(results.selection == [1] && results.currentIndex == 1)
        results.setMode(.cards, query: FilterQuery())
        try await until { results.list?.mode == .cards }
        // Note 101's cards: card 12 alone.
        #expect(results.selection == [2] && results.currentIndex == 2)
    }

    @Test func aNewQueryMapsTheSelectionAndSelectsTheFirstRowWhenNothingMaps() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3]
        session.results["run:1"] = [2, 3]
        session.results["run:2"] = [7]
        let results = results(session)
        // No selection to map: nothing is selected.
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        #expect(results.selection.isEmpty && results.currentIndex == nil)
        results.select([2])
        results.run(Self.deckQuery, mode: .cards)
        try await until { results.list?.count == 2 }
        #expect(results.selection == [1] && results.currentIndex == 1)
        // The selected card is not found: the first row is selected and current (H12).
        results.run(FilterQuery(tokens: [.value([.state(.new)], negated: false), .advanced("x")]), mode: .cards)
        try await until { results.list?.count == 1 }
        #expect(results.selection == [0] && results.currentIndex == 0)
    }

    @Test func choosingRowsMakesTheAddedRowCurrent() async throws {
        let results = BrowseResults(backend: FakeBrowseBackend())
        results.select([3])
        #expect(results.currentIndex == 3)
        results.select([3, 5])
        #expect(results.currentIndex == 5)
        results.select([1, 3, 5, 7])
        #expect(results.currentIndex == 5)
        results.select([1, 3])
        #expect(results.currentIndex == 1)
        results.select([])
        #expect(results.currentIndex == nil)
    }

    // MARK: Change sets

    @Test func aChangeSetRemovesRowsAndShiftsTheSelectionAndReadsTouchedRowsAgain() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3, 4]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.row(at: 3) != nil }
        results.select([3])
        session.invalidation = BrowseInvalidation(removed: [1], refetch: .rows([0]), revision: 1, isComplete: true, count: 3)
        session.results[""] = [1, 3, 4]
        results.changesCommitted(Self.changes(removing: 2), seq: 9)
        try await until { results.list?.revision == 1 }
        #expect(results.list?.count == 3 && results.selection == [2] && results.currentIndex == 2)
        #expect(session.invalidations.first?.changes.map(\.seq) == [9])
        // Row 0 is read again in the new revision (the old row shows until then,
        // so its presence alone says nothing); the moved rows kept their place.
        try await until { session.rowCalls.last?.revision == 1 && results.row(at: 0) != nil }
        #expect(results.row(at: 1)?.id == 3 && results.row(at: 2)?.id == 4)
    }

    @Test func aRemovalKeepsOnlyPagesWhoseRowsAreAllKnown() async throws {
        let session = FakeSearchSession()
        session.results[""] = Array(1...450)
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.row(at: 399) != nil }
        // Page 2 (400 to 449) was never read: after a removal in page 0, page 1's last row would come from it.
        session.invalidation = BrowseInvalidation(removed: [5], refetch: .rows([]), revision: 1, isComplete: true, count: 449)
        results.changesCommitted(Self.changes(removing: 6), seq: 3)
        try await until { results.list?.revision == 1 }
        #expect(results.row(at: 5)?.id == 7)
        try await until { results.row(at: 399) != nil }
        #expect(results.row(at: 399)?.id == 401 && session.rowCalls.last?.revision == 1)
    }

    @Test func changesBeforeTheFirstPageArePassedWithTheFirstCall() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2]
        session.holdEvents = true
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { session.starts.count == 1 }
        results.changesCommitted(Self.changes(removing: 9), seq: 4)
        session.invalidation = BrowseInvalidation(removed: [], refetch: .rows([]), revision: 0, isComplete: true, count: 2)
        session.releaseEvents()
        try await until { session.invalidations.count == 1 }
        #expect(session.invalidations[0].changes.map(\.seq) == [4])
    }

    @Test func aDeletedDamagedOrUnreadableRowStaysAsItsError() async throws {
        let session = FakeSearchSession()
        session.results[""] = [1, 2, 3, 4]
        session.errors = [2: .deleted, 3: .damaged, 4: .unreadable(table: "cards", column: "ivl", row: "4")]
        let results = results(session)
        results.run(FilterQuery(), mode: .cards)
        try await until { results.list != nil }
        #expect(results.row(at: 0)?.status == .row)
        #expect(results.row(at: 1)?.status == .deleted && results.row(at: 2)?.status == .damaged)
        #expect(results.row(at: 3)?.status == .unreadable && results.row(at: 3)?.isDimmed == true)
    }

    // MARK: Status line

    @Test func theStatusLineCountsTheRowsTheTotalAndTheSelection() async throws {
        let session = FakeSearchSession()
        session.results["run:1"] = Array(1...12)
        let backend = FakeBrowseBackend()
        backend.searchSession = session
        let model = BrowseModel(backend: backend)
        model.results.progressDelay = .seconds(60)
        model.appear()
        try await until { model.results.total != nil }
        // The counts' last item is the whole collection.
        #expect(backend.countCalls.last?.1.last == .query(FilterQuery()))
        model.activate(.state(.suspended))
        try await until { model.results.list?.count == 12 }
        model.results.select([0, 1, 2])
        let status = BrowseStatus(results: model.results, query: model.query)
        #expect(status == BrowseStatus(shown: 12, total: 1, mode: .cards, wholeCollection: false, searching: false, selected: 3))
    }

    // MARK: Helpers

    struct TimedOut: Error {}

    /// The budget is for a loaded machine running suites side by side: a
    /// condition that holds returns at once, whatever the budget.
    private func until(_ condition: () -> Bool, timeout: Duration = .seconds(30)) async throws {
        let clock = ContinuousClock(), end = clock.now + timeout
        while !condition() {
            guard clock.now < end else {
                Issue.record("timed out")
                throw TimedOut()
            }
            try await Task.sleep(for: .milliseconds(2))
        }
    }

    static func changes(removing card: Int64) -> ChangeSet {
        ChangeSet(wholeCollection: false, source: .userUndoable,
                  cards: [ChangeSet.Card(id: card, noteID: card * 10, change: .removed, memoryStateOnly: false)],
                  notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [], revlogAdded: [],
                  revlogRemoved: [], mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
                  skippedIncomingDecks: [], collectionRow: false)
    }
}

/// A scripted search session: each run text finds `results[text]` (card
/// ids, in ascending order); resorts and toggles derive from the handle.
nonisolated final class FakeSearchSession: BrowseSearching, @unchecked Sendable {
    struct Start: Equatable { let text: String; let mode: BrowseMode; let sort: BrowseSort }
    struct RowCall: Equatable { let handle: UInt64; let revision: UInt64; let range: Range<UInt64> }
    struct Invalidation { let handle: UInt64; let revision: UInt64; let changes: [SequencedChangeSet] }

    private let lock = NSRecursiveLock()
    private func locked<T>(_ body: () -> T) -> T { lock.lock(); defer { lock.unlock() }; return body() }

    let events: AsyncStream<BrowseSearchEvent>
    private let continuation: AsyncStream<BrowseSearchEvent>.Continuation

    init() {
        (events, continuation) = AsyncStream.makeStream(of: BrowseSearchEvent.self)
    }

    // Script.
    var results: [String: [Int64]] { get { locked { _results } } set { locked { _results = newValue } } }
    private var _results: [String: [Int64]] = [:]
    /// Card → note, for Notes mode (default: the card's own id × 10).
    var notesOf: [Int64: Int64] { get { locked { _notesOf } } set { locked { _notesOf = newValue } } }
    private var _notesOf: [Int64: Int64] = [:]
    var errors: [Int64: BrowseRowResult] { get { locked { _errors } } set { locked { _errors = newValue } } }
    private var _errors: [Int64: BrowseRowResult] = [:]
    var streams: Bool { get { locked { _streams } } set { locked { _streams = newValue } } }
    private var _streams = false
    var eventsBeforeReturn: Bool { get { locked { _before } } set { locked { _before = newValue } } }
    private var _before = false
    var holdEvents: Bool { get { locked { _hold } } set { locked { _hold = newValue } } }
    private var _hold = false
    private var held: [BrowseSearchEvent] = []
    var failNext: BridgeError? { get { locked { _failNext } } set { locked { _failNext = newValue } } }
    private var _failNext: BridgeError?
    var invalidation: BrowseInvalidation? { get { locked { _invalidation } } set { locked { _invalidation = newValue } } }
    var rowsFail: Bool { get { locked { _rowsFail } } set { locked { _rowsFail = newValue } } }
    private var _rowsFail = false
    var rowsDelay: Duration? { get { locked { _rowsDelay } } set { locked { _rowsDelay = newValue } } }
    private var _rowsDelay: Duration?
    var qaFail: Bool { get { locked { _qaFail } } set { locked { _qaFail = newValue } } }
    private var _qaFail = false
    private(set) var qaCalls: Int { get { locked { _qaCalls } } set { locked { _qaCalls = newValue } } }
    private var _qaCalls = 0
    /// Resorts and toggles find their handle gone (H6).
    var forgetsHandles: Bool { get { locked { _forgets } } set { locked { _forgets = newValue } } }
    private var _forgets = false
    /// How long a start takes to return after its events went.
    var startDelay: Duration? { get { locked { _startDelay } } set { locked { _startDelay = newValue } } }
    private var _startDelay: Duration?
    private var _invalidation: BrowseInvalidation?

    // Record.
    private(set) var starts: [Start] { get { locked { _starts } } set { locked { _starts = newValue } } }
    private var _starts: [Start] = []
    private(set) var resorts: [BrowseSort] { get { locked { _resorts } } set { locked { _resorts = newValue } } }
    private var _resorts: [BrowseSort] = []
    private(set) var toggles: [BrowseSort] { get { locked { _toggles } } set { locked { _toggles = newValue } } }
    private var _toggles: [BrowseSort] = []
    private(set) var rowCalls: [RowCall] { get { locked { _rowCalls } } set { locked { _rowCalls = newValue } } }
    private var _rowCalls: [RowCall] = []
    private(set) var invalidations: [Invalidation] { get { locked { _invalidations } } set { locked { _invalidations = newValue } } }
    private var _invalidations: [Invalidation] = []

    private var seq: UInt64 = 0
    private var handles: [UInt64: (ids: [Int64], mode: BrowseMode, complete: Bool, text: String)] = [:]

    func emit(_ event: BrowseSearchEvent) { continuation.yield(event) }

    func releaseEvents() {
        let events = locked { () -> [BrowseSearchEvent] in
            _hold = false
            defer { held = [] }
            return held
        }
        events.forEach(emit)
    }

    private func send(_ events: [BrowseSearchEvent]) {
        let now = locked { () -> Bool in
            if _hold { held += events; return false }
            return true
        }
        if now { events.forEach(emit) }
    }

    private func note(_ card: Int64) -> Int64 { _notesOf[card] ?? card * 10 }

    private func ordered(_ ids: [Int64], _ sort: BrowseSort) -> [Int64] {
        sort.direction == .ascending ? ids.sorted() : ids.sorted(by: >)
    }

    private func begin(ids: [Int64], mode: BrowseMode, text: String) -> UInt64 {
        let (seq, events) = locked { () -> (UInt64, [BrowseSearchEvent]) in
            seq += 1
            let handle = BrowseHandle(rawValue: seq)
            let complete = !_streams
            handles[seq] = (ids, mode, complete, text)
            var events: [BrowseSearchEvent] = [.firstPage(seq: seq, handle: handle, ids: Array(ids.prefix(200)))]
            if complete { events.append(.complete(seq: seq, handle: handle, count: UInt64(ids.count), revision: 0)) }
            return (seq, events)
        }
        if eventsBeforeReturn { send(events) } else {
            Task { self.send(events) }
        }
        return seq
    }

    func startSearch(_ text: String, mode: BrowseMode, sort: BrowseSort) async throws -> UInt64 {
        starts.append(Start(text: text, mode: mode, sort: sort))
        if let error = locked({ () -> BridgeError? in defer { _failNext = nil }; return _failNext }) {
            if case .browse(.search) = error {
                let seq = locked { seq += 1; return seq }
                Task { self.send([.failed(seq: seq, error: error)]) }
                return seq
            }
            throw error
        }
        let cards = results[text] ?? []
        let ids = mode == .cards ? cards : Array(Set(locked { cards.map { note($0) } })).sorted()
        let seq = begin(ids: ordered(ids, sort), mode: mode, text: text)
        if let delay = startDelay { try? await Task.sleep(for: delay) }
        return seq
    }

    func resort(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64 {
        resorts.append(sort)
        if forgetsHandles { throw BridgeError.browse(.unknownHandle) }
        guard let old = locked({ handles[handle.rawValue] }) else { throw BridgeError.browse(.unknownHandle) }
        guard old.complete else { throw BridgeError.browse(.notComplete) }
        return begin(ids: ordered(old.ids, sort), mode: old.mode, text: old.text)
    }

    func toggle(_ handle: BrowseHandle, sort: BrowseSort) async throws -> UInt64 {
        toggles.append(sort)
        guard let old = locked({ handles[handle.rawValue] }) else { throw BridgeError.browse(.unknownHandle) }
        guard old.complete else { throw BridgeError.browse(.notComplete) }
        let cards = results[old.text] ?? []
        let mode: BrowseMode = old.mode == .cards ? .notes : .cards
        let ids = mode == .cards ? cards : Array(Set(locked { cards.map { note($0) } })).sorted()
        return begin(ids: ordered(ids, sort), mode: mode, text: old.text)
    }

    func rows(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, columns: [String]) async throws
        -> [BrowseRowResult] {
        rowCalls.append(RowCall(handle: handle.rawValue, revision: revision, range: range))
        if let delay = rowsDelay { try? await Task.sleep(for: delay) }
        if rowsFail { throw BridgeError.unavailable(reason: "sample") }
        return locked {
            guard let entry = handles[handle.rawValue] else { return [] }
            return range.compactMap { index -> BrowseRowResult? in
                guard Int(index) < entry.ids.count else { return nil }
                let id = entry.ids[Int(index)]
                if let error = _errors[id] { return error }
                return .row(BrowseRow(id: id, noteID: entry.mode == .cards ? note(id) : id,
                                      state: BrowseRowState(flag: 0, marked: false, leech: false, suspended: false,
                                                            buried: false, filtered: false),
                                      tint: .none, font: BrowseFont(name: nil, size: nil), sortFieldRightToLeft: false,
                                      cells: columns.map { .text("\($0) \(id)") }))
            }
        }
    }

    func qaCells(_ handle: BrowseHandle, revision: UInt64, range: Range<UInt64>, which: BrowseQAWhich) async throws
        -> [BrowseQAResult] {
        qaCalls += 1
        if qaFail { throw BridgeError.unavailable(reason: "sample") }
        return range.map { _ in .cells(question: .text("q"), answer: .text("a")) }
    }

    func invalidatedRows(_ handle: BrowseHandle, revision: UInt64, changes: [SequencedChangeSet]) async throws
        -> BrowseInvalidation {
        invalidations.append(Invalidation(handle: handle.rawValue, revision: revision, changes: changes))
        let result = invalidation ?? BrowseInvalidation(removed: [], refetch: .rows([]), revision: revision, isComplete: true,
                                                        count: UInt64(locked { handles[handle.rawValue]?.ids.count ?? 0 }))
        locked {
            if !result.removed.isEmpty, var entry = handles[handle.rawValue] {
                let removed = Set(result.removed.map { Int($0) })
                entry.ids = entry.ids.enumerated().filter { !removed.contains($0.offset) }.map(\.element)
                handles[handle.rawValue] = entry
            }
        }
        return result
    }

    // The commands (U275): the counts and actions of a target, over the
    // card states the test gives; an action records what it was given.
    struct Action: Equatable { let card: BulkCardAction?; let note: BulkNoteAction?; let target: BrowseTarget }
    var suspendedCards: Set<Int64> { get { locked { _suspended } } set { locked { _suspended = newValue } } }
    private var _suspended: Set<Int64> = []
    var buriedCards: Set<Int64> { get { locked { _buried } } set { locked { _buried = newValue } } }
    private var _buried: Set<Int64> = []
    /// Note → its cards in the collection (default: the cards the results find).
    var cardsOfNote: [Int64: [Int64]] { get { locked { _cardsOfNote } } set { locked { _cardsOfNote = newValue } } }
    private var _cardsOfNote: [Int64: [Int64]] = [:]
    /// The revision each handle is at now; a target of another is refused (K1).
    var revisions: [UInt64: UInt64] { get { locked { _revisions } } set { locked { _revisions = newValue } } }
    private var _revisions: [UInt64: UInt64] = [:]
    private(set) var actions: [Action] { get { locked { _actions } } set { locked { _actions = newValue } } }
    private var _actions: [Action] = []
    var actionFails = false
    /// The target cards' home decks the counts report (U276).
    var deckIDs: [Int64] { get { locked { _deckIDs } } set { locked { _deckIDs = newValue } } }
    private var _deckIDs: [Int64] = [1]
    /// The tags `tags(of:)` reports for any target (Remove Tags).
    var noteTags: [String] { get { locked { _noteTags } } set { locked { _noteTags = newValue } } }
    private var _noteTags: [String] = []
    /// U276: a started action reports one progress and waits until it is
    /// released or cancelled (then it ends `.cancelled`, nothing recorded).
    var holdsActions = false
    let heldAction = HeldAction()

    private func check(_ target: BrowseTarget) throws -> [Int64] {
        lock.lock(); defer { lock.unlock() }
        guard let entry = handles[target.handle.rawValue] else { throw BridgeError.browse(.unknownHandle) }
        guard (_revisions[target.handle.rawValue] ?? 0) == target.revision else {
            throw BridgeError.invalidArgument("stale selection")
        }
        return target.ranges.flatMap { $0.map { entry.ids[Int($0)] } }
    }

    private func notes(_ ids: [Int64], _ mode: BrowseMode) -> [Int64] {
        locked { mode == .notes ? ids : ids.map { note($0) } }.reduce(into: []) { if !$0.contains($1) { $0.append($1) } }
    }

    private func cards(_ ids: [Int64], _ mode: BrowseMode) -> [Int64] {
        guard mode == .notes else { return ids }
        return locked { ids.flatMap { note in _cardsOfNote[note] ?? _results.values.flatMap { $0 }.filter { self.note($0) == note } } }
    }

    func counts(of target: BrowseTarget) async throws -> BrowseSelectionCounts {
        let ids = try check(target)
        let cards = cards(ids, target.mode)
        return locked {
            BrowseSelectionCounts(cards: UInt64(cards.count), notes: UInt64(notes(ids, target.mode).count),
                                  suspended: UInt64(cards.filter(_suspended.contains).count),
                                  buried: UInt64(cards.filter(_buried.contains).count), decks: UInt64(_deckIDs.count),
                                  deck: _deckIDs.count == 1 ? _deckIDs.first : nil, deckIDs: _deckIDs,
                                  notetypes: _notetypesOfNote.isEmpty ? [8]
                                      : Array(Set(notes(ids, target.mode).compactMap { _notetypesOfNote[$0] })).sorted())
        }
    }

    func cardsOfNotes(of target: BrowseTarget) async throws -> UInt64 {
        let notes = notes(try check(target), target.mode)
        return UInt64(locked { notes.flatMap { note in _cardsOfNote[note] ?? [note] } }.count)
    }

    func siblingRows(of target: BrowseTarget) async throws -> [Range<UInt64>] {
        let notes = Set(notes(try check(target), target.mode))
        return locked {
            let ids = handles[target.handle.rawValue]?.ids ?? []
            return BrowseResults.ranges(Set(ids.indices.filter { notes.contains(note(ids[$0])) }))
        }
    }

    func run(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BulkActionResult {
        let cards = cards(try check(target), target.mode)
        actions.append(Action(card: action, note: nil, target: target))
        if actionFails { throw BridgeError.unavailable(reason: "sample") }
        let affected: Int = locked {
            switch action {
            case .suspend: defer { _suspended.formUnion(cards) }; return cards.filter { !_suspended.contains($0) }.count
            case .unsuspend: defer { _suspended.subtract(cards) }; return cards.filter(_suspended.contains).count
            case .bury: defer { _buried.formUnion(cards) }; return cards.filter { !_buried.contains($0) }.count
            case .unbury: defer { _buried.subtract(cards) }; return cards.filter(_buried.contains).count
            default: return cards.count
            }
        }
        return Self.result(affected: affected, considered: cards.count, undo: affected > 0 ? "Suspend" : nil)
    }

    func tags(of target: BrowseTarget) async throws -> [String] {
        _ = try check(target)
        return noteTags
    }

    // U277: Find and Replace and Find Duplicates over a script. A preview
    // and a run get the target's note ids; each call is recorded.
    struct FindReplaceCall: Equatable { let request: FindReplaceRequest; let notes: [Int64]; let confirmed: UInt64? }
    /// Note → its note type (default: every note is note type 8).
    var notetypesOfNote: [Int64: Int64] { get { locked { _notetypesOfNote } } set { locked { _notetypesOfNote = newValue } } }
    private var _notetypesOfNote: [Int64: Int64] = [:]
    /// The preview of a request on the target's notes (default: no match).
    var previewing: @Sendable (FindReplaceRequest, [Int64]) throws -> FindReplacePreview {
        get { locked { _previewing } } set { locked { _previewing = newValue } }
    }
    private var _previewing: @Sendable (FindReplaceRequest, [Int64]) throws -> FindReplacePreview = { _, notes in
        FindReplacePreview(changed: 0, considered: UInt64(notes.count), entries: [])
    }
    private(set) var previewCalls: [FindReplaceCall] { get { locked { _previewCalls } } set { locked { _previewCalls = newValue } } }
    private var _previewCalls: [FindReplaceCall] = []
    private(set) var replaceCalls: [FindReplaceCall] { get { locked { _replaceCalls } } set { locked { _replaceCalls = newValue } } }
    private var _replaceCalls: [FindReplaceCall] = []
    /// The run's refusal, else it writes the preview's count of notes.
    var replaceError: BridgeError? { get { locked { _replaceError } } set { locked { _replaceError = newValue } } }
    private var _replaceError: BridgeError?
    /// A run waits until `heldAction` is released or cancelled.
    var holdsReplace = false
    var duplicates = FindDuplicatesResult(groups: [], noteIds: [], considered: 0)
    struct DuplicatesCall: Equatable { let field: String; let notes: [Int64]? }
    private(set) var duplicatesCalls: [DuplicatesCall] {
        get { locked { _duplicatesCalls } } set { locked { _duplicatesCalls = newValue } }
    }
    private var _duplicatesCalls: [DuplicatesCall] = []

    func findReplacePreviews() async throws -> any BrowseFindReplacePreviewing { FakePreviews(session: self) }

    nonisolated final class FakePreviews: BrowseFindReplacePreviewing {
        let session: FakeSearchSession
        init(session: FakeSearchSession) { self.session = session }
        func preview(_ request: FindReplaceRequest, on target: BrowseTarget) async throws -> FindReplacePreview {
            try await session.preview(request, on: target)
        }
    }

    func preview(_ request: FindReplaceRequest, on target: BrowseTarget) async throws -> FindReplacePreview {
        let notes = notes(try check(target), target.mode)
        locked { _previewCalls.append(FindReplaceCall(request: request, notes: notes, confirmed: nil)) }
        return try previewing(request, notes)
    }

    func startFindReplace(_ request: FindReplaceRequest, on target: BrowseTarget, confirmed: UInt64) async throws
        -> BrowseRunning<FindReplaceResult> {
        let notes = notes(try check(target), target.mode)
        let run: @Sendable () async throws -> FindReplaceResult = {
            self.locked { self._replaceCalls.append(FindReplaceCall(request: request, notes: notes, confirmed: confirmed)) }
            if let error = self.replaceError { throw error }
            let changed = try self.previewing(request, notes).changed
            let summary = Self.result(affected: 0, considered: 0, undo: changed > 0 ? "Find and Replace" : nil).summary
            return FindReplaceResult(affected: changed, considered: UInt64(notes.count), summary: summary)
        }
        guard holdsReplace else { return .finishing(run) }
        let held = heldAction.running {
            let affected = Int(try await run().affected)
            return Self.result(affected: affected, considered: 0, undo: nil)
        }
        return BrowseRunning(progress: held.progress, cancel: held.cancel, result: {
            let done = try await held.result()
            return FindReplaceResult(affected: done.affected, considered: UInt64(notes.count),
                                     summary: Self.result(affected: 0, considered: 0, undo: "Find and Replace").summary)
        })
    }

    func startFindDuplicates(field: String, on target: BrowseTarget?) async throws -> BrowseRunning<FindDuplicatesResult> {
        let notes = try target.map { notes(try check($0), $0.mode) }
        locked { _duplicatesCalls.append(DuplicatesCall(field: field, notes: notes)) }
        let found = duplicates
        return .finishing { found }
    }

    func start(_ action: BulkCardAction, on target: BrowseTarget) async throws -> BrowseRunningAction {
        guard holdsActions else { return .finishing { try await self.run(action, on: target) } }
        _ = try check(target)
        return heldAction.running { try await self.run(action, on: target) }
    }

    func start(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BrowseRunningAction {
        guard holdsActions else { return .finishing { try await self.run(action, on: target) } }
        _ = try check(target)
        return heldAction.running { try await self.run(action, on: target) }
    }

    func run(_ action: BulkNoteAction, on target: BrowseTarget) async throws -> BulkActionResult {
        let notes = notes(try check(target), target.mode)
        actions.append(Action(card: nil, note: action, target: target))
        if actionFails { throw BridgeError.unavailable(reason: "sample") }
        return Self.result(affected: notes.count, considered: notes.count, undo: "Delete Note")
    }

    static func result(affected: Int, considered: Int, undo: String?) -> BulkActionResult {
        BulkActionResult(affected: UInt64(affected), considered: UInt64(considered), cardsRemoved: nil,
                         summary: MutationSummary(seq: 500, generation: 1, undoName: undo,
                                                  changes: ChangeSet(wholeCollection: false, source: .userUndoable, cards: [], notes: [],
                                                                     notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [],
                                                                     revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [],
                                                                     skippedIncomingCards: [], skippedIncomingNotes: [],
                                                                     skippedIncomingDecks: [], collectionRow: false),
                                                  pastCancelPoint: false))
    }

    func mapSelection(from source: BrowseMapSource, selected: BrowseSelected, current: Int64?, to target: BrowseHandle,
                      targetRevision: UInt64) async throws -> BrowseMappedSelection {
        locked {
            guard let to = handles[target.rawValue] else { return BrowseMappedSelection(selected: [], current: nil) }
            let fromMode: BrowseMode
            var ids: [Int64]
            switch (source, selected) {
            case (.handle(let handle, _), .ranges(let ranges)):
                let from = handles[handle.rawValue]
                fromMode = from?.mode ?? .cards
                ids = ranges.flatMap { $0.compactMap { Int($0) < (from?.ids.count ?? 0) ? from?.ids[Int($0)] : nil } }
            case (.mode(let mode), .ids(let list)):
                fromMode = mode
                ids = list
            default:
                return BrowseMappedSelection(selected: [], current: nil)
            }
            func targets(_ id: Int64) -> [Int64] {
                if fromMode == to.mode { return [id] }
                if to.mode == .notes { return [note(id)] }
                return to.ids.filter { note($0) == id }
            }
            ids = ids.flatMap(targets)
            let indexes = Set(ids.compactMap { id in to.ids.firstIndex(of: id) }).sorted()
            let currentIndex = current.flatMap { targets($0).compactMap { to.ids.firstIndex(of: $0) }.min() }
            return BrowseMappedSelection(selected: BrowseResults.ranges(Set(indexes)),
                                         current: currentIndex.map { UInt64($0) })
        }
    }
}

/// A held action (U276's progress and Cancel): it reports one progress, then
/// waits for `release()` (it runs) or its Cancel (it ends cancelled).
nonisolated final class HeldAction: @unchecked Sendable {
    private let lock = NSLock()
    private var waiting: CheckedContinuation<Bool, Never>?
    private var decided: Bool?
    private(set) var cancelled = false
    private(set) var started = 0

    func running(_ run: @escaping @Sendable () async throws -> BulkActionResult) -> BrowseRunningAction {
        lock.withLock { started += 1; decided = nil }
        let progress = AsyncStream<BrowseActionProgress> { continuation in
            continuation.yield(BrowseActionProgress(completed: 1, total: 4, cancellable: true))
        }
        return BrowseRunningAction(progress: progress, cancel: { self.decide(false) }, result: {
            let proceed = await withCheckedContinuation { continuation in
                self.lock.withLock {
                    if let decided = self.decided { continuation.resume(returning: decided) } else { self.waiting = continuation }
                }
            }
            guard proceed else { throw BridgeError.cancelled }
            return try await run()
        })
    }

    func release() { decide(true) }

    private func decide(_ proceed: Bool) {
        let waiting: CheckedContinuation<Bool, Never>? = lock.withLock {
            if !proceed { cancelled = true }
            decided = proceed
            defer { self.waiting = nil }
            return self.waiting
        }
        waiting?.resume(returning: proceed)
    }
}
