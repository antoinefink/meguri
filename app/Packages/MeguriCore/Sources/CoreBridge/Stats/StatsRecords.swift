import Foundation

// The values of the Statistics page's parts (`stats/history-reads` H5),
// decoded from G6's bytes in the member orders of `stats/graphs-common`
// G6's table (the Anki records), the H6 to H8 extras after them as the core
// writes them, and the page's own parts as `meguri-ffi`'s `stats_page.rs`
// writes them. Day keys are `stats/day-buckets` B3's day indexes (0 today,
// −k k days ago); counts are cards or rows; times are milliseconds.

// MARK: - Shared shapes

/// One day's review-log rows by kind (`stats/graphs-revlog` V1), as counts or summed milliseconds.
public struct StatsReviewKinds: Sendable, Hashable, Codable {
    public var learn: Int, relearn: Int, young: Int, mature: Int, filtered: Int
    public init(learn: Int, relearn: Int, young: Int, mature: Int, filtered: Int) {
        self.learn = learn; self.relearn = relearn; self.young = young; self.mature = mature; self.filtered = filtered
    }
    public var total: Int { learn + relearn + young + mature + filtered }
}

/// One day's rows by answer (`stats/history-reads` H6), as counts or summed milliseconds.
public struct StatsAnswers: Sendable, Hashable, Codable {
    public var again: Int, hard: Int, good: Int, easy: Int
    public init(again: Int, hard: Int, good: Int, easy: Int) {
        self.again = again; self.hard = hard; self.good = good; self.easy = easy
    }
    public var total: Int { again + hard + good + easy }
}

/// The five card states of `stats/history-reads` H2, as counts or their changes on a day.
public struct StatsStateCounts: Sendable, Hashable, Codable {
    public var new: Int, learning: Int, relearning: Int, young: Int, mature: Int
    public init(new: Int, learning: Int, relearning: Int, young: Int, mature: Int) {
        self.new = new; self.learning = learning; self.relearning = relearning; self.young = young; self.mature = mature
    }
    public static let zero = StatsStateCounts(new: 0, learning: 0, relearning: 0, young: 0, mature: 0)
    public var total: Int { new + learning + relearning + young + mature }
    public static func + (lhs: StatsStateCounts, rhs: StatsStateCounts) -> StatsStateCounts {
        StatsStateCounts(new: lhs.new + rhs.new, learning: lhs.learning + rhs.learning,
                         relearning: lhs.relearning + rhs.relearning, young: lhs.young + rhs.young,
                         mature: lhs.mature + rhs.mature)
    }
}

/// Passed and failed rows of the retention rule (`stats/graphs-revlog` V4.1), young and mature apart.
public struct StatsRetentionCounts: Sendable, Hashable, Codable {
    public var youngPassed: Int, youngFailed: Int, maturePassed: Int, matureFailed: Int
    public init(youngPassed: Int, youngFailed: Int, maturePassed: Int, matureFailed: Int) {
        self.youngPassed = youngPassed; self.youngFailed = youngFailed
        self.maturePassed = maturePassed; self.matureFailed = matureFailed
    }
}

// MARK: - Anki's records (G6's table)

/// `reviews` (`stats/graphs-revlog` V2.2): the same day keys in both maps.
public struct StatsReviews: Sendable, Hashable, Codable {
    public let count: StatsSparseMap<StatsReviewKinds>
    public let time: StatsSparseMap<StatsReviewKinds>
    public init(count: StatsSparseMap<StatsReviewKinds>, time: StatsSparseMap<StatsReviewKinds>) {
        self.count = count; self.time = time
    }
}

/// `true_retention` (V4.3): `allTime` only with all history.
public struct StatsTrueRetention: Sendable, Hashable, Codable {
    public let today: StatsRetentionCounts, yesterday: StatsRetentionCounts, week: StatsRetentionCounts
    public let month: StatsRetentionCounts, year: StatsRetentionCounts
    public let allTime: StatsRetentionCounts?
    public init(today: StatsRetentionCounts, yesterday: StatsRetentionCounts, week: StatsRetentionCounts, month: StatsRetentionCounts, year: StatsRetentionCounts, allTime: StatsRetentionCounts?) {
        self.today = today; self.yesterday = yesterday; self.week = week; self.month = month; self.year = year; self.allTime = allTime
    }
}

