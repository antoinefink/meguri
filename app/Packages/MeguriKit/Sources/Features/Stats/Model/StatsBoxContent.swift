import CoreBridge
import Foundation

/// The page's boxes (`stats/statistics-page` S5), in page order; the raw value is S15's
/// `stats.box.<box>`.
public enum StatsBoxID: String, CaseIterable, Hashable, Sendable {
    case knowledgeOverTime, retention, forgotten
    case reviewsOverTime, activity
    case cardsOverTime, cardCounts, addedAndStarted
    case reviewsDue
    case answers, timeOfDay
    case retrievability, stability, difficulty
    case byDeck

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
public enum StatsStripID: String, CaseIterable, Hashable, Sendable {
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
public struct StatsBoxSource: Hashable, Sendable {
    public let scope: StatsPageScope
    public let context: StatsContext
    public let browseScope: StatsBrowseScope
}

/// Everything one box shows (N25.4), written by the model; the box's view only lays it out.
public struct StatsBoxContent: Hashable, Sendable {
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
public enum StatsBoxDetail: Hashable, Sendable {
    case cardCounts(StatsCardCountsTable)
    case answers(StatsAnswersTable)
    /// Time of Day's upper panel, the share remembered per hour (S7.11); `data` is the lower.
    case timeOfDay(retention: StatsChartData)
    case byDeck(StatsByDeckTable)
    case activity(StatsActivity)
    /// The desired retention the Memory boxes mark on their axis (S7.12), as a fraction.
    case retrievability(desiredRetention: Double?)
}

/// Card Counts' table (S7.7).
public struct StatsCardCountsTable: Hashable, Sendable {
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
public struct StatsAnswersTable: Hashable, Sendable {
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
public struct StatsByDeckTable: Hashable, Sendable {
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

/// Activity's year blocks and streaks (S7.5).
public struct StatsActivity: Hashable, Sendable {
    public struct Year: Hashable, Sendable, Identifiable {
        public var id: Int { year }
        public let year: Int
        /// The block's days (H1), first to last (the last is today in the current year).
        public let firstDay: Int
        public let lastDay: Int
        /// Day → reviews, only days with some.
        public let counts: [Int: Int]
        public let total: Int
        /// "2025: 41,233 reviews".
        public let totalText: String
    }

    /// The current year first, then every earlier year back to the scope's first review.
    public let years: [Year]
    public let currentStreak: Int
    public let longestStreak: Int
    public let daysStudiedThisYear: Int
    let days: StatsDays

    /// The date of a day (H1), for the grid's weeks and labels.
    public func date(ofDay day: Int) -> Date { days.date(ofDay: day) }
}

/// One figure of the strip (S6): its value with a unit in small text and a detail, or the
/// progress indicator while its part has not returned (S10.1).
public struct StatsStripFigure: Hashable, Sendable {
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
public enum StatsPageState: Hashable, Sendable {
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
