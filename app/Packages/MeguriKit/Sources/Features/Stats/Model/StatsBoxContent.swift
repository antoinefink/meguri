import CoreBridge
import Foundation
import HeatmapKit

/// The page's boxes (`stats/statistics-page` S5), in page order; the raw value is S15's
/// `stats.box.<box>`.
nonisolated public enum StatsBoxID: String, CaseIterable, Hashable, Sendable {
    case knowledgeOverTime, retention, forgotten
    case reviewsOverTime, activity
    case cardsOverTime, cardCounts, addedAndStarted
    case reviewsDue
    case answers, timeOfDay
    case retrievability, stability, difficulty
    case byDeck

    /// Whether the box's content reads `part`: its own parts, and the extra values it takes
    /// from the newest set that has them (Retrievability's desired retention, Reviews' streak).
    func reads(_ part: StatsPart, reviewsMode: StatsReviewsMode) -> Bool {
        if parts(reviewsMode: reviewsMode).contains(part) { return true }
        switch self {
        case .retrievability: return part == .retentionByDay
        case .reviewsOverTime: return part == .activityDays
        default: return false
        }
    }

    /// The parts the box draws, all from one refresh.
    func parts(reviewsMode: StatsReviewsMode) -> [StatsPart] {
        switch self {
        case .knowledgeOverTime: [.knowledgeOverTime]
        case .retention, .forgotten: [.retentionByDay]
        case .reviewsOverTime: reviewsMode == .byAnswer ? [.reviews, .byAnswer] : [.reviews]
        case .activity: [.activityDays]
        case .cardsOverTime: [.cardStatesOverTime]
        case .cardCounts: [.cardCounts]
        case .addedAndStarted: [.added, .started]
        case .reviewsDue: [.futureDue]
        case .answers: [.buttons]
        case .timeOfDay: [.hours]
        case .retrievability: [.retrievability]
        case .stability: [.stability, .intervals]
        case .difficulty: [.difficulty]
        case .byDeck: [.byDeck]
        }
    }
}

/// The strip's figures (S6).
nonisolated public enum StatsStripID: String, CaseIterable, Hashable, Sendable {
    case studiedToday, retentionToday, remainingToday, streak, mature

    var parts: [StatsPart] {
        switch self {
        case .studiedToday: [.studiedToday]
        case .retentionToday: [.retention]
        case .remainingToday: [.futureDue]
        case .streak: [.activityDays]
        case .mature: [.cardCounts]
        }
    }
}

/// What a box's set was read for: the context Show in Browse passes back (H13) and the scope's
/// browse text (Q2).
nonisolated public struct StatsBoxSource: Hashable, Sendable {
    public let scope: StatsPageScope
    public let context: StatsContext
    public let browseScope: StatsBrowseScope
}

/// Everything one box shows (N25.4), written by the model; the box's view only lays it out.
nonisolated public struct StatsBoxContent: Hashable, Sendable {
    /// Ready, loading (with the core's fraction), empty with its line, or failed (S8.5, S10.3,
    /// S12.8). A partial value (a `last12Months` result on All's axis) is `.loading` with its
    /// data kept in `data`: never shown as complete.
    public var phase: StatsBoxPhase
    public var figure: StatsBoxFigure?
    public var data: StatsChartData
    /// The day span of each bin of `data` (H1 days), for Show in Browse (H13) and the chart's page.
    public var binDays: [StatsAxisBin]
    public var figures: [StatsSmallFigure]
    public var footnote: String?
    /// The box's own extra values (tables, the second panel, the Activity grid).
    public var detail: StatsBoxDetail?
    /// A newer refresh is running and has not returned this box's parts yet (S10.2's refresh in place).
    public var updating: Bool
    /// What the shown values were read for; nil while nothing is shown.
    public var source: StatsBoxSource?

    public init(phase: StatsBoxPhase, figure: StatsBoxFigure? = nil, data: StatsChartData,
                binDays: [StatsAxisBin] = [], figures: [StatsSmallFigure] = [], footnote: String? = nil,
                detail: StatsBoxDetail? = nil, updating: Bool = false, source: StatsBoxSource? = nil) {
        self.phase = phase
        self.figure = figure
        self.data = data
        self.binDays = binDays
        self.figures = figures
        self.footnote = footnote
        self.detail = detail
        self.updating = updating
        self.source = source
    }
}