/// One hour of `hours` (V5.3).
public struct StatsHour: Sendable, Hashable, Codable {
    public let total: Int, correct: Int
    public init(total: Int, correct: Int) {
        self.total = total; self.correct = correct
    }
}

/// One hour of `retention_by_hour` (`stats/history-reads` H7).
public struct StatsHourRetention: Sendable, Hashable, Codable {
    public let passed: Int, failed: Int
    public init(passed: Int, failed: Int) {
        self.passed = passed; self.failed = failed
    }
}

/// The ranges of `hours` and `buttons` (V5.2, H7's `sixMonths`); `allTime` only with all history.
public struct StatsRanges<Value: Sendable & Hashable>: Sendable, Hashable {
    public let oneMonth: Value, threeMonths: Value, sixMonths: Value, oneYear: Value
    public let allTime: Value?
    public init(oneMonth: Value, threeMonths: Value, sixMonths: Value, oneYear: Value, allTime: Value?) {
        self.oneMonth = oneMonth; self.threeMonths = threeMonths; self.sixMonths = sixMonths; self.oneYear = oneYear; self.allTime = allTime
    }
}

extension StatsRanges: Codable where Value: Codable {}

/// `hours` (V5.3) with H7's extras: each range is 24 hours, hour 0 first.
public struct StatsHours: Sendable, Hashable, Codable {
    public let hours: StatsRanges<[StatsHour]>
    public let retentionByHour: StatsRanges<[StatsHourRetention]>
    public init(hours: StatsRanges<[StatsHour]>, retentionByHour: StatsRanges<[StatsHourRetention]>) {
        self.hours = hours; self.retentionByHour = retentionByHour
    }
}

/// One range of `buttons` (V6.2): presses of buttons 1 to 4 for learning, young and mature cards.
public struct StatsButtonPresses: Sendable, Hashable, Codable {
    public let learning: [Int], young: [Int], mature: [Int]
    public init(learning: [Int], young: [Int], mature: [Int]) {
        self.learning = learning; self.young = young; self.mature = mature
    }
}

/// `buttons` (V6.2) with H7's `sixMonths`.
public struct StatsButtons: Sendable, Hashable, Codable {
    public let ranges: StatsRanges<StatsButtonPresses>
    public init(ranges: StatsRanges<StatsButtonPresses>) {
        self.ranges = ranges
    }
}

/// `future_due` (`stats/graphs-cards` C3.6) with H8's `future_due_young`.
public struct StatsFutureDue: Sendable, Hashable, Codable {
    /// Day offset from today → cards due.
    public let due: StatsSparseMap<Int>
    public let haveBacklog: Bool
    public let dailyLoad: Int
    public let dueTomorrow: Int
    /// The same keys' cards with an interval under 21 days (H8).
    public let dueYoung: StatsSparseMap<Int>
    public init(due: StatsSparseMap<Int>, haveBacklog: Bool, dailyLoad: Int, dueTomorrow: Int, dueYoung: StatsSparseMap<Int>) {
        self.due = due; self.haveBacklog = haveBacklog; self.dailyLoad = dailyLoad; self.dueTomorrow = dueTomorrow; self.dueYoung = dueYoung
    }
}

/// The seven counts of one Card Counts set (C4.1).
public struct StatsCardStateCounts: Sendable, Hashable, Codable {
    public let new: Int, learning: Int, relearning: Int, young: Int, mature: Int, suspended: Int, buried: Int
    public init(new: Int, learning: Int, relearning: Int, young: Int, mature: Int, suspended: Int, buried: Int) {
        self.new = new; self.learning = learning; self.relearning = relearning; self.young = young; self.mature = mature; self.suspended = suspended; self.buried = buried
    }
}

/// A Card Counts row's label (`stats/browse-queries` Q4.1), in the table's order.
public enum StatsCountLabel: Int, Sendable, Hashable, Codable, CaseIterable {
    case new = 0, learning, relearning, young, mature, suspended, buried
}

/// A label shown, with its item's search text.
public struct StatsCountLabelText: Sendable, Hashable, Codable {
    public let label: StatsCountLabel
    public let itemText: String
    public init(label: StatsCountLabel, itemText: String) {
        self.label = label; self.itemText = itemText
    }
}

