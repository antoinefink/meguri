import CoreBridge
import Foundation
import Synchronization
import Testing

@testable import Stats

/// Fixture records for the page model (made-up values, no collection), and a fake
/// `StatsService` whose requests the tests answer by hand.
nonisolated enum Fixture {
    /// A value a test needed was not there.
    struct Missing: Error {}
    /// 2026-10-13 04:00 in Europe/Paris (UTC+2), rollover hour 4: `T0` is 2026-10-12, a Monday
    /// (`stats/activity-heatmap` AH3's example).
    static let nextDayAt: Int64 = 1_791_856_800
    static let locale = Locale(identifier: "en_GB")

    static func calendar(firstWeekday: Int = 2, zone: String = "Europe/Paris") -> Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: zone)!
        // The locale first: setting it resets the first weekday.
        calendar.locale = locale
        calendar.firstWeekday = firstWeekday
        return calendar
    }

    static func context(nextDayAt: Int64 = nextDayAt, rollover: UInt8 = 4, offset: Int32 = 7_200,
                        now: Int64? = nil) -> StatsContext {
        StatsContext(nextDayAt: nextDayAt, today: 2_000, rolloverHour: rollover,
                     nowMilliseconds: now ?? (nextDayAt - 3_600) * 1_000, utcOffsetSeconds: offset,
                     zoneIdentifier: "Europe/Paris", generation: 1, seq: 1)
    }

    static func header(refresh: UInt64, history: StatsHistory, oldest: StatsOldestDay = .day(-400),
                       context: StatsContext = context()) -> StatsPageHeader {
        StatsPageHeader(refresh: refresh, history: history, context: context,
                        preferences: StatsPreferences(firstDayOfWeek: 0, cardCountsSeparateInactive: true,
                                                      futureDueShowBacklog: true, fsrs: true),
                        browseScope: .none, oldestDay: oldest)
    }

    static func map<Value>(_ pairs: [Int: Value]) -> StatsSparseMap<Value> {
        let keys = pairs.keys.sorted()
        return StatsSparseMap(keys: keys, values: keys.map { pairs[$0]! })
    }

    static func ranges<Value>(_ value: Value, all: Value? = nil) -> StatsRanges<Value> {
        StatsRanges(oneMonth: value, threeMonths: value, sixMonths: value, oneYear: value, allTime: all)
    }

    static func cardCounts(new: Int = 10, learning: Int = 2, relearning: Int = 1, young: Int = 20, mature: Int = 60,
                           suspended: Int = 5, buried: Int = 2, notes: Int = 80) -> StatsCardCounts {
        let excluding = StatsCardStateCounts(new: new, learning: learning, relearning: relearning, young: young,
                                             mature: mature, suspended: suspended, buried: buried)
        return StatsCardCounts(excludingInactive: excluding, includingInactive: excluding, notes: notes,
                               labelsSeparate: [StatsCountLabelText(label: .mature, itemText: "prop:ivl>=21")],
                               labelsTogether: [], suspendedByState: .zero)
    }

    static func retentionDay(_ yp: Int, _ yf: Int, _ mp: Int, _ mf: Int, desired: Double = 0.9) -> StatsRetentionDay {
        let rows = yp + yf + mp + mf
        return StatsRetentionDay(youngPassed: yp, youngFailed: yf, maturePassed: mp, matureFailed: mf,
                                 desiredSum: Double(rows) * desired, desiredRows: rows)
    }

    static func kinds(learn: Int = 0, relearn: Int = 0, young: Int = 0, mature: Int = 0, filtered: Int = 0) -> StatsReviewKinds {
        StatsReviewKinds(learn: learn, relearn: relearn, young: young, mature: mature, filtered: filtered)
    }

    /// A value for every part, small and hand-countable.
    static func value(_ part: StatsPart, history: StatsHistory = .last12Months) -> StatsPartValue {
        let all = history == .allHistory
        switch part {
        case .cardCounts: return .cardCounts(cardCounts())
        case .futureDue:
            return .futureDue(StatsFutureDue(due: map([-2: 3, 0: 5, 1: 4, 40: 2]), haveBacklog: true, dailyLoad: 7,
                                             dueTomorrow: 4, dueYoung: map([0: 2, 1: 1])))
        case .intervals: return .intervals(map([1: 1, 2: 1, 3: 1, 100: 1]))
        case .stability: return .stability(StatsStability(byDays: map([0: 1, 1: 2, 3: 1, 400: 1]), leftOut: 0))
        case .difficulty: return .difficulty(StatsDifficulty(byPercent: map([5: 2, 55: 1, 99: 1]), median: 30, leftOut: 0))
        case .retrievability:
            return .retrievability(StatsRetrievability(byPercent: map([50: 1, 95: 3]), average: 83.75, sumByCard: 3.35,
                                                       sumByNote: 2.6, leftOut: 2))
        case .studiedToday: return .studiedToday(StatsStudiedToday(answers: 30, cards: 12, timeMilliseconds: 1_140_000))
        case .reviews:
            return .reviews(StatsReviews(count: map([-1: kinds(learn: 2, mature: 8), 0: kinds(young: 5)]),
                                         time: map([-1: kinds(learn: 20_000, mature: 80_000), 0: kinds(young: 50_000)])))
        case .byAnswer:
            let answers = StatsAnswers(again: 1, hard: 1, good: 7, easy: 1)
            return .byAnswer(StatsByAnswer(count: map([-1: answers]), time: map([-1: answers]), unrated: 0))
        case .retention:
            let today = StatsRetentionCounts(youngPassed: 8, youngFailed: 1, maturePassed: 9, matureFailed: 2)
            return .retention(StatsTrueRetention(today: today, yesterday: today, week: today, month: today, year: today,
                                                 allTime: all ? today : nil))
        case .retentionByDay:
            return .retentionByDay(StatsRetentionByDay(days: map([-1: retentionDay(10, 2, 30, 3), 0: retentionDay(4, 1, 0, 0)]),
                                                       retentions: [0.9], several: false))
        case .hours:
            var hours = Array(repeating: StatsHour(total: 0, correct: 0), count: 24)
            hours[9] = StatsHour(total: 30, correct: 25)
            hours[20] = StatsHour(total: 10, correct: 9)
            var retention = Array(repeating: StatsHourRetention(passed: 0, failed: 0), count: 24)
            retention[9] = StatsHourRetention(passed: 18, failed: 2)
            retention[20] = StatsHourRetention(passed: 9, failed: 1)
            return .hours(StatsHours(hours: ranges(hours, all: all ? hours : nil),
                                     retentionByHour: ranges(retention, all: all ? retention : nil)))
        case .buttons:
            let presses = StatsButtonPresses(learning: [2, 0, 8, 0], young: [1, 1, 6, 2], mature: [1, 0, 8, 1])
            return .buttons(StatsButtons(ranges: ranges(presses, all: all ? presses : nil)))
        case .added: return .added(map([-3: 4, 0: 2, 1: 1]))
        case .cardStatesOverTime:
            return .cardStatesOverTime(StatsCardStatesOverTime(
                start: StatsStateCounts(new: 50, learning: 0, relearning: 0, young: 10, mature: 30),
                deltas: map([-10: StatsStateCounts(new: -5, learning: 5, relearning: 0, young: 0, mature: 0),
                             -1: StatsStateCounts(new: 0, learning: -3, relearning: 0, young: 1, mature: 2)]),
                leftOut: 5, firstDay: -700))
        case .started: return .started(StatsStarted(days: map([-10: 5]), firstDay: -700))
        case .byDeck:
            let values = StatsDeckValues(reviews: 10, timeMilliseconds: 60_000, passed: 8, failed: 2)
            let other = StatsDeckValues(reviews: 5, timeMilliseconds: 20_000, passed: 0, failed: 0)
            return .byDeck(StatsByDeck(rows: [
                StatsDeckRow(deckID: 1, ownCards: false, cards: 70, windows: ranges(values, all: all ? values : nil)),
                StatsDeckRow(deckID: 2, ownCards: false, cards: 30, windows: ranges(other, all: all ? other : nil)),
            ]))
        case .activityDays: return .activityDays(map([-3: 4, -2: 6, -1: 5, -400: 2]))
        case .knowledgeOverTime:
            return .knowledgeOverTime(StatsKnowledgeOverTime(samples: [
                StatsKnowledgeSample(day: -30, instantMilliseconds: 0, knowledge: 70.4, studied: 80),
                StatsKnowledgeSample(day: -1, instantMilliseconds: 0, knowledge: 75.2, studied: 85),
                StatsKnowledgeSample(day: 0, instantMilliseconds: 0, knowledge: 76.6, studied: 86),
            ], failed: 0, leftOut: 5))
        }
    }

    static func nodes() -> [CountedDeckNode] {
        let child = CountedDeckNode(id: 3, name: "Verbs", filtered: false, studyCollapsed: false, browserCollapsed: false,
                                    counts: ReviewCounts(new: 1, learning: 0, review: 2), children: [])
        return [
            CountedDeckNode(id: 1, name: "Words", filtered: false, studyCollapsed: false, browserCollapsed: false,
                            counts: ReviewCounts(new: 5, learning: 1, review: 10), children: [child]),
            CountedDeckNode(id: 2, name: "Places", filtered: false, studyCollapsed: false, browserCollapsed: false,
                            counts: ReviewCounts(new: 0, learning: 0, review: 4), children: []),
            CountedDeckNode(id: 9, name: "Cram", filtered: true, studyCollapsed: false, browserCollapsed: false,
                            counts: ReviewCounts(new: 0, learning: 0, review: 0), children: []),
        ]
    }

    static func set(_ parts: [StatsPart] = StatsPart.allCases, scope: StatsPageScope = .allDecks,
                    history: StatsHistory = .last12Months, oldest: StatsOldestDay = .day(-400),
                    context: StatsContext = context(), values: [StatsPart: StatsPartValue] = [:]) -> StatsPartSet {
        var set = StatsPartSet(scope: scope, history: history, refresh: 1)
        let header = header(refresh: 1, history: history, oldest: oldest, context: context)
        for part in parts {
            set.receive(StatsPartResult(part: part, header: header, outcome: .data(values[part] ?? value(part, history: history))))
        }
        set.complete = true
        return set
    }

    @MainActor static func presenter(_ set: StatsPartSet, period: StatsPagePeriod = .oneMonth,
                          selection: StatsPageSelection? = nil, decks: [CountedDeckNode] = nodes(),
                          calendar: Calendar = calendar()) -> StatsPresenter {
        var chosen = selection ?? StatsPageSelection(deck: nil, period: period)
        chosen.period = period
        return StatsPresenter(set: set, period: period, selection: chosen, decks: StatsDeckDirectory(nodes: decks),
                              calendar: calendar, locale: locale)!
    }

    @MainActor static func content(_ box: StatsBoxID, _ presenter: StatsPresenter) -> StatsBoxContent? {
        if case let .content(content) = presenter.box(box) { return content }
        return nil
    }
}