/// A box's values beyond its chart.
nonisolated public enum StatsBoxDetail: Hashable, Sendable {
    case cardCounts(StatsCardCountsTable)
    case answers(StatsAnswersTable)
    /// Time of Day's upper panel, the share remembered per hour (S7.11); `data` is the lower.
    case timeOfDay(retention: StatsChartData)
    case byDeck(StatsByDeckTable)
    case activity(StatsActivity)
    /// Activity's span while its part is not back yet or has failed, from the header's oldest
    /// review (H9), so the box is drawn at its final height for its width (S10.3, S8.5).
    case activitySpan(StatsActivitySpan)
    /// The desired retention the Memory boxes mark on their axis (S7.12), as a fraction.
    case retrievability(desiredRetention: Double?)
}

/// Card Counts' table (S7.7).
nonisolated public struct StatsCardCountsTable: Hashable, Sendable {
    public struct Row: Hashable, Sendable, Identifiable {
        public var id: StatsCountLabel { label }
        public let label: StatsCountLabel
        public let name: String
        public let color: StatsSeriesColor
        public let count: Int
        public let countText: String
        /// Its share of the total, 0 to 1.
        public let share: Double
        public let shareText: String
        /// Q4.1's search text for Show in Browse.
        public let itemText: String?
    }

    public let rows: [Row]
    public let total: String
    public let notes: String
}

/// Answers' table (S7.10): one row per group, Again to Easy.
nonisolated public struct StatsAnswersTable: Hashable, Sendable {
    public struct Row: Hashable, Sendable, Identifiable {
        public var id: String { name }
        public let name: String
        public let counts: [Int]
        public let countTexts: [String]
        public let shareTexts: [String]
    }

    public let rows: [Row]
}

/// By Deck's table (S7.15).
nonisolated public struct StatsByDeckTable: Hashable, Sendable {
    public struct Row: Hashable, Sendable, Identifiable {
        public var id: String { "\(deckID)-\(ownCards)" }
        public let deckID: Int64
        public let name: String
        /// The deck's own cards: the row is the deck shown and does not filter (S7.15).
        public let ownCards: Bool
        public let cards: Int
        public let reviews: Int
        /// passed ÷ (passed + failed), nil with none.
        public let retention: Double?
        public let timeShare: Double
        public let cardsText: String
        public let reviewsText: String
        public let retentionText: String
        public let timeShareText: String
    }

    public let rows: [Row]
    /// "This deck has no subdecks" for a deck without subdecks.
    public let note: String?
}

/// The days Activity's grid runs over (S7.5): from the scope's first review to today, known
/// from the header (H9) before the counts return, so the grid's size is final from the first
/// frame (S10.3).
nonisolated public struct StatsActivitySpan: Hashable, Sendable {
    /// The first day drawn (H1, at most 0): the scope's oldest review.
    public let firstDay: Int
    /// Today's Gregorian date (day 0).
    public let todayYear: Int
    public let todayMonth: Int
    public let todayDay: Int
    /// The display calendar's first weekday, 1 Sunday to 7 Saturday (S7.5: the system's).
    public let firstWeekday: Int
    /// The Gregorian years drawn, the current one first.
    public let years: [Int]

    /// Days drawn, the first day through today.
    public var dayCount: Int { 1 - firstDay }

    init(firstDay: Int, days: StatsDays) {
        self.firstDay = min(firstDay, 0)
        let today = days.gregorianDate(ofDay: 0)
        todayYear = today.year
        todayMonth = today.month
        todayDay = today.day
        firstWeekday = days.display.firstWeekday
        let first = days.year(ofDay: self.firstDay)
        years = Array(stride(from: today.year, through: min(first, today.year), by: -1))
    }

    /// The grid with these days and `counts` (day → reviews, H1 days).
    public func grid(counts: [Int: Int] = [:]) -> ActivityContinuousGrid {
        var dense = [UInt32](repeating: 0, count: counts.isEmpty ? 0 : dayCount)
        for (day, count) in counts where day >= firstDay && day <= 0 { dense[-day] = UInt32(clamping: count) }
        return ActivityContinuousGrid(year: todayYear, month: todayMonth, day: todayDay, days: dayCount,
                                      firstWeekday: firstWeekday, counts: dense)
    }
}

