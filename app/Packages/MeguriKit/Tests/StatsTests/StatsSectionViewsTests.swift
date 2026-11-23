import CoreBridge
import Foundation
import Testing

@testable import Stats

/// The logic the section views of U299 to U301 add on top of the model's values, against
/// hand-computed fixtures: Reviews Due's axis labels beside Overdue and Later (S7.9; the labels
/// ran into them in a small box), Retrievability's desired-retention marker (S7.12), the
/// segmented bars of Card Counts and Answers (S7.7, S7.10), Time of Day's shared callout
/// (S7.11) and By Deck's sorting (S7.15).
@MainActor
@Suite struct StatsSectionViewsTests {
    static let locale = Locale(identifier: "en_GB")
    static var calendar: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        calendar.firstWeekday = 2
        return calendar
    }

    static let now = calendar.date(from: DateComponents(year: 2026, month: 11, day: 22, hour: 9))!
    static var format: StatsChartFormat { StatsChartFormat(locale: locale, calendar: calendar, now: now) }

    static func day(_ offset: Int) -> Date { calendar.date(byAdding: .day, value: offset, to: calendar.startOfDay(for: now))! }
    static func month(_ offset: Int) -> Date {
        calendar.date(byAdding: .month, value: offset, to: calendar.date(from: DateComponents(year: 2026, month: 11, day: 1))!)!
    }

    // MARK: Reviews Due's axis (S7.9)

    /// 1M ahead with overdue cards: Overdue, then today and 29 days. A lead box labels about
    /// seven bins: Overdue takes one place, six are left for the 30 days, one every five days
    /// counted on from today (bins 1, 6, …, 26); today's label, one bin from Overdue, would touch
    /// it and is dropped.
    @Test func aMonthAheadKeepsItsDatesClearOfOverdue() {
        let bins: [StatsPeriod] = [.overdue] + (0...29).map { .day(Self.day($0)) }
        let labels = Self.format.labelledBins(bins, maximum: 7)
        #expect(labels.keys.sorted() == [0, 6, 11, 16, 21, 26])
        #expect(labels[0] == "Overdue")
        #expect(labels[6] == "27 Nov")
    }

    /// With nothing overdue there is nothing to keep clear of: the days are labelled as any
    /// day axis, counted back from its last bar.
    @Test func aMonthAheadWithNothingOverdueLabelsLikeAnyDayAxis() {
        let bins: [StatsPeriod] = (0...29).map { .day(Self.day($0)) }
        let labels = Self.format.labelledBins(bins, maximum: 7)
        #expect(labels.keys.sorted() == [4, 9, 14, 19, 24, 29])
    }

    /// All ahead: Overdue, 25 months (this one to the 24th) and Later. Months over more than a
    /// year take a wider label, so a lead box keeps four places for its months (Overdue and
    /// Later take two of the seven): every 7th month from this one (bins 1, 8, 15, 22); bin 1
    /// touches Overdue and bin 22 stands 4 bins from Later, under the clearance of 5: both go.
    @Test func allAheadKeepsItsMonthsClearOfOverdueAndLater() {
        let bins: [StatsPeriod] = [.overdue] + (0...24).map { .month(Self.month($0)) } + [.later]
        let labels = Self.format.labelledBins(bins, maximum: 7)
        #expect(labels.keys.sorted() == [0, 8, 15, 26])
        #expect(labels[8] == "Jun 2027")
        #expect(labels[15] == "Jan 2028")
        #expect(labels[26] == "Later")
    }

    /// A year ahead in weeks with overdue cards, in a lead box: six places for 52 weeks, one
    /// every nine weeks from this one; the first, beside Overdue, goes.
    @Test func aYearAheadInWeeksSpacesItsLabels() {
        let bins: [StatsPeriod] = [.overdue] + (0..<52).map { .week(start: Self.day(7 * $0 - 6)) }
        let labels = Self.format.labelledBins(bins, maximum: 7)
        #expect(labels.keys.sorted() == [0, 10, 19, 28, 37, 46])
    }

    /// The axes of the past keep their rule: counted back from today.
    @Test func pastAxesStillCountBackFromToday() {
        let bins: [StatsPeriod] = (-29...0).map { .day(Self.day($0)) }
        let labels = Self.format.labelledBins(bins, maximum: 7)
        #expect(labels.keys.sorted() == [4, 9, 14, 19, 24, 29])
        #expect(labels[29] == "22 Nov")
    }

    // MARK: Retrievability's marker (S7.12)

    /// 90% among twenty 5% bins stands at 18, the leading edge of the 90%–95% bar; 92.5% in
    /// the middle of that bar; values off the axis stay on the plot.
    @Test func theMarkerStandsWhereItsValueFallsOnTheBins() throws {
        #expect(StatsMarker.position(of: 0.9, lower: 0, upper: 1, bins: 20) == 18)
        #expect(StatsMarker.position(of: 0.925, lower: 0, upper: 1, bins: 20) == 18.5)
        #expect(StatsMarker.position(of: 0.97, lower: 0, upper: 1, bins: 20).map { ($0 * 100).rounded() } == 1_940)
        #expect(StatsMarker.position(of: 1.2, lower: 0, upper: 1, bins: 20) == 20)
        #expect(StatsMarker.position(of: -0.1, lower: 0, upper: 1, bins: 20) == 0)
        #expect(StatsMarker.position(of: 0.9, lower: 0, upper: 1, bins: 0) == nil)
        // Bars 10 points apart, the first centred at 5: bar 18 spans 180 to 190.
        #expect(StatsMarker.x(18, firstCenter: 5, step: 10) == 180)
        #expect(StatsMarker.x(18.5, firstCenter: 5, step: 10) == 185)
        #expect(StatsMarker.x(0, firstCenter: 5, step: 10) == 0)
    }

    /// The box's chart carries the model's desired retention (the fixture's 90%) as its marker:
    /// in the legend after the series, and in a bar's callout after its count.
    @Test func retrievabilityMarksTheDesiredRetention() throws {
        let content = try #require(Fixture.content(.retrievability, Fixture.presenter(Fixture.set())))
        // The core's retentions are 32-bit floats.
        #expect(content.detail == .retrievability(desiredRetention: Double(Float(0.9))))
        let data = StatsRetrievabilityBox.marked(content, name: "Desired retention", format: Self.format)
        let marker = try #require(data.markers.first)
        #expect(abs(try #require(marker.position) - 18) < 1e-5)
        #expect(marker.valueText == "90.0%")
        #expect(data.legendNames == ["Cards", "Desired retention"])
        #expect(data.keepsOneFamily)
        #expect(Self.format.calloutText(data, bin: 19) == "95%–100%, Cards 3, Desired retention 90.0%")
        // Not known yet (its part is the review log's): the legend entry alone, no rule.
        var waiting = content
        waiting.detail = .retrievability(desiredRetention: nil)
        let unknown = StatsRetrievabilityBox.marked(waiting, name: "Desired retention", format: Self.format)
        #expect(unknown.markers.first?.position == nil && unknown.legendNames.last == "Desired retention")
        #expect(!Self.format.calloutText(unknown, bin: 19).contains("Desired"))
    }

    // MARK: Segmented bars (S7.7, S7.10)

    static let bar = StatsBarRow(id: "g", name: "Young", segments: [
        .init(id: "again", name: "Again", color: .again, value: 1, text: "1 (10.0%)"),
        .init(id: "hard", name: "Hard", color: .hard, value: 0, text: "0 (0.0%)"),
        .init(id: "good", name: "Good", color: .good, value: 6, text: "6 (60.0%)"),
        .init(id: "easy", name: "Easy", color: .easy, value: 3, text: "3 (30.0%)"),
    ])

    @Test func aBarsSegmentsSpanItsWidthInOrder() {
        #expect(Self.bar.total == 10)
        #expect(Self.bar.spans.map(\.start) == [0, 0.1, 0.1, 0.7])
        #expect(Self.bar.spans.map(\.width) == [0.1, 0, 0.6, 0.3])
        // An empty segment is never under the pointer.
        #expect(Self.bar.segment(at: 0.05) == 0)
        #expect(Self.bar.segment(at: 0.1) == 2)
        #expect(Self.bar.segment(at: 0.69) == 2)
        #expect(Self.bar.segment(at: 0.7) == 3)
        #expect(Self.bar.segment(at: 1) == 3)
        #expect(Self.bar.segment(at: 1.1) == nil)
        let empty = StatsBarRow(id: "e", name: "Mature", segments: [.init(id: "again", name: "Again", color: .again, value: 0, text: "")])
        #expect(empty.segment(at: 0.5) == nil)
        #expect(Self.bar.spoken(locale: Self.locale) == "Young, Again 1 (10.0%), Hard 0 (0.0%), Good 6 (60.0%), Easy 3 (30.0%)")
    }

    /// Left and Right move along the segments with a value, bar after bar, and stop at the ends.
    @Test func theKeysMoveAlongTheSegmentsWithAValue() {
        let rows = [Self.bar, StatsBarRow(id: "m", name: "Mature", segments: Self.bar.segments)]
        var selection = StatsBarSelection.moved(.none, by: 1, in: rows)
        #expect(selection == StatsChartSelection(bin: 0, pinned: true, segment: 0))
        selection = StatsBarSelection.moved(selection, by: 1, in: rows)
        #expect(selection.segment == 2)
        selection = StatsBarSelection.moved(selection, by: 2, in: rows)
        #expect(selection.bin == 1 && selection.segment == 0)
        #expect(StatsBarSelection.moved(.none, by: -1, in: rows) == StatsChartSelection(bin: 1, pinned: true, segment: 3))
        let last = StatsChartSelection(bin: 1, pinned: true, segment: 3)
        #expect(StatsBarSelection.moved(last, by: 1, in: rows) == last)
    }

    /// A refresh that keeps the bars keeps the pinned segment (S10.2).
    @Test func aCarriedSelectionKeepsItsSegment() {
        let bins: [StatsPeriod] = [.bucket("Learning"), .bucket("Young"), .bucket("Mature")]
        let pinned = StatsChartSelection(bin: 1, pinned: true, segment: 2)
        #expect(pinned.carried(from: bins, to: bins) == pinned)
        #expect(pinned.carried(from: bins, to: Array(bins.suffix(2))) == StatsChartSelection(bin: 0, pinned: true, segment: 2))
    }

    /// Answers' bars come from the model's table: a bar per group, the four buttons in the
    /// series' order with their names and colours, each segment's callout its count and share.
    @Test func answersBarsComeFromTheTable() throws {
        let content = try #require(Fixture.content(.answers, Fixture.presenter(Fixture.set())))
        guard case let .answers(table)? = content.detail else { Issue.record("no table"); return }
        let bars = StatsAnswersBox.bars(table, data: content.data, text: StatsText(locale: Self.locale))
        #expect(bars.map(\.name) == ["Learning", "Young", "Mature"])
        #expect(bars.map(\.id) == ["learning", "young", "mature"])
        #expect(bars[1].segments.map(\.name) == ["Again", "Hard", "Good", "Easy"])
        #expect(bars[1].segments.map(\.color) == [.again, .hard, .good, .easy])
        #expect(bars[1].segments.map(\.value) == [1, 1, 6, 2])
        #expect(bars[1].segments.map(\.text) == ["1 (10.0%)", "1 (10.0%)", "6 (60.0%)", "2 (20.0%)"])
        #expect(StatsAnswersBox.spoken("Young", buttons: ["Again", "Hard"], counts: ["1"], text: StatsText(locale: Self.locale),
                                       locale: Self.locale) == "Young, Again 1, Hard —")
    }

    /// Card Counts' bar has a segment per state in the table's order, its Audio Graph a
    /// category per state.
    @Test func cardCountsBarHasASegmentPerState() throws {
        let content = try #require(Fixture.content(.cardCounts, Fixture.presenter(Fixture.set())))
        guard case let .cardCounts(table)? = content.detail else { Issue.record("no table"); return }
        let bar = StatsCardCountsBox.bar(table, title: "Card Counts", text: StatsText(locale: Self.locale))
        #expect(bar.segments.map(\.id) == ["mature", "young", "relearning", "learning", "new", "suspended", "buried"])
        #expect(bar.segments.first?.text == "60 (60.0%)")
        let graph = StatsCardCountsBox.values(table, series: "Cards")
        #expect(graph.bins.count == 7 && graph.series.first?.values.first == 60)
    }

    // MARK: Time of Day (S7.11)

    /// The lower panel's callout and elements also name the hour's share remembered, so one
    /// callout reads both panels.
    @Test func timeOfDaysLowerPanelSpeaksForBoth() throws {
        let content = try #require(Fixture.content(.timeOfDay, Fixture.presenter(Fixture.set())))
        let text = StatsText(locale: Self.locale)
        let upper = StatsTimeOfDayBox.upper(content, text: text)
        let lower = StatsTimeOfDayBox.lower(content, upper: upper, text: text)
        #expect(lower.calloutLines.map(\.name) == ["Remembered"])
        let hour9 = Self.format.calloutText(lower, bin: 9)
        #expect(hour9.hasSuffix("Reviews 30, Remembered 90.0%"), "\(hour9)")
        // Hour 20 has too few counted reviews for a share: its callout names the reviews alone.
        #expect(!Self.format.calloutText(lower, bin: 20).contains("Remembered"))
        // Before the part arrives: the upper panel's series, no values, and no extra line below.
        let loading = StatsBoxContent(phase: .loading(fraction: nil), data: content.data.skeleton())
        let skeleton = StatsTimeOfDayBox.upper(loading, text: text)
        #expect(skeleton.series.map(\.name) == ["Remembered"] && skeleton.bins.isEmpty)
        #expect(StatsTimeOfDayBox.lower(loading, upper: skeleton, text: text).calloutLines.isEmpty)
    }

    // MARK: By Deck's sorting (S7.15)

    static func deckRow(_ id: Int64, _ name: String, cards: Int, reviews: Int, retention: Double?, time: Double,
                        own: Bool = false) -> StatsByDeckTable.Row {
        StatsByDeckTable.Row(deckID: id, name: name, ownCards: own, cards: cards, reviews: reviews, retention: retention,
                             timeShare: time, cardsText: "\(cards)", reviewsText: "\(reviews)",
                             retentionText: retention.map { "\($0)" } ?? "—", timeShareText: "\(time)")
    }

    /// The deck tree's order: the deck's own cards, then Alpha, Beta, Gamma, Delta.
    static let rows = [
        deckRow(1, "Parent", cards: 5, reviews: 2, retention: 0.5, time: 0.05, own: true),
        deckRow(2, "Alpha", cards: 40, reviews: 30, retention: 0.9, time: 0.4),
        deckRow(3, "Beta", cards: 10, reviews: 0, retention: nil, time: 0),
        deckRow(4, "Gamma", cards: 40, reviews: 50, retention: 0.8, time: 0.45),
        deckRow(5, "Delta", cards: 25, reviews: 8, retention: 0.95, time: 0.1),
    ]

    static func names(_ sort: StatsByDeckSort) -> [String] { sort.sorted(rows).map(\.name) }

    @Test func byDeckSortsByEveryColumn() {
        // The first order is the deck tree's, as the model gives it; descending reverses it.
        #expect(Self.names(StatsByDeckSort()) == ["Parent", "Alpha", "Beta", "Gamma", "Delta"])
        #expect(Self.names(StatsByDeckSort(column: .deck, ascending: false)) == ["Delta", "Gamma", "Beta", "Alpha", "Parent"])
        // Equal counts keep the deck order (Alpha before Gamma, 40 each).
        #expect(Self.names(StatsByDeckSort(column: .cards, ascending: false)) == ["Alpha", "Gamma", "Delta", "Beta", "Parent"])
        #expect(Self.names(StatsByDeckSort(column: .cards, ascending: true)) == ["Parent", "Beta", "Delta", "Alpha", "Gamma"])
        #expect(Self.names(StatsByDeckSort(column: .reviews, ascending: false)) == ["Gamma", "Alpha", "Delta", "Parent", "Beta"])
        #expect(Self.names(StatsByDeckSort(column: .time, ascending: false)) == ["Gamma", "Alpha", "Delta", "Parent", "Beta"])
        // A deck with no retention goes last either way.
        #expect(Self.names(StatsByDeckSort(column: .retention, ascending: false)) == ["Delta", "Alpha", "Gamma", "Parent", "Beta"])
        #expect(Self.names(StatsByDeckSort(column: .retention, ascending: true)) == ["Parent", "Gamma", "Alpha", "Delta", "Beta"])
    }

    /// A click on the sorted column turns its direction; on another column, the deck column
    /// starts ascending and a number the largest first.
    @Test func aHeaderClickSortsItsColumn() {
        let first = StatsByDeckSort()
        #expect(first.clicked(.deck) == StatsByDeckSort(column: .deck, ascending: false))
        #expect(first.clicked(.reviews) == StatsByDeckSort(column: .reviews, ascending: false))
        #expect(first.clicked(.reviews).clicked(.reviews) == StatsByDeckSort(column: .reviews, ascending: true))
        #expect(StatsByDeckSort(column: .time, ascending: false).clicked(.deck) == StatsByDeckSort())
    }

    // MARK: Strings

    @Test func theSectionsTextsComeFromTheCatalog() {
        let text = StatsSectionText(locale: Self.locale)
        let texts = StatsByDeckColumn.allCases.map(text.column) + [text.ascending, text.descending, text.rowHint,
                                                                   text.labelled("Retention", "91.2%")]
        for value in texts { #expect(!value.contains("stats."), "\(value)") }
        #expect(StatsByDeckColumn.allCases.map(text.column) == ["Deck", "Cards", "Reviews", "Retention", "Time"])
        #expect(text.labelled("Retention", "91.2%") == "Retention 91.2%")
        #expect(StatsSectionText.summary("Stability", StatsBoxFigure(value: "3 months", sentence: "Median stability"))
            == "Stability, 3 months, Median stability")
        #expect(StatsSectionText.summary("Card Counts", StatsBoxFigure(value: "100 cards in 80 notes", sentence: ""), ["x"])
            == "Card Counts, 100 cards in 80 notes, x")
    }
}
