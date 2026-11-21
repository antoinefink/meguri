import AppModel
import CoreBridge
import Foundation

/// What Today shows in the sidebar layouts (contract N24.2, `stats/studied-today` T12.2):
/// the walk's decks and cards, the estimate, Studied Today and the previous session, from
/// what the collection already holds. Pure values and their texts; `TodayView` draws them.
public nonisolated struct TodaySummary: Equatable, Sendable {
    /// The walk's decks (N24.3): the top-level normal decks with cards waiting, in list order.
    public let decks: [StudyWalk.Deck]
    /// T12.2's W: their cards waiting.
    public let waiting: Int
    public let studied: StudiedToday?
    public let previous: PreviousSession?

    /// The previous session, a walk's or one deck's.
    public nonisolated struct PreviousSession: Equatable, Sendable {
        /// The decks' full names: one for a deck's session, the walk's answered decks in order.
        public let deckNames: [String]
        public let walk: Bool
        /// Different cards answered.
        public let cards: Int
        /// When it ended, or its last answer for a session that never ended.
        public let endedAt: Date
        public init(deckNames: [String], walk: Bool, cards: Int, endedAt: Date) {
            self.deckNames = deckNames; self.walk = walk; self.cards = cards; self.endedAt = endedAt
        }
    }

    public init(nodes: [CountedDeckNode], studied: StudiedToday?, previous: PreviousSession? = nil) {
        decks = StudyWalk.decks(from: nodes)
        waiting = decks.reduce(0) { $0 + $1.waiting }
        self.studied = studied
        self.previous = previous
    }

    /// Nothing waits in the walk's decks: Study gives way to the nothing-waiting line.
    public var nothingWaiting: Bool { decks.isEmpty }

    // MARK: Study's lines

    /// T12.2: W × the last seven days' time per answer, rounded up to whole minutes, at
    /// least 1; nil without an answer in those days or with nothing waiting.
    public var estimateMinutes: Int? {
        guard waiting > 0, let studied, studied.recentAnswers >= 1 else { return nil }
        let milliseconds = Double(waiting) * Double(max(0, studied.recentTimeMilliseconds)) / Double(studied.recentAnswers)
        return max(1, Int((milliseconds / 60_000).rounded(.up)))
    }

    /// "492 cards in 4 decks · about 35 min"; one deck: "40 cards in Languages · about 3 min".
    public func studyLine(locale: Locale) -> String {
        let cards = Self.cards(waiting, locale: locale)
        let total = decks.count == 1
            ? String(localized: "home.today.study.cardsInDeck", defaultValue: "\(cards) in \(Self.title(decks[0].name))",
                     bundle: .module, comment: "Study's first line with one deck: the cards waiting, then the deck's name.")
            : String(localized: "home.today.study.cardsInDecks", defaultValue: "\(cards) in \(Self.decksText(decks.count, locale: locale))",
                     bundle: .module, comment: "Study's first line: the cards waiting, then how many decks they are in.")
        guard let minutes = estimateMinutes else { return total }
        let time = Self.shortDuration(seconds: Int64(minutes) * 60, units: [.hour, .minute], locale: locale)
        return total + " · " + String(localized: "about \(time)", bundle: .module)
    }

    /// With more than one deck, the order: "Languages first, then Science, Culture and Music Theory".
    public func orderLine(locale: Locale) -> String? {
        guard decks.count > 1 else { return nil }
        let names = decks.map { Self.title($0.name) }
        let rest = Array(names.dropFirst()).formatted(.list(type: .and).locale(locale))
        return String(localized: "\(names[0]) first, then \(rest)", bundle: .module)
    }

    // MARK: Studied Today

    /// T12's different cards studied today; 0 before the first read.
    public var studiedCards: Int { studied.map { Int(clamping: $0.cards) } ?? 0 }

    /// The bar's value: today's cards over those and the walk's cards waiting; empty with both 0.
    public var studiedFraction: Double {
        let total = studiedCards + waiting
        return total == 0 ? 0 : Double(studiedCards) / Double(total)
    }

    /// "128 cards · 19 min", or "None yet".
    public func studiedValue(locale: Locale) -> String {
        guard studiedCards > 0, let studied else { return String(localized: "None yet", bundle: .module) }
        return Self.cards(studiedCards, locale: locale) + " · " + Self.shortTime(milliseconds: studied.timeMilliseconds, locale: locale)
    }

    /// VoiceOver's value: "128 cards, 19 minutes", or "None yet".
    public func studiedAccessibilityValue(locale: Locale) -> String {
        guard studiedCards > 0, let studied else { return String(localized: "None yet", bundle: .module) }
        let cards = Self.cards(studiedCards, locale: locale)
        let time = Self.time(milliseconds: studied.timeMilliseconds, width: .wide, locale: locale)
        return String(localized: "home.today.studied.spoken", defaultValue: "\(cards), \(time)", bundle: .module)
    }

    // MARK: Previous Session

    /// The deck's last name component with its parent ("Vocabulary in Español"), or the
    /// walk's decks in the list format ("Languages, Science and Culture").
    public func previousDecks(locale: Locale) -> String? {
        guard let previous, !previous.deckNames.isEmpty else { return nil }
        if previous.walk || previous.deckNames.count > 1 {
            return previous.deckNames.map(Self.title).formatted(.list(type: .and).locale(locale))
        }
        let path = previous.deckNames[0].components(separatedBy: "::")
        guard path.count > 1, let name = path.last else { return path.last }
        let parent = path[path.count - 2]
        return String(localized: "home.today.previous.deck", defaultValue: "\(name) in \(parent)", bundle: .module)
    }

    /// "46 cards · 8:12 AM": the short time today, the system's "Yesterday" with the time
    /// for the calendar day before, otherwise the date.
    public func previousValue(now: Date, calendar: Calendar, locale: Locale) -> String? {
        guard let previous else { return nil }
        return Self.cards(previous.cards, locale: locale) + " · " + Self.ended(previous.endedAt, now: now, calendar: calendar, locale: locale)
    }

    static func ended(_ date: Date, now: Date, calendar: Calendar, locale: Locale) -> String {
        if calendar.isDate(date, inSameDayAs: now) {
            return date.formatted(Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, calendar: calendar,
                                                   timeZone: calendar.timeZone))
        }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(date, inSameDayAs: yesterday) {
            let formatter = DateFormatter()
            formatter.locale = locale; formatter.calendar = calendar; formatter.timeZone = calendar.timeZone
            formatter.dateStyle = .medium; formatter.timeStyle = .short
            formatter.doesRelativeDateFormatting = true
            return formatter.string(from: date)
        }
        let formatter = DateFormatter()
        formatter.locale = locale; formatter.calendar = calendar; formatter.timeZone = calendar.timeZone
        formatter.dateStyle = .medium; formatter.timeStyle = .none
        return formatter.string(from: date)
    }

    // MARK: Pieces

    /// "1 card", "128 cards" (the catalog's plural forms).
    static func cards(_ count: Int, locale: Locale) -> String {
        String(localized: "\(count) cards", bundle: .module, locale: locale)
    }

    static func decksText(_ count: Int, locale: Locale) -> String {
        String(localized: "\(count) decks", bundle: .module, locale: locale)
    }

    /// A deck's last name component.
    static func title(_ fullName: String) -> String { fullName.components(separatedBy: "::").last ?? fullName }

    /// N16.5's time: whole seconds under a minute, else minutes rounded to the nearest;
    /// a negative sum shows as 0 (T12.2).
    /// The system's short unit form ("19 min", "40 sec", "1 hr, 5 min"), with N16.5's rounding.
    static func shortTime(milliseconds: Int64, locale: Locale) -> String {
        let summary = ReviewSessionSummary(cards: 0, seconds: UInt64(max(0, milliseconds)) / 1_000)
        if summary.seconds < 60 { return shortDuration(seconds: Int64(summary.seconds), units: [.second], locale: locale) }
        let minutes = Int64(summary.duration.components.seconds / 60)
        return shortDuration(seconds: minutes * 60, units: [.hour, .minute], locale: locale)
    }
    static func shortDuration(seconds: Int64, units: NSCalendar.Unit, locale: Locale) -> String {
        let formatter = DateComponentsFormatter()
        var calendar = Calendar.current; calendar.locale = locale
        formatter.calendar = calendar
        formatter.unitsStyle = .short; formatter.allowedUnits = units; formatter.zeroFormattingBehavior = .dropAll
        return formatter.string(from: TimeInterval(seconds)) ?? ""
    }
    static func time(milliseconds: Int64, width: Duration.UnitsFormatStyle.UnitWidth, locale: Locale) -> String {
        let summary = ReviewSessionSummary(cards: 0, seconds: UInt64(max(0, milliseconds)) / 1_000)
        let units: Set<Duration.UnitsFormatStyle.Unit> = summary.seconds < 60 ? [.seconds] : [.hours, .minutes]
        return summary.duration.formatted(.units(allowed: units, width: width).locale(locale))
    }
}
