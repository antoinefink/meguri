import CoreBridge
import Foundation
import Synchronization

/// A made-up collection's Statistics page parts (SAMPLE data, no collection), for the test-hook
/// routes and previews of the page (plan U296): about three years of study over a small deck
/// tree, answered by tier with short delays as the core does (`stats/history-reads` H15), so the
/// page fills box by box. Nothing is read or written.
nonisolated public final class StatsSampleService: StatsService {
    /// What the sample collection looks like.
    public enum Shape: Sendable, Hashable {
        /// Cards and three years of reviews.
        case typical
        /// Cards and no review yet (S12.3).
        case noReviews
        /// No card at all (S12.2).
        case noCards
    }

    /// How its requests end.
    public enum Ending: Sendable, Hashable {
        /// Every part, tier by tier.
        case complete
        /// The parts of the tiers before this one, then nothing: the later boxes keep their
        /// progress (S10.3), the knowledge replay reporting a fraction.
        case stall(StatsTier)
        /// S12.8: an unreadable row of the scope, before any part.
        case unreadable
        /// S12.8's second case: this part fails alone.
        case partFails(StatsPart)
        /// The first request completes; every later one stalls before its first tier, so the
        /// page shows its kept result while it refreshes (S10.2, S10.4's "Updating…").
        case laterStall
    }

    private let shape: Shape
    private let ending: Ending
    private let tierDelay: Duration
    private let requests = Mutex(0)

    /// - Parameter tierDelay: the wait before each tier (the first included).
    public init(shape: Shape = .typical, ending: Ending = .complete, tierDelay: Duration = .milliseconds(120)) {
        self.shape = shape
        self.ending = ending
        self.tierDelay = tierDelay
    }

    public func page(_ request: StatsPageRequest) async throws(BridgeError) -> StatsPageStream {
        let (stream, continuation) = AsyncStream.makeStream(of: StatsPageMessage.self, bufferingPolicy: .unbounded)
        let collection = StatsSampleCollection(shape: shape, scope: request.scope, history: request.period.history)
        let header = collection.header(refresh: request.refresh)
        let first = requests.withLock { count in
            count += 1
            return count == 1
        }
        let ending: Ending = ending == .laterStall ? (first ? .complete : .stall(.present)) : ending
        let delay = tierDelay
        let task = Task.detached {
            if case .unreadable = ending {
                try? await Task.sleep(for: delay)
                continuation.yield(.end(.unreadableRow(StatsUnreadable(table: "revlog", column: "ease", key: 1))))
                continuation.finish()
                return
            }
            if case let .deck(id) = request.scope, StatsSampleCollection.weight(id) == nil {
                continuation.yield(.end(.scopeError(.unknownDeck)))
                continuation.finish()
                return
            }
            for tier in [StatsTier.present, .reviewLog, .knowledge] {
                try? await Task.sleep(for: delay)
                if Task.isCancelled { break }
                if case let .stall(stalled) = ending, tier >= stalled {
                    for part in request.parts where part.tier >= stalled {
                        continuation.yield(.progress(part, part.tier == .knowledge ? .fraction(0.4) : .indeterminate))
                    }
                    // The request stays open: the boxes keep their progress.
                    return
                }
                for part in request.parts where part.tier == tier {
                    let outcome: StatsPartOutcome = if case .partFails(part) = ending {
                        .failed(.failed("sample failure"))
                    } else {
                        .data(collection.value(part))
                    }
                    continuation.yield(.part(StatsPartResult(part: part, header: header, outcome: outcome)))
                }
            }
            continuation.yield(.end(Task.isCancelled ? .cancelled : .complete))
            continuation.finish()
        }
        return StatsPageStream(replaying: stream) {
            task.cancel()
            continuation.yield(.end(.cancelled))
            continuation.finish()
        }
    }

    public func dayCards(scope: StatsPageScope, context: StatsContext, day: Int, kind: StatsDayKind,
                         cancel: CancelToken?) async throws(BridgeError) -> StatsDayCards {
        throw .unavailable(reason: "sample collection")
    }

    public func releaseCaches() async throws(BridgeError) {}
    public func homeIdle() async throws(BridgeError) {}

    /// The sample deck tree, with today's counts: Languages (Japanese, Spanish), Science
    /// (Zindro), Music, and the filtered deck Review Ahead.
    public static var decks: [CountedDeckNode] {
        func node(_ id: Int64, _ name: String, filtered: Bool = false, _ counts: (UInt32, UInt32, UInt32),
                  _ children: [CountedDeckNode] = []) -> CountedDeckNode {
            CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: false, browserCollapsed: false,
                            counts: ReviewCounts(new: counts.0, learning: counts.1, review: counts.2), children: children)
        }
        return [
            node(StatsSampleCollection.languages, "Languages", (20, 6, 97), [
                node(StatsSampleCollection.japanese, "Japanese", (10, 4, 71)),
                node(StatsSampleCollection.spanish, "Spanish", (10, 2, 26)),
            ]),
            node(StatsSampleCollection.science, "Science", (10, 1, 38), [
                node(StatsSampleCollection.zindro, "Zindro", (5, 1, 22)),
            ]),
            node(StatsSampleCollection.music, "Music", (5, 0, 9)),
            node(StatsSampleCollection.reviewAhead, "Review Ahead", filtered: true, (0, 0, 12)),
        ]
    }
}

