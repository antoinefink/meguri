import CoreBridge
import Foundation
import Testing

@testable import Stats

/// The page model against a fake `StatsService` (S20's model tests): the kept state (S3.3),
/// tier filling and per-box phases through progress, failure, cancel and `ContextChanged`
/// (S10, S12.8), kept results shown at once, refreshed in place and never crossing scopes
/// (S10.2), no request for a period change within the year (S3.2), All's two refreshes
/// (S10.3), the subtitle (S10.4), live refresh after half a second (S11) and the selection
/// carried across a refresh.
@MainActor
@Suite struct StatsPageModelTests {
    let service = FakeStatsService()
    let timers = ManualTimers()
    let defaults: UserDefaults
    let kept = StatsKeptResults(directory: nil)

    init() {
        let suite = "stats-model-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
    }

    var tier1: [StatsPart] { StatsPart.allCases.filter { $0.tier == .present } }
    var tier2: [StatsPart] { StatsPart.allCases.filter { $0.tier == .reviewLog } }

    func model(_ selection: StatsPageSelection? = StatsPageSelection(deck: nil, period: .oneMonth),
               kept: StatsKeptResults? = nil) -> StatsPageModel {
        let model = StatsPageModel(service: service, kept: kept ?? self.kept, store: StatsSelectionStore(defaults: defaults),
                                   selection: selection, timers: timers, calendar: Fixture.calendar(), locale: Fixture.locale)
        model.decksChanged(Fixture.nodes())
        model.collectionOpened(identity: "collection-a")
        return model
    }

    /// Waits for request `n` (1-based) and returns it.
    func request(_ n: Int) async throws -> StatsPageRequest {
        await settle({ service.requests.count >= n }, "request \(n) was made")
        return try #require(service.requests.count >= n ? service.requests[n - 1] : nil)
    }

    func complete(_ request: StatsPageRequest, _ model: StatsPageModel, parts: [StatsPart] = StatsPart.allCases,
                  context: StatsContext = Fixture.context()) async {
        service.answer(request, parts, context: context)
        service.send(request.refresh, .end(.complete))
        await settle { model.boxes.values.allSatisfy { $0.phase != .loading(fraction: nil) } && model.subtitle?.contains("Updated") == true }
    }

    // MARK: Kept state (S3.3)

    @Test func theFirstTimeIsAllDecksAndAll() {
        let model = StatsPageModel(service: service, kept: kept, store: StatsSelectionStore(defaults: defaults), timers: timers)
        #expect(model.selection.scope == .allDecks)
        #expect(model.selection.period == .all)
        #expect(model.selection.reviewsMode == .byType && model.selection.retentionKind == .all)
    }

