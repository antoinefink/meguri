import CoreBridge
import Foundation
import Testing

@testable import Stats

/// The logic the section views of U299 to U301 add on top of the model's values, against
/// hand-computed fixtures: Reviews Due's axis labels beside Overdue and Later (S7.9; the labels
/// ran into them in a small box), Retrievability's desired-retention marker (S7.12), the
/// Card Counts' pie and Answers' shares (S7.7, S7.10), Time of Day's shared callout
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

    // MARK: Card Counts' pie (S7.7) and Answers' shares (S7.10)

    /// A turn from 12 o'clock clockwise among 50, 0, 25 and 25 cards: the first half is the
    /// first state, the empty one takes nothing, then a quarter each.
    @Test func thePiesSectorsFollowTheTurnFromTwelveOClock() {
        let counts = [50, 0, 25, 25]
        #expect(StatsCardCountsPie.state(atFraction: 0, counts: counts) == 0)
        #expect(StatsCardCountsPie.state(atFraction: 0.49, counts: counts) == 0)
        #expect(StatsCardCountsPie.state(atFraction: 0.5, counts: counts) == 2)
        #expect(StatsCardCountsPie.state(atFraction: 0.75, counts: counts) == 3)
        #expect(StatsCardCountsPie.state(atFraction: 1, counts: counts) == 3)
        #expect(StatsCardCountsPie.state(atFraction: 0.5, counts: [0, 0]) == nil)
        // A pie 100 points across: four points near its edge, one near its centre and one beyond it.
        let size = CGSize(width: 100, height: 100)
        #expect(StatsCardCountsPie.state(at: CGPoint(x: 50, y: 5), in: size, counts: counts) == 0)
        #expect(StatsCardCountsPie.state(at: CGPoint(x: 95, y: 50), in: size, counts: counts) == 0)
        #expect(StatsCardCountsPie.state(at: CGPoint(x: 50, y: 95), in: size, counts: counts) == 2)
        #expect(StatsCardCountsPie.state(at: CGPoint(x: 5, y: 50), in: size, counts: counts) == 3)
        // A pie has no empty centre: a point near the centre is in a sector (just right of 12 o'clock).
        #expect(StatsCardCountsPie.state(at: CGPoint(x: 51, y: 49), in: size, counts: counts) == 0)
        #expect(StatsCardCountsPie.state(at: CGPoint(x: 2, y: 2), in: size, counts: counts) == nil)
    }

    /// The overview's pie follows width and leaves the table its measured width. Its sizing
    /// has no height input: neither a tall neighbor nor large text can enlarge it.
    @Test func overviewPieUsesAThirdOfTheWidthWithACap() {
        #expect(StatsOverviewPieBesideTable.pieSide(width: 450, tableWidth: 220) == 150)
        #expect(StatsOverviewPieBesideTable.pieSide(width: 600, tableWidth: 220) == 160)
        #expect(StatsOverviewPieBesideTable.pieSide(width: 1_000, tableWidth: 220) == 160)
        // A long localized table takes priority over the pie's preferred third.
        #expect(StatsOverviewPieBesideTable.pieSide(width: 480, tableWidth: 320) == 144)
        #expect(StatsPlotSize.overview.height == 120)
        #expect(StatsPlotSize.regular.height == 150 && StatsPlotSize.page.height == 320)
    }

    @Test func addedOverviewHasFourLocalizedRealValues() throws {
        let set = Fixture.set(values: [
            .added: .added(Fixture.map([-3: 12_345])),
            .started: .started(StatsStarted(days: Fixture.map([-10: 6_789]), firstDay: -700)),
        ])
        for identifier in ["en_GB", "fr_FR"] {
            let locale = Locale(identifier: identifier)
            let presenter = try #require(StatsPresenter(
                set: set, period: .oneMonth, selection: StatsPageSelection(deck: nil, period: .oneMonth),
                decks: StatsDeckDirectory(nodes: Fixture.nodes()), calendar: Self.calendar, locale: locale
            ))
            let content = try #require(Fixture.content(.addedAndStarted, presenter))
            let figures = StatsAddedAndStartedBox.summaryFigures(content, text: StatsText(locale: locale))
            let format = StatsChartFormat(locale: locale, calendar: Self.calendar, now: .distantPast)
            #expect(figures.map(\.label) == ["Added", "Started", "Added a Day", "Started a Day"])
            #expect(figures.map(\.value) == [format.count(12_345), format.count(6_789)] + content.figures.map(\.value))
            #expect(content.figure?.sentence == StatsText(locale: locale).startedSentence(format.count(6_789)))
            #expect(content.data.legendNames == ["Added", "Started"])
            #expect(content.data.series.map(\.mark) == [.bar, .line()])
        }
    }

    @Test func addedOverviewKeepsLabelsAndDashesUntilReady() {
        let waiting = StatsBoxContent(phase: .loading(fraction: nil), data: StatsChartData(bins: [], series: []))
        let figures = StatsAddedAndStartedBox.summaryFigures(waiting, text: StatsText(locale: Self.locale))
        #expect(figures.map(\.label) == ["Added", "Started", "Added a Day", "Started a Day"])
        #expect(figures.allSatisfy { $0.value == nil })
    }

    /// Detail and mobile: beside a taller box the pie takes the height it is given, up to its limit and never
    /// width the table needs; at its own height, or with no room, it keeps its side.
    @Test func thePieGrowsIntoTheRowsHeight() {
        let side = StatsCardCountsBox.side, limit = StatsCardCountsBox.largeSide
        #expect(side < limit)
        let grow = { (height: CGFloat, room: CGFloat) in
            StatsPieBesideTable.pieSide(side: side, limit: limit, own: side, height: height, room: room)
        }
        #expect(grow(side, 400) == side)
        #expect(grow(side + 30, 400) == side + 30)
        #expect(grow(limit + 80, 400) == limit)
        #expect(grow(limit + 80, side + 10) == side + 10)
        #expect(grow(limit + 80, side - 40) == side)
        #expect(grow(0, 0) == side)
        // A table taller than the pie (large text) sets the pair's own height: the pie keeps its
        // side there, and grows only in height beyond it.
        let tall = side + 30
        #expect(StatsPieBesideTable.pieSide(side: side, limit: limit, own: tall, height: tall, room: 400) == side)
        #expect(StatsPieBesideTable.pieSide(side: side, limit: limit, own: tall, height: tall + 10, room: 400) == tall + 10)
        // A table taller than the limit, offered more height still: the pie reaches the limit.
        #expect(StatsPieBesideTable.pieSide(side: side, limit: limit, own: limit + 20, height: limit + 90, room: 400) == limit)
    }

    /// Answers stack each kind to 100%: Young's 1, 1, 6 and 2 presses are 10%, 10%, 60% and 20%,
    /// each written with its count, and the kind's 10 answers end its callout.
    @Test func answersAreSharesOfEachKind() throws {
        let content = try #require(Fixture.content(.answers, Fixture.presenter(Fixture.set())))
        let data = StatsAnswersBox.shares(content.data, text: StatsText(locale: Self.locale), format: Self.format)
        #expect(data.unit == .fraction)
        #expect(data.series.map(\.id) == ["again", "hard", "good", "easy"])
        #expect(data.series.map { $0.values[1] } == [0.1, 0.1, 0.6, 0.2])
        #expect(data.bins.indices.allSatisfy { data.stackTotal(at: $0) > 0.999 && data.stackTotal(at: $0) < 1.001 })
        #expect(data.yDomain == 0...1)
        #expect(Self.format.calloutText(data, bin: 1)
            == "Young, Again 10.0% (1), Hard 10.0% (1), Good 60.0% (6), Easy 20.0% (2), 10 answers")
        #expect(data.keepsOneFamily)
        // Before the part arrives: the four answers' series, so the legend is in place.
        let waiting = StatsAnswersBox.shares(StatsChartData(bins: [], series: []), text: StatsText(locale: Self.locale),
                                             format: Self.format)
        #expect(waiting.legendNames == ["Again", "Hard", "Good", "Easy"] && waiting.bins.isEmpty)
    }

    // MARK: Knowledge's likely forgotten band (S7.1)

    /// The band runs only where both lines have a value, ordered low to high, and a gap in
    /// either breaks it into pieces.
    @Test func aBandFollowsTheGapsInItsLines() {
        let data = StatsChartData(
            bins: (0..<6).map { StatsPeriod.bucket("b\($0)") },
            series: [
                StatsSeries(id: "known", name: "Known", color: .remembered, mark: .line(), values: [8, 9, nil, 7, 8, 9]),
                StatsSeries(id: "studied", name: "Studied", color: .reference, mark: .line(dashed: true),
                            values: [10, 10, 10, nil, 9, 8]),
            ],
            bands: [StatsBand(id: "gap", name: "Likely forgotten", color: .forgotten, lower: "known", upper: "studied")])
        let pieces = data.bandPieces(data.bands[0])
        #expect(pieces.map { $0.map(\.bin) } == [[0, 1], [4, 5]])
        #expect(pieces[0].map(\.lower) == [8, 9] && pieces[0].map(\.upper) == [10, 10])
        // Known above Studied at the last bin: the band still runs from the lower to the higher.
        #expect(pieces[1].last.map { [$0.lower, $0.upper] } == [8, 9])
        #expect(data.legendNames == ["Known", "Studied", "Likely forgotten"])
        #expect(data.keepsOneFamily)
        #expect(data.bandPieces(StatsBand(id: "x", name: "", color: .forgotten, lower: "none", upper: "studied")).isEmpty)
    }

    /// Knowledge Over Time's chart carries the band between Known and Studied in Forgotten's
    /// orange, its legend entry after the two lines, and the callout's likely forgotten line
    /// with the same swatch.
    @Test func knowledgeShadesWhatIsLikelyForgotten() throws {
        let content = try #require(Fixture.content(.knowledgeOverTime, Fixture.presenter(Fixture.set())))
        let band = try #require(content.data.bands.first)
        #expect(band.color == .forgotten && band.lower == "known" && band.upper == "studied")
        #expect(band.spoken?.contains("stats.") == false)
        #expect(content.data.legendNames == ["Known", "Studied", "Likely forgotten"])
        #expect(content.data.calloutLines.first?.color == .forgotten)
        let rows = Self.format.calloutRows(content.data, bin: content.data.bins.count - 1)
        #expect(rows.last?.name == "Likely forgotten" && rows.last?.color == .forgotten)
        // The box keeps its legend while loading.
        #expect(content.data.skeleton().legendNames.last == "Likely forgotten")
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