/// The sample collection's numbers for one scope and history: deterministic, from a seeded
/// hash of each day, scaled by the deck's share of the collection.
nonisolated struct StatsSampleCollection {
    static let languages: Int64 = 1_700_000_000_101
    static let japanese: Int64 = 1_700_000_000_102
    static let spanish: Int64 = 1_700_000_000_103
    static let science: Int64 = 1_700_000_000_104
    static let zindro: Int64 = 1_700_000_000_105
    static let music: Int64 = 1_700_000_000_106
    static let reviewAhead: Int64 = 1_700_000_000_109

    /// A deck's share of the collection's cards and reviews (with its subdecks); nil for no deck.
    static func weight(_ deck: Int64) -> Double? {
        switch deck {
        case languages: 0.62
        case japanese: 0.41
        case spanish: 0.21
        case science: 0.28
        case zindro: 0.17
        case music: 0.10
        case reviewAhead: 0.03
        default: nil
        }
    }

    /// The first review, about three years ago.
    static let firstDay = -1_090

    let shape: StatsSampleService.Shape
    let scope: StatsPageScope
    let history: StatsHistory
    private let weight: Double

    init(shape: StatsSampleService.Shape, scope: StatsPageScope, history: StatsHistory) {
        self.shape = shape
        self.scope = scope
        self.history = history
        weight = switch scope {
        case .allDecks, .search: 1
        case let .deck(id): Self.weight(id) ?? 0
        }
    }

    private var reviewed: Bool { shape == .typical }
    private var hasCards: Bool { shape != .noCards }
    private var windowStart: Int { history == .allHistory ? Self.firstDay : -366 }

    func header(refresh: UInt64) -> StatsPageHeader {
        StatsPageHeader(refresh: refresh, history: history, context: Self.context(),
                        preferences: StatsPreferences(firstDayOfWeek: 0, cardCountsSeparateInactive: true,
                                                      futureDueShowBacklog: true, fsrs: true),
                        browseScope: scope == .allDecks ? .none : .text("deck:Sample"),
                        oldestDay: reviewed ? .day(Self.firstDay) : .noRows)
    }

    /// Now, with Anki's day ending at the next 4 a.m. (rollover hour 4) in the device's zone.
    static func context() -> StatsContext {
        let now = Date()
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .autoupdatingCurrent
        let fourToday = calendar.date(bySettingHour: 4, minute: 0, second: 0, of: now) ?? now
        let next = fourToday > now ? fourToday : calendar.date(byAdding: .day, value: 1, to: fourToday) ?? now
        return StatsContext(nextDayAt: Int64(next.timeIntervalSince1970), today: 3_000, rolloverHour: 4,
                            nowMilliseconds: Int64(now.timeIntervalSince1970 * 1_000),
                            utcOffsetSeconds: Int32(TimeZone.autoupdatingCurrent.secondsFromGMT(for: now)),
                            zoneIdentifier: TimeZone.autoupdatingCurrent.identifier, generation: 1, seq: 1)
    }

    // MARK: Days

    /// A value in 0..<1 for `day` and `salt`.
    private static func noise(_ day: Int, _ salt: UInt64) -> Double {
        var x = UInt64(bitPattern: Int64(day)) &* 0x9E37_79B9_7F4A_7C15 &+ salt &* 0xBF58_476D_1CE4_E5B9
        x ^= x >> 31
        x &*= 0x94D0_49BB_1331_11EB
        x ^= x >> 29
        return Double(x % 10_000) / 10_000
    }

    /// Whether `day` was studied: most days, with a gap now and then and a run up to today.
    private static func studied(_ day: Int) -> Bool {
        guard day >= firstDay, day <= 0 else { return false }
        if day >= -46 { return true }
        return noise(day, 1) < 0.86
    }

    /// The collection's reviews on `day`, before the deck's share.
    private static func reviews(_ day: Int) -> Int {
        guard studied(day) else { return 0 }
        let age = Double(day - firstDay) / Double(-firstDay)
        let base = 70 + 110 * age
        let weekly = [0.7, 1.1, 1.05, 1.0, 1.1, 0.85, 0.6][((day % 7) + 7) % 7]
        return Int(base * weekly * (0.6 + 0.8 * noise(day, 2)))
    }

    private func scaled(_ value: Int) -> Int { Int((Double(value) * weight).rounded()) }

    /// The scope's reviews on `day`; none for a shape with no review.
    private func reviews(on day: Int) -> Int { reviewed ? scaled(Self.reviews(day)) : 0 }

    /// The scope's reviews on an average day, for the windows with no time axis.
    private var dailyReviews: Int { reviewed ? scaled(150) : 0 }

    private func map<Value>(_ days: [Int], _ value: (Int) -> Value?) -> StatsSparseMap<Value> {
        var keys: [Int] = []
        var values: [Value] = []
        for day in days {
            guard let made = value(day) else { continue }
            keys.append(day)
            values.append(made)
        }
        return StatsSparseMap(keys: keys, values: values)
    }

    private var window: [Int] { Array(windowStart...0) }

    private func windowValues<Value>(_ make: (Int) -> Value) -> StatsRanges<Value> {
        StatsRanges(oneMonth: make(30), threeMonths: make(90), sixMonths: make(182), oneYear: make(365),
                    allTime: history == .allHistory ? make(-Self.firstDay + 1) : nil)
    }

    // MARK: Parts

    func value(_ part: StatsPart) -> StatsPartValue {
        switch part {
        case .cardCounts: return .cardCounts(cardCounts)
        case .futureDue: return .futureDue(futureDue)
        case .intervals:
            return .intervals(reviewed ? map(Array(1...400)) { $0 % 3 == 0 || $0 < 30 ? scaled(Int(40 * exp(-Double($0) / 90))) : nil } : map([]) { _ in 0 })
        case .stability:
            return .stability(StatsStability(byDays: reviewed ? map(Array(0...900)) { scaled(Int(30 * exp(-Double($0) / 160))) } : map([]) { _ in 0 },
                                             leftOut: reviewed ? scaled(120) : 0))
        case .difficulty:
            return .difficulty(StatsDifficulty(
                byPercent: reviewed ? map(Array(1...100)) { scaled(Int(60 * exp(-pow(Double($0) - 48, 2) / 500))) } : map([]) { _ in 0 },
                median: reviewed ? 47 : 0, leftOut: reviewed ? scaled(120) : 0))
        case .retrievability:
            return .retrievability(StatsRetrievability(
                byPercent: reviewed ? map(Array(40...100)) { scaled(Int(4 + 120 * pow(Double($0) / 100, 9))) } : map([]) { _ in 0 },
                average: reviewed ? 91 : 0, sumByCard: Float(scaled(3_512)), sumByNote: Float(scaled(2_140)),
                leftOut: reviewed ? scaled(120) : 0))
        case .studiedToday:
            return .studiedToday(StatsStudiedToday(answers: reviewed ? scaled(164) : 0, cards: reviewed ? scaled(128) : 0,
                                                   timeMilliseconds: reviewed ? scaled(1_140_000) : 0))
        case .reviews:
            return .reviews(StatsReviews(count: map(window) { reviewKinds($0, time: false) },
                                         time: map(window) { reviewKinds($0, time: true) }))
        case .byAnswer:
            return .byAnswer(StatsByAnswer(count: map(window) { answers($0, time: false) },
                                           time: map(window) { answers($0, time: true) }, unrated: 0))
        case .retention:
            func counts(_ days: Int) -> StatsRetentionCounts {
                let total = (-days + 1...0).reduce(0) { $0 + reviews(on: $1) }
                return StatsRetentionCounts(youngPassed: total * 30 / 100, youngFailed: total * 4 / 100,
                                            maturePassed: total * 60 / 100, matureFailed: total * 6 / 100)
            }
            return .retention(StatsTrueRetention(today: counts(1), yesterday: counts(2), week: counts(7), month: counts(30),
                                                 year: counts(365), allTime: history == .allHistory ? counts(-Self.firstDay + 1) : nil))
        case .retentionByDay:
            return .retentionByDay(StatsRetentionByDay(days: map(window) { day in
                let total = reviews(on: day)
                guard total > 0 else { return nil }
                let fail = 0.06 + 0.06 * Self.noise(day, 3)
                let young = total * 35 / 100, mature = total - young
                return StatsRetentionDay(youngPassed: Int(Double(young) * (1 - fail * 1.4)), youngFailed: Int(Double(young) * fail * 1.4),
                                         maturePassed: Int(Double(mature) * (1 - fail)), matureFailed: Int(Double(mature) * fail),
                                         desiredSum: Double(total) * 0.9, desiredRows: total)
            }, retentions: [0.9], several: false))
        case .hours:
            let curve: [Double] = [0, 0, 0, 0, 0, 0, 1, 4, 8, 9, 6, 4, 5, 6, 3, 2, 2, 3, 5, 8, 10, 9, 6, 2]
            func hours(_ days: Int) -> [StatsHour] {
                let total = Double(min(days, -windowStart) * dailyReviews)
                return curve.map { StatsHour(total: Int(total * $0 / 93), correct: Int(total * $0 / 93 * 0.9)) }
            }
            func retention(_ days: Int) -> [StatsHourRetention] {
                hours(days).enumerated().map { hour, value in
                    let rate = 0.86 + 0.08 * Self.noise(hour, 4)
                    return StatsHourRetention(passed: Int(Double(value.total) * rate), failed: Int(Double(value.total) * (1 - rate)))
                }
            }
            return .hours(StatsHours(hours: windowValues(hours), retentionByHour: windowValues(retention)))
        case .buttons:
            return .buttons(StatsButtons(ranges: windowValues { days in
                let n = Double(min(days, -windowStart) * dailyReviews)
                return StatsButtonPresses(learning: [Int(n * 0.03), Int(n * 0.01), Int(n * 0.09), Int(n * 0.01)],
                                          young: [Int(n * 0.03), Int(n * 0.02), Int(n * 0.22), Int(n * 0.03)],
                                          mature: [Int(n * 0.04), Int(n * 0.03), Int(n * 0.43), Int(n * 0.06)])
            }))
        case .added:
            return .added(hasCards ? map(Array(Self.firstDay - 30...0)) { day in
                let n = Self.noise(day, 5) < 0.3 ? scaled(Int(18 * Self.noise(day, 6))) : 0
                return n > 0 ? n : nil
            } : map([]) { _ in 0 })
        case .cardStatesOverTime:
            return .cardStatesOverTime(cardStatesOverTime)
        case .started:
            return .started(StatsStarted(days: reviewed ? map(window) { day in
                Self.studied(day) ? scaled(Int(14 * Self.noise(day, 7))) : nil
            } : map([]) { _ in 0 }, firstDay: hasCards ? Self.firstDay - 30 : nil))
        case .byDeck:
            return .byDeck(StatsByDeck(rows: byDeckRows))
        case .activityDays:
            return .activityDays(map(Array(Self.firstDay...0)) { day in
                let n = reviewed ? scaled(Self.reviews(day)) : 0
                return n > 0 ? n : nil
            })
        case .knowledgeOverTime:
            return .knowledgeOverTime(knowledge)
        }
    }

    private func reviewKinds(_ day: Int, time: Bool) -> StatsReviewKinds? {
        let n = reviewed ? scaled(Self.reviews(day)) : 0
        guard n > 0 else { return nil }
        let unit = time ? 7_500 : 1
        return StatsReviewKinds(learn: n * 8 / 100 * unit, relearn: n * 5 / 100 * unit, young: n * 30 / 100 * unit,
                                mature: (n - n * 43 / 100) * unit, filtered: 0)
    }

    private func answers(_ day: Int, time: Bool) -> StatsAnswers? {
        let n = reviewed ? scaled(Self.reviews(day)) : 0
        guard n > 0 else { return nil }
        let unit = time ? 7_500 : 1
        return StatsAnswers(again: n * 9 / 100 * unit, hard: n * 6 / 100 * unit, good: n * 74 / 100 * unit,
                            easy: (n - n * 89 / 100) * unit)
    }

    private var cardCounts: StatsCardCounts {
        let total = hasCards ? scaled(4_210) : 0
        let new = reviewed ? total * 18 / 100 : total
        let learning = reviewed ? total * 2 / 100 : 0
        let relearning = reviewed ? total / 100 : 0
        let young = reviewed ? total * 21 / 100 : 0
        let suspended = reviewed ? total * 3 / 100 : 0
        let buried = reviewed ? total / 200 : 0
        let mature = total - new - learning - relearning - young - suspended - buried
        let counts = StatsCardStateCounts(new: new, learning: learning, relearning: relearning, young: young,
                                          mature: mature, suspended: suspended, buried: buried)
        let labels: [StatsCountLabelText] = [
            .init(label: .new, itemText: "is:new"), .init(label: .learning, itemText: "is:learn -is:review"),
            .init(label: .relearning, itemText: "is:learn is:review"), .init(label: .young, itemText: "is:review prop:ivl<21"),
            .init(label: .mature, itemText: "prop:ivl>=21"), .init(label: .suspended, itemText: "is:suspended"),
            .init(label: .buried, itemText: "is:buried"),
        ]
        return StatsCardCounts(excludingInactive: counts, includingInactive: counts, notes: total * 7 / 10,
                               labelsSeparate: labels, labelsTogether: [], suspendedByState: .zero)
    }

    private var futureDue: StatsFutureDue {
        guard reviewed else {
            return StatsFutureDue(due: map([]) { _ in 0 }, haveBacklog: false, dailyLoad: 0, dueTomorrow: 0, dueYoung: map([]) { _ in 0 })
        }
        let due = map(Array(-3...800)) { day -> Int? in
            if day < 0 { return scaled(9) }
            let n = scaled(Int(150 * exp(-Double(day) / 70) * (0.7 + 0.6 * Self.noise(day, 8))))
            return n > 0 ? n : nil
        }
        let young = map(Array(0...60)) { day -> Int? in
            let n = scaled(Int(60 * exp(-Double(day) / 12)))
            return n > 0 ? n : nil
        }
        return StatsFutureDue(due: due, haveBacklog: true, dailyLoad: scaled(142), dueTomorrow: scaled(131), dueYoung: young)
    }

    private var cardStatesOverTime: StatsCardStatesOverTime {
        guard hasCards else {
            return StatsCardStatesOverTime(start: .zero, deltas: map([]) { _ in .zero }, leftOut: 0, firstDay: nil)
        }
        let start = windowStart
        // The collection grows from about 1,000 cards three years ago; cards mature over time.
        let age = Double(start - Self.firstDay) / Double(-Self.firstDay)
        let total = Double(scaled(4_100)) * (0.25 + 0.75 * age)
        let begin = StatsStateCounts(new: Int(total * 0.3), learning: Int(total * 0.03), relearning: Int(total * 0.01),
                                     young: Int(total * 0.26), mature: Int(total * (0.4 * age)))
        let deltas = map(Array(start...0)) { day -> StatsStateCounts? in
            guard reviewed, day % 3 == 0 else { return nil }
            let grow = max(1, scaled(4))
            return StatsStateCounts(new: Self.noise(day, 9) < 0.5 ? -grow : grow, learning: 0, relearning: 0,
                                    young: Self.noise(day, 10) < 0.5 ? grow : -1, mature: grow)
        }
        return StatsCardStatesOverTime(start: begin, deltas: deltas, leftOut: scaled(126), firstDay: Self.firstDay - 30)
    }

    private var knowledge: StatsKnowledgeOverTime {
        guard reviewed else { return StatsKnowledgeOverTime(samples: [], failed: 0, leftOut: 0) }
        let days = history == .allHistory ? Array(Self.firstDay...0) : Array(-366...0)
        let samples = days.map { day in
            let age = Double(day - Self.firstDay) / Double(-Self.firstDay)
            let studied = Double(scaled(3_950)) * (0.2 + 0.8 * age)
            let known = studied * (0.84 + 0.05 * age + 0.01 * Self.noise(day, 11))
            return StatsKnowledgeSample(day: day, instantMilliseconds: 0, knowledge: known, studied: Int(studied))
        }
        return StatsKnowledgeOverTime(samples: samples, failed: 0, leftOut: scaled(126))
    }

    private var byDeckRows: [StatsDeckRow] {
        guard hasCards else { return [] }
        let children: [Int64] = switch scope {
        case .allDecks, .search: [Self.languages, Self.science, Self.music]
        case .deck(Self.languages): [Self.japanese, Self.spanish]
        case .deck(Self.science): [Self.zindro]
        default: []
        }
        return children.map { id in
            let share = (Self.weight(id) ?? 0) / weight
            return StatsDeckRow(deckID: id, ownCards: false, cards: Int(Double(scaled(4_210)) * share), windows: windowValues { days in
                let reviews = Int(Double(min(days, -windowStart) * dailyReviews) * share)
                return StatsDeckValues(reviews: reviews, timeMilliseconds: reviews * 7_500,
                                       passed: Int(Double(reviews) * 0.9), failed: Int(Double(reviews) * 0.1))
            })
        }
    }
}