nonisolated extension StatsPagePeriod {
    /// The days Activity's grid shows ending today, as the charts with no time axis read their
    /// window (S4.3, H1's 30, 90, 182 and 365 days); nil for All, every day back to the first
    /// review (Antoine, 2026-11-23: "with 3M selected it showed two years").
    public var activityWindowDays: Int? {
        switch self {
        case .oneMonth: 30
        case .threeMonths: 90
        case .sixMonths: 182
        case .oneYear: 365
        case .all: nil
        }
    }

    /// The first day (H1) of the window, from `oldest`: the later of the two.
    public func activityFirstDay(oldest: Int) -> Int {
        max(min(oldest, 0), activityWindowDays.map { 1 - $0 } ?? Int.min)
    }
}

/// Activity's grid, streaks and totals (S7.5).
nonisolated public struct StatsActivity: Hashable, Sendable {
    /// A year's total, beside its label on the grid (the owner's Option B, 2026-11-23) and in
    /// its container's label for VoiceOver.
    nonisolated public struct Year: Hashable, Sendable, Identifiable {
        public var id: Int { year }
        public let year: Int
        public let total: Int
        /// "2025: 41,233 reviews": the year's container label.
        public let totalText: String
        /// After the year on the grid: "· 41,233 reviews", or the number alone, "· 41,233".
        public var totalFull: String = ""
        public var totalNumber: String = ""
    }

    public let span: StatsActivitySpan
    /// The grid over the span, its counts and AH7's levels over every day drawn.
    public let grid: ActivityContinuousGrid
    /// Day (H1) → reviews, only days with some.
    public let counts: [Int: Int]
    /// The current year first, then every earlier year back to the scope's first review.
    public let years: [Year]
    public let currentStreak: Int
    public let longestStreak: Int
    public let daysStudiedThisYear: Int
    /// The days of this year so far, today included (the "<D>" of "studied <d> of <D> days").
    public var daysThisYear: Int = 1
    /// Reviews in all history.
    public let total: Int
    /// The grid shows a period's window, not all history (1M to 1Y): the narrow page shows it
    /// whole, with no Show All Years.
    public var windowed = false
    let days: StatsDays

    init(span: StatsActivitySpan, counts: [Int: Int], years: [Year], currentStreak: Int, longestStreak: Int,
         daysStudiedThisYear: Int, days: StatsDays) {
        self.span = span
        grid = span.grid(counts: counts)
        self.counts = counts
        self.years = years
        self.currentStreak = currentStreak
        self.longestStreak = longestStreak
        self.daysStudiedThisYear = daysStudiedThisYear
        total = counts.values.reduce(0, +)
        self.days = days
    }

    /// The date of a day (H1), for the callout and the days' elements.
    public func date(ofDay day: Int) -> Date { days.date(ofDay: day) }
}

/// One figure of the strip (S6): its value with a unit in small text and a detail, or the
/// progress indicator while its part has not returned (S10.1).
nonisolated public struct StatsStripFigure: Hashable, Sendable {
    public var value: String?
    public var unit: String?
    public var detail: String?
    /// Its part has not returned: the small progress indicator in place of the value.
    public var loading: Bool
    /// VoiceOver's reading of the value ("128 cards, 19 minutes").
    public var spoken: String?

    public static let loading = StatsStripFigure(value: nil, unit: nil, detail: nil, loading: true, spoken: nil)
}

/// What the page shows instead of its sections (S12).
nonisolated public enum StatsPageState: Hashable, Sendable {
    /// S12.1: no collection open; the shell's state, controls disabled.
    case noCollection
    /// S12.2: no cards; controls disabled.
    case noCards
    /// S12.7: the deck shown no longer exists.
    case deckMissing
    /// S12.8: an unreadable row in the scope; the strip and controls stay.
    case unavailable
    case content
}