/// `card_counts` (C4.4) with H8's `suspended_by_state`.
public struct StatsCardCounts: Sendable, Hashable, Codable {
    public let excludingInactive: StatsCardStateCounts
    public let includingInactive: StatsCardStateCounts
    public let notes: Int
    /// The labels shown with `cardCountsSeparateInactive` true, then false.
    public let labelsSeparate: [StatsCountLabelText]
    public let labelsTogether: [StatsCountLabelText]
    /// The suspended cards by state (H8).
    public let suspendedByState: StatsStateCounts
    public init(excludingInactive: StatsCardStateCounts, includingInactive: StatsCardStateCounts, notes: Int, labelsSeparate: [StatsCountLabelText], labelsTogether: [StatsCountLabelText], suspendedByState: StatsStateCounts) {
        self.excludingInactive = excludingInactive; self.includingInactive = includingInactive; self.notes = notes; self.labelsSeparate = labelsSeparate; self.labelsTogether = labelsTogether; self.suspendedByState = suspendedByState
    }
}

/// `stability` (C7): rounded stability in days → cards.
public struct StatsStability: Sendable, Hashable, Codable {
    public let byDays: StatsSparseMap<Int>
    public let leftOut: Int
    public init(byDays: StatsSparseMap<Int>, leftOut: Int) {
        self.byDays = byDays; self.leftOut = leftOut
    }
}

/// `difficulty` (C8): percent → cards, and C8.2's median.
public struct StatsDifficulty: Sendable, Hashable, Codable {
    public let byPercent: StatsSparseMap<Int>
    public let median: Float
    public let leftOut: Int
    public init(byPercent: StatsSparseMap<Int>, median: Float, leftOut: Int) {
        self.byPercent = byPercent; self.median = median; self.leftOut = leftOut
    }
}

/// `retrievability` (C9.7): percent → cards.
public struct StatsRetrievability: Sendable, Hashable, Codable {
    public let byPercent: StatsSparseMap<Int>
    public let average: Float
    public let sumByCard: Float
    public let sumByNote: Float
    public let leftOut: Int
    public init(byPercent: StatsSparseMap<Int>, average: Float, sumByCard: Float, sumByNote: Float, leftOut: Int) {
        self.byPercent = byPercent; self.average = average; self.sumByCard = sumByCard; self.sumByNote = sumByNote; self.leftOut = leftOut
    }
}

// MARK: - The page's own parts

/// `by_answer` (H6): the rows `reviews` counts with an answer 1 to 4, on its day keys.
public struct StatsByAnswer: Sendable, Hashable, Codable {
    public let count: StatsSparseMap<StatsAnswers>
    public let time: StatsSparseMap<StatsAnswers>
    /// The rows `reviews` counts with no such answer, in the window.
    public let unrated: Int
    public init(count: StatsSparseMap<StatsAnswers>, time: StatsSparseMap<StatsAnswers>, unrated: Int) {
        self.count = count; self.time = time; self.unrated = unrated
    }
}

/// `card_states_over_time` (H2.4).
public struct StatsCardStatesOverTime: Sendable, Hashable, Codable {
    /// The counts at the end of the day before the window's first day.
    public let start: StatsStateCounts
    /// The change of the counts on each day that has one (H1 days).
    public let deltas: StatsSparseMap<StatsStateCounts>
    /// The scope's suspended cards, left out of every day (H2.1).
    public let leftOut: Int
    /// The oldest day any card of the series enters.
    public let firstDay: Int?
    public init(start: StatsStateCounts, deltas: StatsSparseMap<StatsStateCounts>, leftOut: Int, firstDay: Int?) {
        self.start = start; self.deltas = deltas; self.leftOut = leftOut; self.firstDay = firstDay
    }

    /// The counts at the end of `day`: `start` plus the deltas up to it.
    public func counts(on day: Int) -> StatsStateCounts {
        zip(deltas.keys, deltas.values).reduce(start) { counts, entry in entry.0 <= day ? counts + entry.1 : counts }
    }
}

/// `started` (H2.4): cards first reviewed per day of the window.
public struct StatsStarted: Sendable, Hashable, Codable {
    public let days: StatsSparseMap<Int>
    /// The oldest such day in all history.
    public let firstDay: Int?
    public init(days: StatsSparseMap<Int>, firstDay: Int?) {
        self.days = days; self.firstDay = firstDay
    }
}

/// `studied_today` (H12): the strip's numbers for the page scope.
public struct StatsStudiedToday: Sendable, Hashable, Codable {
    public let answers: Int
    public let cards: Int
    public let timeMilliseconds: Int
    public init(answers: Int, cards: Int, timeMilliseconds: Int) {
        self.answers = answers; self.cards = cards; self.timeMilliseconds = timeMilliseconds
    }
}