/// A `StatsService` whose page requests the test answers: each request's stream stays open
/// until the test ends it or the model cancels it.
nonisolated final class FakeStatsService: StatsService {
    struct State {
        var requests: [StatsPageRequest] = []
        var streams: [UInt64: AsyncStream<StatsPageMessage>.Continuation] = [:]
        var cancelled: Set<UInt64> = []
        var released = 0
        var idle = 0
    }

    let state = Mutex(State())

    var requests: [StatsPageRequest] { state.withLock { $0.requests } }
    var cancelled: Set<UInt64> { state.withLock { $0.cancelled } }

    func page(_ request: StatsPageRequest) async throws(BridgeError) -> StatsPageStream {
        let (stream, continuation) = AsyncStream.makeStream(of: StatsPageMessage.self, bufferingPolicy: .unbounded)
        state.withLock {
            $0.requests.append(request)
            $0.streams[request.refresh] = continuation
        }
        let refresh = request.refresh
        return StatsPageStream(replaying: stream) { [self] in
            let open = self.state.withLock { state -> AsyncStream<StatsPageMessage>.Continuation? in
                state.cancelled.insert(refresh)
                return state.streams.removeValue(forKey: refresh)
            }
            open?.finish()
        }
    }

    func send(_ refresh: UInt64, _ message: StatsPageMessage) {
        let stream = state.withLock { $0.streams[refresh] }
        stream?.yield(message)
        if case .end = message {
            stream?.finish()
            state.withLock { _ = $0.streams.removeValue(forKey: refresh) }
        }
    }

    /// Sends `parts` of request `request` with fixture values.
    func answer(_ request: StatsPageRequest, _ parts: [StatsPart], oldest: StatsOldestDay = .day(-400),
                context: StatsContext = Fixture.context(), values: [StatsPart: StatsPartValue] = [:],
                failing: [StatsPart: StatsPartFailure] = [:]) {
        let history = request.period.history
        let header = Fixture.header(refresh: request.refresh, history: history, oldest: oldest, context: context)
        for part in parts {
            let outcome: StatsPartOutcome = failing[part].map { .failed($0) }
                ?? .data(values[part] ?? Fixture.value(part, history: history))
            send(request.refresh, .part(StatsPartResult(part: part, header: header, outcome: outcome)))
        }
    }

    func dayCards(scope: StatsPageScope, context: StatsContext, day: Int, kind: StatsDayKind,
                  cancel: CancelToken?) async throws(BridgeError) -> StatsDayCards { .noCards }

    func releaseCaches() async throws(BridgeError) { state.withLock { $0.released += 1 } }

    func homeIdle() async throws(BridgeError) { state.withLock { $0.idle += 1 } }
}

