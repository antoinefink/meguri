import Foundation
import Testing
@testable import CoreBridge
@testable import Home

/// N24.2 and `stats/studied-today` T12.2: what Today's summary says. SwiftPM builds do not
/// compile the string catalog, so these read the catalog's keys, which are its English values
/// in the plural "other" form.
@Suite struct TodaySummaryTests {
    private let locale = Locale(identifier: "en_US")

    private func node(_ id: Int64, _ name: String, _ counts: (UInt32, UInt32, UInt32), filtered: Bool = false,
                      _ children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: counts.0, learning: counts.1, review: counts.2), children: children)
    }

    private func studied(cards: UInt64 = 0, time: Int64 = 0, recentAnswers: UInt64 = 0, recentTime: Int64 = 0) -> StudiedToday {
        StudiedToday(answers: cards, cards: cards, timeMilliseconds: time, recentAnswers: recentAnswers,
                     recentTimeMilliseconds: recentTime)
    }

    /// The tree of N24.2's example: four decks waiting 492 cards, a filtered deck and an empty one.
    private var tree: [CountedDeckNode] {
        [node(1, "Languages", (20, 3, 177), [node(2, "Languages::Español", (20, 3, 177))]),
         node(3, "Science", (10, 2, 140)), node(9, "Quick Review", (0, 0, 30), filtered: true),
         node(4, "Culture", (0, 0, 40)), node(6, "Reading", (0, 0, 0)), node(5, "Music Theory", (5, 0, 95))]
    }

    @Test func theWalksDecksAreTheTopLevelNormalDecksWithCardsWaiting() {
        let summary = TodaySummary(nodes: tree, studied: nil)
        // Filtered and empty decks are left out, subdecks count under their top-level deck.
        #expect(summary.decks.map(\.id) == [1, 3, 4, 5])
        #expect(summary.waiting == 492)
        #expect(!summary.nothingWaiting)
        #expect(TodaySummary(nodes: [node(6, "Reading", (0, 0, 0)), node(9, "Quick Review", (0, 0, 30), filtered: true)],
                             studied: nil).nothingWaiting)
    }

    @Test func studysLinesNameTheCardsTheDecksAndTheOrder() {
        // 2,070,000 ms over 492 answers: 4,207 ms an answer, so 492 cards take 34.5 minutes, written 35.
        let summary = TodaySummary(nodes: tree, studied: studied(recentAnswers: 492, recentTime: 2_070_000))
        #expect(summary.estimateMinutes == 35)
        #expect(summary.studyLine(locale: locale) == "492 cards in 4 decks · about 35 min")
        // The system's list format: N24.2's example has no comma before the last name; the en_US format adds one.
        #expect(summary.orderLine(locale: Locale(identifier: "en_GB")) == "Languages first, then Science, Culture and Music Theory")
        #expect(summary.orderLine(locale: locale) == "Languages first, then Science, Culture, and Music Theory")
        // One deck names it, and has no order line.
        let one = TodaySummary(nodes: [node(1, "Languages", (10, 0, 30))], studied: studied(recentAnswers: 40, recentTime: 180_000))
        #expect(one.studyLine(locale: locale) == "40 cards in Languages · about 3 min")
        #expect(one.orderLine(locale: locale) == nil)
        // Two decks: "first, then" the other.
        let two = TodaySummary(nodes: [node(1, "Languages", (1, 0, 0)), node(3, "Science", (2, 0, 0))], studied: nil)
        #expect(two.orderLine(locale: locale) == "Languages first, then Science")
    }

    @Test func theEstimateNeedsARecentAnswerAndRoundsUpToAMinute() {
        // No answer in the last seven days: no estimate, and the line stops at the decks.
        let none = TodaySummary(nodes: tree, studied: studied(recentAnswers: 0, recentTime: 0))
        #expect(none.estimateMinutes == nil)
        #expect(none.studyLine(locale: locale) == "492 cards in 4 decks")
        #expect(TodaySummary(nodes: tree, studied: nil).estimateMinutes == nil)
        // One card at a second an answer: at least a minute.
        let quick = TodaySummary(nodes: [node(1, "Languages", (1, 0, 0))], studied: studied(recentAnswers: 1, recentTime: 1_000))
        #expect(quick.estimateMinutes == 1)
        // Exactly 3 minutes stays 3; a millisecond more is 4.
        #expect(TodaySummary(nodes: [node(1, "L", (3, 0, 0))], studied: studied(recentAnswers: 3, recentTime: 180_000)).estimateMinutes == 3)
        #expect(TodaySummary(nodes: [node(1, "L", (3, 0, 0))], studied: studied(recentAnswers: 3, recentTime: 180_001)).estimateMinutes == 4)
        // Nothing waiting: no estimate.
        #expect(TodaySummary(nodes: [node(6, "Reading", (0, 0, 0))], studied: studied(recentAnswers: 9, recentTime: 9_000)).estimateMinutes == nil)
    }

    @Test func studiedTodaySaysTheCardsAndTimeWithItsBar() {
        let summary = TodaySummary(nodes: tree, studied: studied(cards: 128, time: 1_140_000))
        #expect(summary.studiedValue(locale: locale) == "128 cards · 19 min")
        #expect(summary.studiedAccessibilityValue(locale: locale) == "128 cards, 19 minutes")
        // The bar: today's cards over those and the walk's cards waiting.
        #expect(summary.studiedFraction == 128.0 / Double(128 + 492))
        // Under a minute, whole seconds; a negative sum shows as 0.
        #expect(TodaySummary(nodes: tree, studied: studied(cards: 3, time: 40_400)).studiedValue(locale: locale) == "3 cards · 40 sec")
        #expect(TodaySummary(nodes: tree, studied: studied(cards: 3, time: -5)).studiedValue(locale: locale) == "3 cards · 0 sec")
        // Minutes round to the nearest: 19 min 30 s is 20.
        #expect(TodaySummary(nodes: tree, studied: studied(cards: 3, time: 1_170_000)).studiedValue(locale: locale) == "3 cards · 20 min")
    }

    @Test func nothingStudiedYetHasAnEmptyBar() {
        let before = TodaySummary(nodes: tree, studied: nil)
        #expect(before.studiedValue(locale: locale) == "None yet")
        #expect(before.studiedAccessibilityValue(locale: locale) == "None yet")
        #expect(before.studiedFraction == 0)
        #expect(TodaySummary(nodes: tree, studied: studied(cards: 0, time: 0)).studiedValue(locale: locale) == "None yet")
        // Nothing studied and nothing waiting: still an empty bar, never a division by zero.
        #expect(TodaySummary(nodes: [], studied: studied()).studiedFraction == 0)
        // Everything studied: a full bar.
        #expect(TodaySummary(nodes: [node(6, "Reading", (0, 0, 0))], studied: studied(cards: 9)).studiedFraction == 1)
    }

    @Test func thePreviousSessionNamesItsDecksAndWhenItEnded() throws {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "Europe/Paris") ?? .gmt
        let now = try #require(calendar.date(from: DateComponents(year: 2026, month: 11, day: 21, hour: 15, minute: 30)))
        let morning = try #require(calendar.date(from: DateComponents(year: 2026, month: 11, day: 21, hour: 8, minute: 12)))
        func summary(_ names: [String], walk: Bool = false, ended: Date) -> TodaySummary {
            TodaySummary(nodes: tree, studied: nil,
                         previous: .init(deckNames: names, walk: walk, cards: 46, endedAt: ended))
        }
        let deck = summary(["Languages::Español::Vocabulary"], ended: morning)
        #expect(deck.previousDecks(locale: locale) == "Vocabulary in Español")
        let time = morning.formatted(Date.FormatStyle(date: .omitted, time: .shortened, locale: locale, calendar: calendar,
                                                      timeZone: calendar.timeZone))
        #expect(deck.previousValue(now: now, calendar: calendar, locale: locale) == "46 cards · \(time)")
        #expect(summary(["Languages"], ended: morning).previousDecks(locale: locale) == "Languages")
        // A walk names its decks with the list format.
        #expect(summary(["Languages", "Science", "Culture"], walk: true, ended: morning).previousDecks(locale: Locale(identifier: "en_GB"))
            == "Languages, Science and Culture")
        // The calendar day before, in the device's zone: the system's "Yesterday" and the time.
        let lateYesterday = try #require(calendar.date(from: DateComponents(year: 2026, month: 11, day: 20, hour: 23, minute: 50)))
        let yesterday = try #require(summary(["Languages"], ended: lateYesterday).previousValue(now: now, calendar: calendar, locale: locale))
        #expect(yesterday.hasPrefix("46 cards · Yesterday"))
        // Earlier: the date.
        let older = try #require(calendar.date(from: DateComponents(year: 2026, month: 11, day: 18, hour: 9)))
        #expect(summary(["Languages"], ended: older).previousValue(now: now, calendar: calendar, locale: locale) == "46 cards · Nov 18, 2026")
        // No previous session, no row.
        #expect(TodaySummary(nodes: tree, studied: nil).previousValue(now: now, calendar: calendar, locale: locale) == nil)
    }

    @Test func theCatalogHasTodaysStrings() throws {
        // SwiftPM builds do not compile string catalogs: read the source the app ships.
        let catalog = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appending(path: "Sources/Features/Home/Resources/Localizable.xcstrings")
        let root = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: catalog)) as? [String: Any])
        let strings = try #require(root["strings"] as? [String: Any])
        func english(_ key: String) -> [String: Any]? {
            ((strings[key] as? [String: Any])?["localizations"] as? [String: Any])?["en"] as? [String: Any]
        }
        func value(_ key: String) -> String? { (english(key)?["stringUnit"] as? [String: Any])?["value"] as? String }
        func plural(_ key: String, _ form: String) -> String? {
            let forms = (english(key)?["variations"] as? [String: Any])?["plural"] as? [String: Any]
            return ((forms?[form] as? [String: Any])?["stringUnit"] as? [String: Any])?["value"] as? String
        }
        #expect(value("Study All Decks") == "Study All Decks")
        #expect(value("Studied Today") == "Studied Today")
        #expect(value("None yet") == "None yet")
        #expect(value("Next learning card at %@") == "Next learning card at %@")
        #expect(value("home.today.studied.spoken") == "%1$@, %2$@")
        #expect(value("home.today.previous.deck") == "%1$@ in %2$@")
        #expect(plural("%lld cards", "one") == "%lld card" && plural("%lld cards", "other") == "%lld cards")
        #expect(plural("%lld decks", "one") == "%lld deck" && plural("%lld decks", "other") == "%lld decks")
    }
}