/// One day of `retention_by_day` (H4.2).
public struct StatsRetentionDay: Sendable, Hashable, Codable {
    public let youngPassed: Int, youngFailed: Int, maturePassed: Int, matureFailed: Int
    /// The retention in force of each counted row's card, summed (H3.4).
    public let desiredSum: Double
    /// The rows in `desiredSum`.
    public let desiredRows: Int
    public init(youngPassed: Int, youngFailed: Int, maturePassed: Int, matureFailed: Int, desiredSum: Double, desiredRows: Int) {
        self.youngPassed = youngPassed; self.youngFailed = youngFailed; self.maturePassed = maturePassed; self.matureFailed = matureFailed; self.desiredSum = desiredSum; self.desiredRows = desiredRows
    }
    /// Forgotten on the day.
    public var forgotten: Int { youngFailed + matureFailed }
}

/// `retention_by_day` (H3.4, H4.2).
public struct StatsRetentionByDay: Sendable, Hashable, Codable {
    public let days: StatsSparseMap<StatsRetentionDay>
    /// The distinct retentions in force among the scope's home decks, ascending.
    public let retentions: [Float]
    /// More than one: the dashed line is an average.
    public let several: Bool
    public init(days: StatsSparseMap<StatsRetentionDay>, retentions: [Float], several: Bool) {
        self.days = days; self.retentions = retentions; self.several = several
    }
}

/// One window of a By Deck row (H10.2).
public struct StatsDeckValues: Sendable, Hashable, Codable {
    public let reviews: Int
    public let timeMilliseconds: Int
    public let passed: Int
    public let failed: Int
    public init(reviews: Int, timeMilliseconds: Int, passed: Int, failed: Int) {
        self.reviews = reviews; self.timeMilliseconds = timeMilliseconds; self.passed = passed; self.failed = failed
    }
}

/// A row of `by_deck` (H10).
public struct StatsDeckRow: Sendable, Hashable, Codable {
    public let deckID: Int64
    /// The deck's own cards, outside its subdecks (the first row of a deck's table).
    public let ownCards: Bool
    public let cards: Int
    /// `allTime` only with all history.
    public let windows: StatsRanges<StatsDeckValues>
    public init(deckID: Int64, ownCards: Bool, cards: Int, windows: StatsRanges<StatsDeckValues>) {
        self.deckID = deckID; self.ownCards = ownCards; self.cards = cards; self.windows = windows
    }
}

/// `by_deck` (H10): rows in the deck tree's order.
public struct StatsByDeck: Sendable, Hashable, Codable {
    public let rows: [StatsDeckRow]
    public init(rows: [StatsDeckRow]) {
        self.rows = rows
    }
}

/// One sample of `knowledge_over_time` (H3.3).
public struct StatsKnowledgeSample: Sendable, Hashable, Codable {
    /// H1's day: −365 to −1 for a day end, earlier for a month end (All only), 0 for today's point.
    public let day: Int
    /// The sample instant, epoch milliseconds.
    public let instantMilliseconds: Int64
    /// The summed recall probabilities of the cards (each in [0, 1]); never above `studied`.
    public let knowledge: Double
    /// The cards with an estimate there.
    public let studied: Int
    public init(day: Int, instantMilliseconds: Int64, knowledge: Double, studied: Int) {
        self.day = day; self.instantMilliseconds = instantMilliseconds; self.knowledge = knowledge; self.studied = studied
    }
}

/// `knowledge_over_time` (H3.3).
public struct StatsKnowledgeOverTime: Sendable, Hashable, Codable {
    /// Ascending by instant, today's point last; empty when no card has a memory estimate (S8.5).
    public let samples: [StatsKnowledgeSample]
    /// The cards left out for a crate error, from the first sample they failed at on.
    public let failed: Int
    /// The scope's suspended cards, left out of every sample (H2.1).
    public let leftOut: Int
    public init(samples: [StatsKnowledgeSample], failed: Int, leftOut: Int) {
        self.samples = samples; self.failed = failed; self.leftOut = leftOut
    }
}