    @Test func theDeviceKeepsTheLastSelectionAndAnInvalidValueReadsAsTheFirstOption() {
        let store = StatsSelectionStore(defaults: defaults)
        let first = model(nil)
        first.selectDeck(1)
        first.selectPeriod(.sixMonths)
        first.setReviewsMeasure(.time)
        #expect(store.load() == StatsPageSelection(deck: 1, period: .sixMonths, reviewsMeasure: .time))
        // A second window starts from the device's last selection.
        #expect(model(nil).selection == store.load())
        defaults.set(#"{"deck":1,"period":"2Y","reviewsMode":"sideways","reviewsMeasure":"time","retentionKind":7}"#,
                     forKey: StatsSelectionStore.key)
        #expect(store.load() == StatsPageSelection(deck: 1, period: .all, reviewsMode: .byType, reviewsMeasure: .time,
                                                   retentionKind: .all))
    }

    @Test func openingFromADeckKeepsThePeriod() {
        let model = model(StatsPageSelection(deck: nil, period: .threeMonths))
        model.selectDeck(3)
        #expect(model.selection.deck == 3 && model.selection.period == .threeMonths)
    }

    // MARK: Tiers and phases (S10.1, S10.3, S12.8)

    @Test func boxesFillByTierWithTheirOwnProgress() async throws {
        let model = model()
        model.appear()
        let request = try await request(1)
        #expect(request.period == .oneMonth && request.parts == StatsPart.allCases && request.scope == .allDecks)
        #expect(model.boxes.values.allSatisfy { $0.phase == .loading(fraction: nil) })
        #expect(model.strip(.studiedToday).loading)

        service.answer(request, tier1)
        await settle { model.box(.difficulty).phase == .ready && !model.strip(.studiedToday).loading }
        #expect(model.box(.cardCounts).phase == .ready)
        #expect(model.box(.reviewsDue).phase == .ready)
        #expect(model.box(.stability).phase == .ready)
        #expect(model.box(.retention).phase == .loading(fraction: nil))
        #expect(!model.strip(.studiedToday).loading && model.strip(.retentionToday).loading)
        #expect(model.subtitle == "100 cards in 4 decks · Updating…")

        service.send(request.refresh, .progress(.cardStatesOverTime, .fraction(0.4)))
        service.send(request.refresh, .progress(.knowledgeOverTime, .indeterminate))
        await settle { model.box(.cardsOverTime).phase == .loading(fraction: 0.4) }
        #expect(model.box(.knowledgeOverTime).phase == .loading(fraction: nil))

        service.answer(request, tier2)
        service.answer(request, [.knowledgeOverTime], failing: [.knowledgeOverTime: .notBuilt])
        service.send(request.refresh, .end(.complete))
        await settle { model.subtitle?.contains("Updated just now") == true }
        #expect(model.box(.cardsOverTime).phase == .ready)
        #expect(model.box(.knowledgeOverTime).phase == .failed)
        #expect(model.box(.knowledgeOverTime).data.legendNames == ["Known", "Studied"])
        #expect(model.subtitle == "100 cards in 4 decks · Updated just now")
        timers.advance(61)
        #expect(model.subtitle == "100 cards in 4 decks")
        #expect(model.state == .content)
    }

    @Test func leavingThePageCancelsItsRequest() async throws {
        let model = model()
        model.appear()
        let request = try await request(1)
        service.answer(request, tier1)
        await settle { model.box(.cardCounts).phase == .ready }
        model.disappear()
        await settle({ service.cancelled.contains(request.refresh) }, "the request was cancelled")
        // What it showed stays; nothing after the cancel lands.
        service.answer(request, tier2)
        await settle()
        #expect(model.box(.cardCounts).phase == .ready)
        #expect(model.box(.retention).phase == .loading(fraction: nil))
    }

    @Test func aFailedRequestLeavesFailedBoxesAndKeepsWhatItShowed() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        await complete(first, model)
        // The next refresh fails: the shown numbers stay, the subtitle says when they were read.
        model.disappear()
        model.appear()
        let second = try await request(2)
        service.send(second.refresh, .end(.failed(.internal("gone"))))
        await settle { model.subtitle?.contains("Updated ") == true && model.subtitle?.contains("just now") == false }
        #expect(model.box(.retention).phase == .ready)
        #expect(model.subtitle?.hasPrefix("100 cards in 4 decks · Updated ") == true)
        // With nothing shown, a failed request gives S12.8's line.
        let other = self.model(StatsPageSelection(deck: 2, period: .oneMonth), kept: StatsKeptResults(directory: nil))
        other.appear()
        let third = try await request(3)
        service.send(third.refresh, .end(.failed(.internal("gone"))))
        await settle { other.box(.retention).phase == .failed }
        #expect(other.box(.cardCounts).phase == .failed)
    }

    @Test func contextChangedKeepsWhatIsShownAndReadsAgain() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        service.answer(first, tier1)
        service.send(first.refresh, .end(.contextChanged))
        let second = try await request(2)
        #expect(model.box(.cardCounts).phase == .ready)
        #expect(model.box(.cardCounts).updating)
        #expect(model.box(.retention).phase == .loading(fraction: nil))
        let later = Fixture.context(now: (Fixture.nextDayAt - 60) * 1_000)
        service.answer(second, [.cardCounts], context: later)
        await settle { model.box(.cardCounts).source?.context == later }
        #expect(!model.box(.cardCounts).updating)
    }

    @Test func pageEndsGiveThePageStates() async throws {
        let model = model(StatsPageSelection(deck: 1, period: .oneMonth))
        model.appear()
        let first = try await request(1)
        service.send(first.refresh, .end(.scopeError(.unknownDeck)))
        await settle { model.state == .deckMissing }
        // Show All Decks.
        model.selectDeck(nil)
        #expect(model.state == .content)
        let second = try await request(2)
        service.send(second.refresh, .end(.unreadableRow(StatsUnreadable(table: "revlog", column: "ease", key: 1))))
        await settle { model.state == .unavailable }
        // A kept deck that the tree no longer has (S12.7), and no collection (S12.1).
        model.selectDeck(42)
        #expect(model.state == .deckMissing)
        model.collectionClosed()
        #expect(model.state == .noCollection && !model.controlsEnabled)
    }

    @Test func anEmptyCollectionIsNoCards() async throws {
        let model = model()
        model.appear()
        let request = try await request(1)
        service.answer(request, [.cardCounts], values: [.cardCounts: .cardCounts(Fixture.cardCounts(
            new: 0, learning: 0, relearning: 0, young: 0, mature: 0, suspended: 0, buried: 0, notes: 0))])
        await settle { model.state == .noCards }
        #expect(!model.controlsEnabled)
    }

    // MARK: Kept results (S10.2)

    @Test func keptResultsShowAtOnceAndRefreshInPlace() async throws {
        let first = model()
        first.appear()
        let request1 = try await request(1)
        await complete(request1, first)
        first.disappear()

        // Another window on the same scope: every box filled before any part arrives.
        let second = model()
        #expect(StatsBoxID.allCases.allSatisfy { second.box($0).phase != .loading(fraction: nil) })
        #expect(second.box(.retention).phase == .ready)
        second.appear()
        let request2 = try await request(2)
        #expect(second.box(.retention).updating)
        #expect(second.subtitle == "100 cards in 4 decks · Updating…")
        let later = Fixture.context(now: (Fixture.nextDayAt - 30) * 1_000)
        service.answer(request2, [.retentionByDay], context: later)
        await settle { second.box(.retention).source?.context == later }
        #expect(second.box(.retention).phase == .ready && !second.box(.retention).updating)
        #expect(second.box(.cardCounts).updating)
    }

    @Test func keptResultsNeverCrossScopes() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        await complete(first, model)
        model.selectDeck(1)
        // The deck was never viewed: S10.3's progress, not All Decks' numbers.
        #expect(model.boxes.values.allSatisfy { $0.phase == .loading(fraction: nil) && $0.source == nil })
        #expect(model.subtitle == nil)
        await settle { service.cancelled.contains(first.refresh) || service.requests.count == 2 }
        let second = try await request(2)
        #expect(second.scope == .deck(1))
        // A late part of the All Decks request never lands on the deck.
        service.answer(first, [.cardCounts])
        await settle()
        #expect(model.box(.cardCounts).phase == .loading(fraction: nil))
        service.answer(second, [.cardCounts])
        await settle { model.box(.cardCounts).phase == .ready }
        #expect(model.box(.cardCounts).source?.scope == .deck(1))
        #expect(model.subtitle == "100 cards in 2 decks · Updating…")
        // Back on All Decks: its kept result at once.
        model.selectDeck(nil)
        #expect(model.box(.retention).phase == .ready && model.box(.retention).source?.scope == .allDecks)
        await settle({ service.cancelled.contains(second.refresh) }, "the deck's request was cancelled")
    }

    @Test func keptResultsLiveOnTheDeviceForTheirCollectionOnly() async throws {
        let directory = URL.temporaryDirectory.appending(path: "stats-kept-\(UUID().uuidString)", directoryHint: .isDirectory)
        defer { try? FileManager.default.removeItem(at: directory) }
        let onDisk = StatsKeptResults(directory: directory)
        let first = model(kept: onDisk)
        first.appear()
        let request = try await request(1)
        await complete(request, first)
        first.disappear()
        await onDisk.waitForWrites()
        #expect((try? FileManager.default.contentsOfDirectory(atPath: directory.path).count) == 1)

        // A relaunch, the same collection: the kept sets come back with their values.
        let relaunched = StatsKeptResults(directory: directory)
        relaunched.open(collection: "collection-a")
        relaunched.prepare(.allDecks) {}
        await relaunched.waitForLoads()
        let sets = relaunched.sets(for: .allDecks)
        #expect(sets.count == 1 && sets.first?.complete == true)
        #expect(sets.first?.value(.retrievability) == Fixture.value(.retrievability))

        // Another collection: nothing is read, and the file goes.
        let other = StatsKeptResults(directory: directory)
        other.open(collection: "collection-b")
        other.prepare(.allDecks) {}
        await other.waitForLoads()
        #expect(other.sets(for: .allDecks).isEmpty)
        #expect((try? FileManager.default.contentsOfDirectory(atPath: directory.path).count) == 0)
    }

    @Test func aReplacedCollectionDropsTheKeptResults() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        await complete(first, model)
        model.receive(.collectionReplaced)
        #expect(model.box(.retention).phase == .loading(fraction: nil))
        #expect(kept.sets(for: .allDecks).isEmpty)
        timers.advance(0.5)
        let second = try await request(2)
        #expect(second.scope == .allDecks)
    }

    // MARK: Periods (S3.2, S10.3, S10.5)

    @Test func aPeriodChangeWithinTheYearMakesNoRequest() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        service.answer(first, [.retentionByDay])
        await settle { model.box(.retention).phase == .ready }
        #expect(model.box(.retention).data.bins.count == 30)
        model.selectPeriod(.threeMonths)
        model.selectPeriod(.oneYear)
        #expect(model.box(.retention).data.bins.count == 52)
        await settle()
        #expect(service.requests.count == 1)
        #expect(!service.cancelled.contains(first.refresh))
        // The running request's parts arrive in the new period's bins.
        service.answer(first, [.reviews])
        await settle { model.box(.reviewsOverTime).phase == .ready }
        #expect(model.box(.reviewsOverTime).data.bins.count == 52)
    }

    @Test func allWithNothingKeptReadsTheYearThenAllHistory() async throws {
        let model = model(StatsPageSelection(deck: nil, period: .all))
        model.appear()
        let first = try await request(1)
        #expect(first.period.history == .last12Months)
        service.answer(first, StatsPart.allCases)
        await settle { model.box(.knowledgeOverTime).source != nil }
        #expect(model.box(.cardCounts).phase == .ready)
        // The year on All's axis is partial: progress, with its values kept, never shown as all history.
        #expect(model.box(.retention).isLoading)
        #expect(model.box(.retention).data.bins.count == 14)
        #expect(model.box(.answers).isLoading)
        #expect(model.box(.activity).phase == .ready)
        service.send(first.refresh, .end(.complete))
        let second = try await request(2)
        #expect(second.period == .all && second.period.history == .allHistory)
        #expect(model.subtitle?.hasSuffix("Updating…") == true)
        service.answer(second, StatsPart.allCases)
        service.send(second.refresh, .end(.complete))
        await settle { model.subtitle?.hasSuffix("Updated just now") == true }
        #expect(model.box(.retention).phase == .ready && model.box(.answers).phase == .ready)
        // Kept now: the next All refresh is one request.
        model.disappear()
        model.appear()
        let third = try await request(3)
        #expect(third.period == .all)
    }

    @Test func aChangeToOrFromAllIsANewRequest() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        await complete(first, model)
        model.selectPeriod(.all)
        // The year is kept: All needs only the all-history read (S10.3's two reads are for nothing kept).
        let second = try await request(2)
        #expect(second.period.history == .allHistory)
        model.selectPeriod(.oneMonth)
        await settle({ service.cancelled.contains(second.refresh) }, "the All request was cancelled")
        let third = try await request(3)
        #expect(third.period == .oneMonth)
    }

    // MARK: Live refresh (S11) and selection

    @Test func aVisiblePageRefreshesHalfASecondAfterTheLastChange() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        await complete(first, model)
        model.receive(.graphsStale)
        timers.advance(0.3)
        model.receive(.dayChanged)
        timers.advance(0.3)
        await settle()
        #expect(service.requests.count == 1)
        timers.advance(0.2)
        let second = try await request(2)
        #expect(second.scope == .allDecks)
        await settle()
        #expect(service.requests.count == 2)
        // A hidden page waits until it appears.
        model.disappear()
        model.receive(.graphsStale)
        timers.advance(1)
        await settle()
        #expect(service.requests.count == 2)
        model.appear()
        _ = try await request(3)
    }

    @Test func theSelectionIsCarriedAcrossARefreshAndClearedByAPeriodChange() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        await complete(first, model)
        model.select(StatsChartSelection(bin: 28, pinned: true), in: .reviewsOverTime)
        model.receive(.graphsStale)
        timers.advance(0.5)
        let second = try await request(2)
        await complete(second, model, context: Fixture.context(now: (Fixture.nextDayAt - 10) * 1_000))
        #expect(model.selection(for: .reviewsOverTime) == StatsChartSelection(bin: 28, pinned: true))
        // A new day moves the bins: the selected day keeps its place by its date.
        model.receive(.dayChanged)
        timers.advance(0.5)
        let third = try await request(3)
        await complete(third, model, context: Fixture.context(nextDayAt: Fixture.nextDayAt + 86_400,
                                                              now: (Fixture.nextDayAt + 3_600) * 1_000))
        #expect(model.selection(for: .reviewsOverTime) == StatsChartSelection(bin: 27, pinned: true))
        model.selectPeriod(.threeMonths)
        #expect(model.selection(for: .reviewsOverTime) == .none)
        model.select(StatsChartSelection(bin: 3), in: .retention)
        model.setRetentionKind(.mature)
        #expect(model.selection(for: .retention) == .none)
    }

    @Test func theSubtitleCountsTheDecksOfTheScope() async throws {
        let model = model(StatsPageSelection(deck: 2, period: .oneMonth))
        model.appear()
        let request = try await request(1)
        await complete(request, model)
        #expect(model.subtitle == "100 cards in 1 deck · Updated just now")
        #expect(model.filteredDeckNote == nil)
        model.selectDeck(9)
        #expect(model.filteredDeckNote == "A filtered deck shows the cards it holds now. Their reviews also count in their home decks.")
    }

    // MARK: Review findings

    @Test func aPageEndClearsWhenTheScopeReadsAgain() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        service.send(first.refresh, .end(.unreadableRow(StatsUnreadable(table: "revlog", column: "ease", key: 1))))
        await settle { model.state == .unavailable }
        // Check Database repaired the row: the next refresh's parts bring the page back.
        model.receive(.graphsStale)
        timers.advance(0.5)
        let second = try await request(2)
        service.answer(second, [.cardCounts])
        await settle { model.state == .content }
        // A deck deleted, then restored by an undo.
        model.selectDeck(2)
        let third = try await request(3)
        service.send(third.refresh, .end(.scopeError(.unknownDeck)))
        await settle { model.state == .deckMissing }
        model.decksChanged(Array(Fixture.nodes().dropLast()))
        #expect(model.state == .content)
        _ = try await request(4)
    }

    @Test func aMemoryWarningKeepsEveryOpenPagesScope() async throws {
        let first = model()
        first.appear()
        await complete(try await request(1), first)
        let second = model(StatsPageSelection(deck: 2, period: .oneMonth))
        second.appear()
        await complete(try await request(2), second)
        second.memoryWarning()
        #expect(kept.sets(for: .allDecks).count == 1 && kept.sets(for: .deck(2)).count == 1)
        #expect(first.box(.retention).phase == .ready)
        // A window that closes no longer holds its scope.
        first.selectDeck(1)
        first.memoryWarning()
        #expect(kept.sets(for: .allDecks).isEmpty)
    }

    @Test func aFailedStripPartShowsADashNotProgress() async throws {
        let model = model()
        model.appear()
        let request = try await request(1)
        service.answer(request, [.studiedToday, .retention], failing: [.studiedToday: .notBuilt, .retention: .failed("x")])
        await settle { !model.strip(.retentionToday).loading }
        #expect(model.strip(.studiedToday).value == "—" && model.strip(.retentionToday).value == "—")
    }

    @Test func byDeckWaitsForTheDeckTree() async throws {
        let model = StatsPageModel(service: service, kept: kept, store: StatsSelectionStore(defaults: defaults),
                                   selection: StatsPageSelection(deck: nil, period: .oneMonth), timers: timers,
                                   calendar: Fixture.calendar(), locale: Fixture.locale)
        model.collectionOpened(identity: "collection-a")
        model.appear()
        let request = try await request(1)
        service.answer(request, [.byDeck])
        await settle { model.box(.byDeck).source != nil || service.requests.count > 1 }
        await settle()
        #expect(model.box(.byDeck).phase == .loading(fraction: nil))
        model.decksChanged([Fixture.nodes()[0]])
        #expect(model.box(.byDeck).phase == .ready)
        // A row the tree does not have yet keeps its share.
        guard case let .byDeck(table)? = model.box(.byDeck).detail else { Issue.record("no table"); return }
        #expect(table.rows.map(\.timeShareText) == ["75.0%", "25.0%"])
        #expect(table.rows.last?.name == "—")
    }

    @Test func byAnswersTotalIsByTypesWithUnratedRows() throws {
        let answers = StatsAnswers(again: 1, hard: 1, good: 5, easy: 1)
        let set = Fixture.set(values: [.byAnswer: .byAnswer(StatsByAnswer(count: Fixture.map([-1: answers]),
                                                                          time: Fixture.map([-1: answers]), unrated: 2))])
        let presenter = Fixture.presenter(set, selection: StatsPageSelection(deck: nil, period: .oneMonth, reviewsMode: .byAnswer))
        let box = try #require(Fixture.content(.reviewsOverTime, presenter))
        #expect(box.data.series.reduce(0) { $0 + ($1.values[28] ?? 0) } == 8)
        #expect(box.data.calloutLines.first?.values[28] == 10)
        #expect(box.figure?.value == "15")
    }

    @Test func extraValuesComeFromTheNewestSetThatHasThem() throws {
        // Retrievability's tier-1 set lacks the desired retention; a newer set of the scope has it.
        let present = Fixture.set(StatsPart.allCases.filter { $0.tier == .present })
        let log = Fixture.set([.retentionByDay, .activityDays])
        let presenter = StatsPresenter(set: present, period: .oneMonth, selection: StatsPageSelection(deck: nil, period: .oneMonth),
                                       decks: StatsDeckDirectory(nodes: Fixture.nodes()), calendar: Fixture.calendar(),
                                       locale: Fixture.locale, extras: [log])!
        let box = try #require(Fixture.content(.retrievability, presenter))
        #expect(box.detail == .retrievability(desiredRetention: Double(Float(0.9))))
        #expect(box.figures.first?.value == "1")
    }

    @Test func theSubtitleSaysWhenKeptNumbersWereRead() async throws {
        let model = model()
        model.appear()
        let first = try await request(1)
        // Read yesterday at 03:00: the date, not the time.
        let yesterday = Fixture.context(nextDayAt: Fixture.nextDayAt - 86_400, now: (Fixture.nextDayAt - 86_400 - 3_600) * 1_000)
        await complete(first, model, context: yesterday)
        timers.advance(61)
        #expect(model.subtitle == "100 cards in 4 decks")
        model.disappear()
        model.appear()
        let second = try await request(2)
        #expect(model.subtitle == "100 cards in 4 decks · Updating…")
        model.disappear()
        let format = StatsChartFormat(locale: Fixture.locale, calendar: Fixture.calendar(), now: timers.now())
        let read = Date(timeIntervalSince1970: TimeInterval(Fixture.nextDayAt - 86_400 - 3_600))
        #expect(model.subtitle == "100 cards in 4 decks · Updated \(format.shortDate(read))")
        await settle({ service.cancelled.contains(second.refresh) }, "the request was cancelled")
        // A finished stream is not cancelled again.
        #expect(!service.cancelled.contains(first.refresh))
    }

    @Test func aPartialBoxFailsWhenTheAllHistoryRequestFails() async throws {
        let model = model(StatsPageSelection(deck: nil, period: .all))
        model.appear()
        let first = try await request(1)
        service.answer(first, StatsPart.allCases)
        service.send(first.refresh, .end(.complete))
        let second = try await request(2)
        #expect(model.box(.retention).isLoading)
        service.send(second.refresh, .end(.failed(.internal("gone"))))
        await settle { model.box(.retention).phase == .failed }
        #expect(model.box(.answers).phase == .failed)
        #expect(model.box(.cardCounts).phase == .ready)
    }

    @Test func everyWindowWaitingForTheSameDeviceReadRefreshes() async throws {
        let directory = URL.temporaryDirectory.appending(path: "stats-wait-\(UUID().uuidString)", directoryHint: .isDirectory)
        defer { try? FileManager.default.removeItem(at: directory) }
        let onDisk = StatsKeptResults(directory: directory)
        let first = model(kept: onDisk)
        let second = model(kept: onDisk)
        first.appear()
        second.appear()
        await onDisk.waitForLoads()
        // Both windows asked while one read ran: both refresh when it ends.
        await settle({ service.requests.count == 2 }, "both windows refreshed")
    }
}