/// Timers the test moves by hand.
@MainActor
final class ManualTimers: StatsTimers {
    private struct Entry {
        let id: Int
        let at: Date
        let action: @MainActor () -> Void
    }

    var current = Date(timeIntervalSince1970: TimeInterval(Fixture.nextDayAt - 3_600))
    private var entries: [Entry] = []
    private var next = 0

    func now() -> Date { current }

    func schedule(after delay: Duration, _ action: @escaping @MainActor () -> Void) -> StatsTimerToken {
        next += 1
        let id = next
        let seconds = Double(delay.components.seconds) + Double(delay.components.attoseconds) / 1e18
        entries.append(Entry(id: id, at: current.addingTimeInterval(seconds), action: action))
        return StatsTimerToken { [weak self] in self?.entries.removeAll { $0.id == id } }
    }

    var pending: Int { entries.count }

    func advance(_ seconds: TimeInterval) {
        current = current.addingTimeInterval(seconds)
        let due = entries.filter { $0.at <= current }.sorted { $0.at < $1.at }
        entries.removeAll { $0.at <= current }
        for entry in due { entry.action() }
    }
}

/// Lets the model's request tasks run until `condition` holds.
@MainActor
func settle(_ condition: () -> Bool = { false }, _ comment: Comment? = nil) async {
    for _ in 0..<400 {
        if condition() { return }
        await Task.yield()
    }
    if comment != nil { #expect(condition(), comment) }
}