/// A part's value. A part whose value type has no case yet arrives as `StatsPartFailure.undecodable`.
public enum StatsPartValue: Sendable, Hashable, Codable {
    case cardCounts(StatsCardCounts)
    case futureDue(StatsFutureDue)
    /// Interval in days → cards (C5).
    case intervals(StatsSparseMap<Int>)
    case stability(StatsStability)
    case difficulty(StatsDifficulty)
    case retrievability(StatsRetrievability)
    case reviews(StatsReviews)
    case byAnswer(StatsByAnswer)
    case retention(StatsTrueRetention)
    case hours(StatsHours)
    case buttons(StatsButtons)
    /// Creation day → cards (C10).
    case added(StatsSparseMap<Int>)
    case cardStatesOverTime(StatsCardStatesOverTime)
    case started(StatsStarted)
    case knowledgeOverTime(StatsKnowledgeOverTime)
    case studiedToday(StatsStudiedToday)
    case retentionByDay(StatsRetentionByDay)
    case byDeck(StatsByDeck)
    /// H1 day → AH2's rows, every year (H11).
    case activityDays(StatsSparseMap<Int>)
}

// MARK: - Decoding

extension StatsPartValue {
    /// The value of `part` from its G6 bytes.
    static func decode(_ part: StatsPart, _ data: Data) throws(StatsBytes.Failure) -> StatsPartValue {
        var bytes = StatsBytes(data)
        let value: StatsPartValue
        switch part {
        case .cardCounts: value = .cardCounts(try .decode(&bytes))
        case .futureDue: value = .futureDue(try .decode(&bytes))
        case .intervals: value = .intervals(try bytes.counts())
        case .stability: value = .stability(StatsStability(byDays: try bytes.counts(), leftOut: try bytes.count()))
        case .difficulty:
            value = .difficulty(StatsDifficulty(byPercent: try bytes.counts(), median: try bytes.float32(),
                                                leftOut: try bytes.count()))
        case .retrievability:
            value = .retrievability(StatsRetrievability(byPercent: try bytes.counts(), average: try bytes.float32(),
                                                        sumByCard: try bytes.float32(), sumByNote: try bytes.float32(),
                                                        leftOut: try bytes.count()))
        case .reviews: value = .reviews(try .decode(&bytes))
        case .byAnswer: value = .byAnswer(try .decode(&bytes))
        case .retention: value = .retention(try .decode(&bytes))
        case .hours: value = .hours(try .decode(&bytes))
        case .buttons: value = .buttons(try .decode(&bytes))
        case .added: value = .added(try bytes.counts())
        case .cardStatesOverTime: value = .cardStatesOverTime(try .decode(&bytes))
        case .started: value = .started(StatsStarted(days: try bytes.counts(), firstDay: try bytes.optional { b throws(StatsBytes.Failure) in try b.integer() }))
        case .knowledgeOverTime: value = .knowledgeOverTime(try .decode(&bytes))
        case .studiedToday:
            value = .studiedToday(StatsStudiedToday(answers: try bytes.count(), cards: try bytes.count(),
                                                    timeMilliseconds: try bytes.integer()))
        case .retentionByDay: value = .retentionByDay(try .decode(&bytes))
        case .byDeck: value = .byDeck(try .decode(&bytes))
        case .activityDays: value = .activityDays(try bytes.counts())
        }
        try bytes.finish()
        return value
    }
}

extension StatsReviewKinds {
    init(_ values: ArraySlice<Int>) {
        let v = Array(values)
        self.init(learn: v[0], relearn: v[1], young: v[2], mature: v[3], filtered: v[4])
    }
}

extension StatsAnswers {
    init(_ values: ArraySlice<Int>) {
        let v = Array(values)
        self.init(again: v[0], hard: v[1], good: v[2], easy: v[3])
    }
}

extension StatsStateCounts {
    init(_ values: [Int]) {
        self.init(new: values[0], learning: values[1], relearning: values[2], young: values[3], mature: values[4])
    }

    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsStateCounts {
        StatsStateCounts([try bytes.integer(), try bytes.integer(), try bytes.integer(), try bytes.integer(),
                          try bytes.integer()])
    }
}

extension StatsReviews {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsReviews {
        var counts: [StatsReviewKinds] = []
        var times: [StatsReviewKinds] = []
        let map = try bytes.map(members: 10) { $0 }
        for row in map.values {
            counts.append(StatsReviewKinds(row[0..<5]))
            times.append(StatsReviewKinds(row[5..<10]))
        }
        return StatsReviews(count: StatsSparseMap(keys: map.keys, values: counts),
                            time: StatsSparseMap(keys: map.keys, values: times))
    }
}

extension StatsByAnswer {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsByAnswer {
        let map = try bytes.map(members: 8) { $0 }
        return StatsByAnswer(count: StatsSparseMap(keys: map.keys, values: map.values.map { StatsAnswers($0[0..<4]) }),
                             time: StatsSparseMap(keys: map.keys, values: map.values.map { StatsAnswers($0[4..<8]) }),
                             unrated: try bytes.count())
    }
}

extension StatsRetentionCounts {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsRetentionCounts {
        StatsRetentionCounts(youngPassed: try bytes.count(), youngFailed: try bytes.count(),
                             maturePassed: try bytes.count(), matureFailed: try bytes.count())
    }
}

extension StatsTrueRetention {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsTrueRetention {
        let periods = try bytes.list { b throws(StatsBytes.Failure) in try StatsRetentionCounts.decode(&b) }
        guard periods.count == 5 || periods.count == 6 else {
            throw StatsBytes.Failure(reason: "\(periods.count) retention periods")
        }
        return StatsTrueRetention(today: periods[0], yesterday: periods[1], week: periods[2], month: periods[3],
                                  year: periods[4], allTime: periods.count == 6 ? periods[5] : nil)
    }
}

extension StatsRanges {
    /// V5.3's and V6.2's list of `oneMonth`, `threeMonths`, `oneYear` and, with all history,
    /// `allTime`, then H7's `sixMonths` alone.
    static func decodeAnkiThenSixMonths(_ bytes: inout StatsBytes,
                                        _ item: (inout StatsBytes) throws(StatsBytes.Failure) -> Value)
        throws(StatsBytes.Failure) -> StatsRanges {
        let listed = try bytes.list(item)
        guard listed.count == 3 || listed.count == 4 else { throw StatsBytes.Failure(reason: "\(listed.count) ranges") }
        let sixMonths = try item(&bytes)
        return StatsRanges(oneMonth: listed[0], threeMonths: listed[1], sixMonths: sixMonths, oneYear: listed[2],
                           allTime: listed.count == 4 ? listed[3] : nil)
    }

    /// H7's list of the five ranges in time order: `oneMonth`, `threeMonths`, `sixMonths`,
    /// `oneYear` and, with all history, `allTime`.
    static func decodeInOrder(_ bytes: inout StatsBytes,
                              _ item: (inout StatsBytes) throws(StatsBytes.Failure) -> Value)
        throws(StatsBytes.Failure) -> StatsRanges {
        let listed = try bytes.list(item)
        guard listed.count == 4 || listed.count == 5 else { throw StatsBytes.Failure(reason: "\(listed.count) ranges") }
        return StatsRanges(oneMonth: listed[0], threeMonths: listed[1], sixMonths: listed[2], oneYear: listed[3],
                           allTime: listed.count == 5 ? listed[4] : nil)
    }
}

extension StatsHours {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsHours {
        let hours = try StatsRanges<[StatsHour]>.decodeAnkiThenSixMonths(&bytes) { b throws(StatsBytes.Failure) in
            try b.list(of: 24) { h throws(StatsBytes.Failure) in StatsHour(total: try h.count(), correct: try h.count()) }
        }
        let retention = try StatsRanges<[StatsHourRetention]>.decodeInOrder(&bytes) { b throws(StatsBytes.Failure) in
            try b.list(of: 24) { h throws(StatsBytes.Failure) in
                StatsHourRetention(passed: try h.count(), failed: try h.count())
            }
        }
        guard (hours.allTime == nil) == (retention.allTime == nil) else {
            throw StatsBytes.Failure(reason: "an all-time range in one list only")
        }
        return StatsHours(hours: hours, retentionByHour: retention)
    }
}

extension StatsButtons {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsButtons {
        let presses: (inout StatsBytes) throws(StatsBytes.Failure) -> [Int] = { b throws(StatsBytes.Failure) in
            try b.list(of: 4) { p throws(StatsBytes.Failure) in try p.count() }
        }
        return StatsButtons(ranges: try .decodeAnkiThenSixMonths(&bytes) { b throws(StatsBytes.Failure) in
            StatsButtonPresses(learning: try presses(&b), young: try presses(&b), mature: try presses(&b))
        })
    }
}

extension StatsFutureDue {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsFutureDue {
        StatsFutureDue(due: try bytes.counts(), haveBacklog: try bytes.flag(), dailyLoad: try bytes.count(),
                       dueTomorrow: try bytes.count(), dueYoung: try bytes.counts())
    }
}

extension StatsCardStateCounts {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsCardStateCounts {
        StatsCardStateCounts(new: try bytes.count(), learning: try bytes.count(), relearning: try bytes.count(),
                             young: try bytes.count(), mature: try bytes.count(), suspended: try bytes.count(),
                             buried: try bytes.count())
    }
}

extension StatsCardCounts {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsCardCounts {
        let labels: (inout StatsBytes) throws(StatsBytes.Failure) -> [StatsCountLabelText] = { b throws(StatsBytes.Failure) in
            try b.list { l throws(StatsBytes.Failure) in
                let index = try l.integer()
                guard let label = StatsCountLabel(rawValue: index) else {
                    throw StatsBytes.Failure(reason: "a Card Counts label \(index)")
                }
                return StatsCountLabelText(label: label, itemText: try l.text())
            }
        }
        return StatsCardCounts(excludingInactive: try .decode(&bytes), includingInactive: try .decode(&bytes),
                               notes: try bytes.count(), labelsSeparate: try labels(&bytes),
                               labelsTogether: try labels(&bytes), suspendedByState: try .decode(&bytes))
    }
}

extension StatsCardStatesOverTime {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsCardStatesOverTime {
        let start = try StatsStateCounts.decode(&bytes)
        let deltas = try bytes.map(members: 5) { StatsStateCounts($0) }
        return StatsCardStatesOverTime(start: start, deltas: deltas, leftOut: try bytes.count(),
                                       firstDay: try bytes.optional { b throws(StatsBytes.Failure) in try b.integer() })
    }
}

extension StatsKnowledgeOverTime {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsKnowledgeOverTime {
        let samples = try bytes.list { b throws(StatsBytes.Failure) in
            StatsKnowledgeSample(day: try b.integer(), instantMilliseconds: Int64(try b.integer()),
                                 knowledge: try b.float64(), studied: try b.count())
        }
        return StatsKnowledgeOverTime(samples: samples, failed: try bytes.count(), leftOut: try bytes.count())
    }
}

extension StatsRetentionByDay {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsRetentionByDay {
        let map = try bytes.map(members: 5) { $0 }
        let sums = try bytes.list { b throws(StatsBytes.Failure) in try b.float64() }
        guard sums.count == map.count else { throw StatsBytes.Failure(reason: "\(sums.count) sums for \(map.count) days") }
        var days: [StatsRetentionDay] = []
        for (at, row) in map.values.enumerated() {
            guard row.allSatisfy({ $0 >= 0 }) else { throw StatsBytes.Failure(reason: "a negative count") }
            days.append(StatsRetentionDay(youngPassed: row[0], youngFailed: row[1], maturePassed: row[2],
                                          matureFailed: row[3], desiredSum: sums[at], desiredRows: row[4]))
        }
        return StatsRetentionByDay(days: StatsSparseMap(keys: map.keys, values: days),
                                   retentions: try bytes.list { b throws(StatsBytes.Failure) in try b.float32() },
                                   several: try bytes.flag())
    }
}

extension StatsDeckValues {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsDeckValues {
        StatsDeckValues(reviews: try bytes.count(), timeMilliseconds: try bytes.integer(), passed: try bytes.count(),
                        failed: try bytes.count())
    }
}

extension StatsByDeck {
    static func decode(_ bytes: inout StatsBytes) throws(StatsBytes.Failure) -> StatsByDeck {
        StatsByDeck(rows: try bytes.list { b throws(StatsBytes.Failure) in
            let deck = Int64(try b.integer())
            let own = try b.flag()
            let cards = try b.count()
            let oneMonth = try StatsDeckValues.decode(&b), threeMonths = try StatsDeckValues.decode(&b)
            let sixMonths = try StatsDeckValues.decode(&b), oneYear = try StatsDeckValues.decode(&b)
            let allTime = try b.optional { v throws(StatsBytes.Failure) in try StatsDeckValues.decode(&v) }
            return StatsDeckRow(deckID: deck, ownCards: own, cards: cards,
                                windows: StatsRanges(oneMonth: oneMonth, threeMonths: threeMonths, sixMonths: sixMonths,
                                                     oneYear: oneYear, allTime: allTime))
        })
    }
}
