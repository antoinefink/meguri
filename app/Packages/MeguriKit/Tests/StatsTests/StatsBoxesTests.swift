import CoreBridge
import Foundation
import Testing

@testable import Stats

/// Every box's figures against hand-computed fixtures (`stats/statistics-page` S7), the streak
/// rules (S7.5), the open points (S7.2), the Memory bins' edges (S7.13, S7.14), the empty lines
/// (S8.5) and the strip (S6). Fixture values: `Fixture.value(_:)`; the period is 1M unless named.
@MainActor
@Suite struct StatsBoxesTests {
    static let presenter = Fixture.presenter(Fixture.set())

    static func box(_ id: StatsBoxID, _ presenter: StatsPresenter = presenter) throws -> StatsBoxContent {
        try #require(Fixture.content(id, presenter))
    }

    static func figures(_ content: StatsBoxContent) -> [String: String?] {
        Dictionary(content.figures.map { ($0.label, $0.value) }, uniquingKeysWith: { first, _ in first })
    }

    @Test func knowledgeOverTime() throws {
        let p = Self.presenter
        let box = try Self.box(.knowledgeOverTime)
        #expect(box.phase == .ready)
        // Known now 76.6 → 77 of 86 studied; at the end of day −30, 70.4 → 70.
        #expect(box.figure?.value == "77 cards")
        let since = p.format.shortDate(p.days.date(ofDay: -30))
        #expect(box.figure?.sentence == "89.1% of the 86 cards with an estimate, up 7 since \(since)")
        #expect(Self.figures(box) == ["Studied": "86", "Likely Forgotten": "9 (10.5%)", "Change in Period": "+7"])
        // The 5 suspended cards left out are not on the box face (the marker's help says it).
        #expect(box.footnote == nil)
        let known = try #require(box.data.series.first { $0.id == "known" })
        #expect(known.values.suffix(2) == [75.2, 76.6])
        #expect(known.values.prefix(28).allSatisfy { $0 == nil })
        #expect(box.data.yScale == .fitted)
        // Known never exceeds Studied (S20's by-construction check).
        let studied = try #require(box.data.series.first { $0.id == "studied" })
        #expect(zip(known.values, studied.values).allSatisfy { ($0 ?? 0) <= ($1 ?? .infinity) })
        // The callout names the gap with its share (S7.1): today 86 − 77 = 9, 10.5%.
        #expect(p.format.calloutText(box.data, bin: 29).hasSuffix("Studied 86, Likely forgotten 9 (10.5%)"))
    }

    @Test func knowledgeEmptyLines() throws {
        let none = StatsPartValue.knowledgeOverTime(StatsKnowledgeOverTime(samples: [], failed: 0, leftOut: 0))
        let noReview = try Self.box(.knowledgeOverTime, Fixture.presenter(Fixture.set(oldest: .noRows, values: [.knowledgeOverTime: none])))
        #expect(noReview.phase == .empty("No reviews yet", detail: "This fills in as you study."))
        let noEstimate = try Self.box(.knowledgeOverTime, Fixture.presenter(Fixture.set(values: [.knowledgeOverTime: none])))
        #expect(noEstimate.phase == .empty("No cards with a memory estimate", detail: nil))
        // The legend stays.
        #expect(noEstimate.data.legendNames == ["Known", "Studied", "Likely forgotten"])
    }

    @Test func retention() throws {
        let box = try Self.box(.retention)
        // Day −1: 40 of 45; day 0: 4 of 5 (an open point).
        let line = try #require(box.data.series.first)
        #expect(line.values.suffix(2) == [40.0 / 45.0, 0.8])
        #expect(line.openPoints == IndexSet([29]))
        #expect(box.figure?.value == "88.0%")
        #expect(box.figure?.sentence == "Desired retention 90.0%")
        #expect(box.data.rules.map(\.name) == ["Desired retention"])
        #expect(Self.figures(box) == ["Remembered": "44", "Forgotten": "6", "Young": "82.4%", "Mature": "90.9%"])
        // Young only.
        let young = Fixture.presenter(Fixture.set(), selection: StatsPageSelection(deck: nil, period: .oneMonth, retentionKind: .young))
        #expect(try Self.box(.retention, young).data.series.first?.values.suffix(2) == [10.0 / 12.0, 0.8])
    }

    @Test func retentionOpenPointsBelowTwentyReviews() throws {
        let days = StatsPartValue.retentionByDay(StatsRetentionByDay(
            days: Fixture.map([-2: Fixture.retentionDay(15, 4, 0, 0), -1: Fixture.retentionDay(16, 4, 0, 0)]),
            retentions: [0.85, 0.9], several: true))
        let box = try Self.box(.retention, Fixture.presenter(Fixture.set(values: [.retentionByDay: days])))
        // 19 counted reviews: open; 20: a point on the line; none: no value.
        #expect(box.data.series.first?.openPoints == IndexSet([27]))
        #expect(box.data.series.first?.values[26] == nil)
        // Counted rows give the weighted mean; `several` names it an average.
        #expect(box.figure?.sentence == "Average desired retention 90.0%")
        #expect(box.data.rules.first?.name == "Average desired retention")
    }

    @Test func desiredRetentionWithNoCountedRowIsTheMeanOfTheRetentionsInForce() throws {
        let p = Fixture.presenter(Fixture.set())
        let value = StatsRetentionByDay(days: .empty, retentions: [0.8, 0.9], several: true)
        let mean = try #require(p.desiredRetention(value, over: p.pastAxis))
        #expect(abs(mean - 0.85) < 1e-6)
    }

    @Test func forgotten() throws {
        let box = try Self.box(.forgotten)
        #expect(box.data.series.first?.values.suffix(2) == [5, 1])
        #expect(box.data.series.first?.color == .forgotten)
        #expect(box.figure?.value == "6")
        #expect(box.figure?.sentence == "0.2 a day on average")
        #expect(Self.figures(box) == ["Young": "3", "Mature": "3"])
        // Show in Browse on 1M's day bars that hold some (S8.4).
        #expect(box.data.browsable == IndexSet([28, 29]))
        let weeks = try Self.box(.forgotten, Fixture.presenter(Fixture.set(), period: .threeMonths))
        #expect(weeks.data.browsable.isEmpty)
    }

    @Test func reviewsOverTimeByTypeAndByAnswer() throws {
        let p = Self.presenter
        let box = try Self.box(.reviewsOverTime)
        #expect(box.data.series.map(\.id) == ["mature", "young", "relearning", "learning"])
        #expect(box.data.series[0].values[28] == 8)
        #expect(box.data.calloutLines.first?.values.suffix(2) == [10, 5])
        #expect(box.figure?.value == "15")
        // Activity's current streak is 3 (today not studied yet).
        #expect(box.figure?.sentence == "3 days in a row")
        #expect(Self.figures(box) == ["Days Studied": "2 of 30", "Average a Day": "1", "Average on Days Studied": "8"])
        #expect(box.data.browsable == IndexSet([28, 29]))

        let time = Fixture.presenter(Fixture.set(), selection: StatsPageSelection(deck: nil, period: .oneMonth, reviewsMeasure: .time))
        let timed = try Self.box(.reviewsOverTime, time)
        #expect(timed.data.unit == .duration)
        #expect(timed.figure?.value == p.format.duration(seconds: 150))
        #expect(Self.figures(timed)["Time per Card"] == p.format.duration(seconds: 10))

        let answers = Fixture.presenter(Fixture.set(), selection: StatsPageSelection(deck: nil, period: .oneMonth, reviewsMode: .byAnswer))
        let byAnswer = try Self.box(.reviewsOverTime, answers)
        #expect(byAnswer.data.series.map(\.name) == ["Again", "Hard", "Good", "Easy"])
        #expect(byAnswer.data.series[2].values[28] == 7)
    }

    @Test func reviewsOverTimeShowsFilteredOnlyWhenThePeriodHasSome() throws {
        let reviews = StatsPartValue.reviews(StatsReviews(count: Fixture.map([-1: Fixture.kinds(mature: 2, filtered: 1)]),
                                                          time: Fixture.map([-1: Fixture.kinds(mature: 2, filtered: 1)])))
        let box = try Self.box(.reviewsOverTime, Fixture.presenter(Fixture.set(values: [.reviews: reviews])))
        #expect(box.data.series.last?.id == "filtered")
        #expect(box.data.series.last?.color == .filtered)
    }

    @Test func streaks() {
        // Today not studied yet: the streak ends yesterday.
        #expect(StatsStreaks(Fixture.map([-3: 1, -2: 1, -1: 1])).current == 3)
        #expect(StatsStreaks(Fixture.map([-2: 1, -1: 1, 0: 1])).current == 3)
        // A gap: yesterday and today missing.
        #expect(StatsStreaks(Fixture.map([-3: 1, -2: 1])).current == 0)
        #expect(StatsStreaks(Fixture.map([-5: 1, -4: 1, -2: 1, -1: 1, 0: 2])).current == 3)
        // The longest run, across a year boundary, anywhere in history.
        let longest = StatsStreaks(Fixture.map(Dictionary(uniqueKeysWithValues: (-300 ... -280).map { ($0, 1) } + [(-1, 1)])))
        #expect(longest.longest == 21)
        #expect(longest.current == 1)
        // A row dated after today counts today.
        #expect(StatsStreaks(Fixture.map([-1: 1, 2: 1])).current == 2)
        #expect(StatsStreaks(Fixture.map([0: 0])).current == 0)
    }

    @Test func activity() throws {
        // All: every day back to the first review, each year with its total.
        let all = try Self.box(.activity, Fixture.presenter(Fixture.set(history: .allHistory), period: .all))
        guard case let .activity(activity)? = all.detail else { Issue.record("no activity"); return }
        #expect(activity.years.map(\.year) == [2026, 2025])
        #expect(activity.years.map(\.total) == [15, 2])
        #expect(activity.years.last?.totalText == "2025: 2 reviews")
        #expect(activity.currentStreak == 3 && activity.longestStreak == 3 && activity.daysStudiedThisYear == 3)
        #expect(activity.span.firstDay == -400 && activity.total == 17 && !activity.windowed)
        // Option B (2026-11-23): the streak is the headline, nothing under the grid; each year's
        // total goes beside its label on the grid.
        #expect(all.figure == StatsBoxFigure(value: "3 days in a row", sentence: "Your longest streak · studied 3 of 285 days this year"))
        #expect(all.figures.isEmpty)
        #expect(activity.daysThisYear == 285, "1 January to 12 October 2026")
        #expect(activity.years.last?.totalFull == "· 2 reviews" && activity.years.last?.totalNumber == "· 2")
        #expect(activity.years.first?.totalFull == "· 15 reviews")
    }

    /// Activity follows the period (Antoine, 2026-11-23: "with 3M selected it showed two years"):
    /// 1M to 1Y draw their window's days ending today (30, 90, 182, 365), All every day. A year
    /// keeps its total beside its label only where the window holds it from 1 January; the
    /// headline is the streak's and this year's whatever the period. Today is 12 October 2026.
    @Test func activityFollowsThePeriod() throws {
        func activity(_ period: StatsPagePeriod) throws -> (StatsActivity, StatsBoxFigure?) {
            let box = try Self.box(.activity, Fixture.presenter(Fixture.set(), period: period))
            guard case let .activity(activity)? = box.detail else { throw Fixture.Missing() }
            return (activity, box.figure)
        }
        let (month, monthFigure) = try activity(.oneMonth)
        #expect(month.span.firstDay == -29 && month.windowed)
        #expect(month.years.map(\.year) == [2026])
        // The window starts inside 2026: its label alone.
        #expect(month.years.first?.totalText == "2026" && month.years.first?.totalFull == "")
        #expect(monthFigure?.value == "3 days in a row" && month.currentStreak == 3)
        let (quarter, _) = try activity(.threeMonths)
        #expect(quarter.span.firstDay == -89 && quarter.years.map(\.year) == [2026])
        #expect(try activity(.sixMonths).0.span.firstDay == -181)
        // 1Y from 13 October 2025: 2026 is whole so far (from 1 January, day −284), 2025 is not.
        let (year, _) = try activity(.oneYear)
        #expect(year.span.firstDay == -364 && year.years.map(\.year) == [2026, 2025])
        #expect(year.years.map(\.totalFull) == ["· 15 reviews", ""])
        #expect(year.years.last?.totalText == "2025")
        // A history shorter than the window starts at its first review.
        #expect(StatsPagePeriod.oneYear.activityFirstDay(oldest: -40) == -40)
        #expect(StatsPagePeriod.oneMonth.activityFirstDay(oldest: -40) == -29)
        #expect(StatsPagePeriod.all.activityFirstDay(oldest: -40) == -40)
        #expect(StatsPagePeriod.all.activityFirstDay(oldest: 3) == 0)
    }

    /// Option B's four headlines: the current streak that is the longest, one shorter than the
    /// longest, none today, and a single day (the catalog's plural rules).
    @Test func activityHeadlines() throws {
        let days = StatsDays(context: Fixture.context(), calendar: Fixture.calendar())
        func figure(current: Int, longest: Int, studied: Int, ofDays: Int) -> StatsBoxFigure {
            var activity = StatsActivity(span: StatsActivitySpan(firstDay: -10, days: days), counts: [:], years: [],
                                         currentStreak: current, longestStreak: longest, daysStudiedThisYear: studied, days: days)
            activity.daysThisYear = ofDays
            let format = StatsChartFormat(locale: Fixture.locale, calendar: Fixture.calendar(), now: Date())
            return StatsPresenter.activityFigure(activity, text: StatsText(locale: Fixture.locale)) { format.count(Double($0)) }
        }
        #expect(figure(current: 12, longest: 12, studied: 200, ofDays: 285)
            == StatsBoxFigure(value: "12 days in a row", sentence: "Your longest streak · studied 200 of 285 days this year"))
        #expect(figure(current: 4, longest: 41, studied: 210, ofDays: 365)
            == StatsBoxFigure(value: "4 days in a row", sentence: "Longest streak 41 days · studied 210 of 365 days this year"))
        #expect(figure(current: 0, longest: 41, studied: 3, ofDays: 9)
            == StatsBoxFigure(value: "No streak today", sentence: "Longest streak 41 days · studied 3 of 9 days this year"))
        #expect(figure(current: 1, longest: 1, studied: 1, ofDays: 1)
            == StatsBoxFigure(value: "1 day in a row", sentence: "Your longest streak · studied 1 of 1 day this year"))
    }

    @Test func cardsOverTime() throws {
        let p = Self.presenter
        let box = try Self.box(.cardsOverTime)
        #expect(box.data.series.map(\.id) == ["mature", "young", "relearning", "learning", "new"])
        #expect(box.data.series.first?.values.suffix(2) == [32, 32])
        #expect(box.data.series.first?.values.first == 30)
        #expect(box.figure?.value == "90 cards")
        #expect(box.figure?.sentence == "32 mature, up 2 in the period")
        #expect(Self.figures(box) == ["New": "45 (\(p.signed(-5)))", "Learning": "2 (+2)", "Young": "11 (+1)",
                                      "Mature": "32 (+2)"])
        // The 5 suspended cards left out are not said on the box face (Antoine, 2026-11-23).
        #expect(box.footnote == nil)
        // No review: S12.3's line.
        let none = try Self.box(.cardsOverTime, Fixture.presenter(Fixture.set(oldest: .noRows)))
        #expect(none.phase == .empty("No reviews yet", detail: nil))
    }

    @Test func cardCounts() throws {
        let box = try Self.box(.cardCounts)
        #expect(box.figure?.value == "100 cards in 80 notes")
        guard case let .cardCounts(table)? = box.detail else { Issue.record("no table"); return }
        #expect(table.rows.map(\.label) == [.mature, .young, .relearning, .learning, .new, .suspended, .buried])
        #expect(table.rows.first?.shareText == "60.0%")
        #expect(table.rows.first?.itemText == "prop:ivl>=21")
        #expect(table.total == "100" && table.notes == "80")
        #expect(Self.figures(box) == ["Mature": "60 of 100"])
    }

    @Test func addedAndStarted() throws {
        let box = try Self.box(.addedAndStarted)
        let added = try #require(box.data.series.first)
        #expect(added.values[26] == 4 && added.values[29] == 3)
        #expect(box.figure?.value == "7")
        #expect(box.figure?.sentence == "5 started")
        #expect(box.detail == .addedAndStarted(added: "7", started: "5"))
        #expect(Self.figures(box) == ["Added a Day": "0.2", "Started a Day": "0.2"])
        #expect(box.data.browsable == IndexSet([26, 29]))
    }

    @Test func reviewsDue() throws {
        let box = try Self.box(.reviewsDue)
        #expect(box.data.bins.first == .overdue)
        let mature = try #require(box.data.series.first { $0.id == "mature" })
        let young = try #require(box.data.series.first { $0.id == "young" })
        let overdue = try #require(box.data.series.first { $0.id == "overdue" })
        #expect(Array(mature.values.prefix(3)) == [nil, 3, 3])
        #expect(Array(young.values.prefix(3)) == [nil, 2, 1])
        #expect(overdue.values.first == 3)
        #expect(box.data.calloutLines.first?.values.prefix(4) == [3, 8, 12, 12])
        #expect(box.figure?.value == "12")
        // All Decks: Today's totals (Words 16 + Places 4 + Cram 0).
        #expect(box.figure?.sentence == "20 remaining today · 4 tomorrow")
        #expect(Self.figures(box) == ["Overdue": "3", "Due Tomorrow": "4", "Next 30 Days": "9", "Daily Load": "7"])
        let empty = StatsPartValue.futureDue(StatsFutureDue(due: .empty, haveBacklog: false, dailyLoad: 0, dueTomorrow: 0, dueYoung: .empty))
        #expect(try Self.box(.reviewsDue, Fixture.presenter(Fixture.set(values: [.futureDue: empty]))).phase == .empty("No cards due", detail: nil))
    }

    @Test func answers() throws {
        let box = try Self.box(.answers)
        #expect(box.figure?.value == "30")
        #expect(box.figure?.sentence == "73.3% Good")
        #expect(Self.figures(box) == ["Again, Learning": "20.0%", "Again, Young": "10.0%", "Again, Mature": "10.0%"])
        guard case let .answers(table)? = box.detail else { Issue.record("no table"); return }
        #expect(table.rows.map(\.counts) == [[2, 0, 8, 0], [1, 1, 6, 2], [1, 0, 8, 1]])
    }

    @Test func timeOfDay() throws {
        let p = Self.presenter
        let box = try Self.box(.timeOfDay)
        #expect(box.figure?.value == p.format.name(.hour(9)))
        #expect(box.figure?.sentence == "is your busiest hour")
        #expect(box.data.series.first?.values[9] == 30)
        guard case let .timeOfDay(upper)? = box.detail else { Issue.record("no upper panel"); return }
        // Hour 20 has 10 counted reviews: no point.
        #expect(upper.series.first?.values[9] == 0.9 && upper.series.first?.values[20] == nil)
        #expect(Self.figures(box) == ["Best Hour for Remembering": p.format.name(.hour(9)), "Reviews Before Noon": "75.0%"])
    }

    @Test func retrievability() throws {
        let box = try Self.box(.retrievability)
        #expect(box.data.bins.count == 20)
        #expect(box.data.series.first?.values[10] == 1 && box.data.series.first?.values[19] == 3)
        #expect(box.figure?.value == "84%")
        #expect(box.figure?.sentence == "Estimated total knowledge 3 cards, 3 notes")
        #expect(Self.figures(box) == ["Below Desired Retention": "1", "Cards with a Memory State": "4"])
        #expect(box.footnote == "2 review cards without a memory state are not included.")
        #expect(box.data.browsable == IndexSet([10, 19]))
    }

    @Test func stability() throws {
        let p = Self.presenter
        let box = try Self.box(.stability)
        #expect(box.data.series.first?.values == [1, 2, 1, 0, 0, 0, 0, 0, 1, 0])
        // Q(0.5) over {0, 1, 1, 3, 400} is 1; the median interval over {1, 2, 3, 100} is 2.5 → 3.
        #expect(box.figure?.value == p.durationDays(1))
        #expect(Self.figures(box) == ["Median Interval": p.durationDays(3), "Over 1 Year": "1"])
    }

    @Test func stabilityBinsDouble() throws {
        let keys = [0, 1, 2, 3, 6, 7, 13, 14, 27, 28, 89, 90, 179, 180, 364, 365, 729, 730, 5_000]
        let value = StatsPartValue.stability(StatsStability(byDays: Fixture.map(Dictionary(uniqueKeysWithValues: keys.map { ($0, 1) })),
                                                            leftOut: 0))
        let box = try Self.box(.stability, Fixture.presenter(Fixture.set(values: [.stability: value])))
        #expect(box.data.series.first?.values == [1, 2, 2, 2, 2, 2, 2, 2, 2, 2])
        #expect(box.data.bins.first == .bucket("Under 1 day") && box.data.bins.last == .bucket("Over 2 years"))
    }

    @Test func difficultyBinsAndMedian() throws {
        let box = try Self.box(.difficulty)
        #expect(box.data.series.first?.values == [2, 0, 0, 0, 0, 1, 0, 0, 0, 1])
        #expect(box.figure?.value == "30%")
        #expect(Self.figures(box) == ["Top Tenth": "1"])
        let edges = StatsPartValue.difficulty(StatsDifficulty(byPercent: Fixture.map([9: 1, 10: 1, 99: 1, 100: 1, 150: 1]),
                                                              median: .nan, leftOut: 1))
        let edge = try Self.box(.difficulty, Fixture.presenter(Fixture.set(values: [.difficulty: edges])))
        #expect(edge.data.series.first?.values == [1, 1, 0, 0, 0, 0, 0, 0, 0, 3])
        #expect(edge.figure?.value == "—")
        #expect(edge.footnote == "1 review card without a memory state is not included.")
    }

    @Test func memoryBoxesWithNoMemoryState() throws {
        let none = StatsPartValue.retrievability(StatsRetrievability(byPercent: .empty, average: 0, sumByCard: 0, sumByNote: 0, leftOut: 3))
        let box = try Self.box(.retrievability, Fixture.presenter(Fixture.set(values: [.retrievability: none])))
        #expect(box.phase == .empty("No cards with a memory state", detail: nil))
        #expect(box.footnote == "3 review cards without a memory state are not included.")
    }

    @Test func byDeck() throws {
        let box = try Self.box(.byDeck)
        guard case let .byDeck(table)? = box.detail else { Issue.record("no table"); return }
        #expect(table.rows.map(\.name) == ["Words", "Places"])
        #expect(table.rows.map(\.timeShareText) == ["75.0%", "25.0%"])
        #expect(table.rows.map(\.retentionText) == ["80.0%", "—"])
        #expect(box.figure?.value == "75.0%")
        #expect(box.figure?.sentence == "of your time in Words")
        // A deck with no subdecks.
        let places = Fixture.presenter(Fixture.set(scope: .deck(2)), selection: StatsPageSelection(deck: 2, period: .oneMonth))
        #expect(try Self.box(.byDeck, places).phase == .empty("This deck has no subdecks", detail: nil))
    }

    @Test func emptyLinesNameThePeriodAndTheDeck() throws {
        let nothing = StatsPartValue.retentionByDay(StatsRetentionByDay(days: Fixture.map([-200: Fixture.retentionDay(1, 0, 0, 0)]),
                                                                        retentions: [0.9], several: false))
        let set = Fixture.set(scope: .deck(1), values: [.retentionByDay: nothing])
        let words = Fixture.presenter(set, selection: StatsPageSelection(deck: 1, period: .oneMonth))
        #expect(try Self.box(.retention, words).phase == .empty("No reviews in Words in the last month", detail: nil))
        let all = Fixture.presenter(Fixture.set(values: [.retentionByDay: nothing]), period: .threeMonths)
        #expect(try Self.box(.forgotten, all).phase == .empty("No reviews in the last 3 months", detail: nil))
        let never = Fixture.presenter(Fixture.set(oldest: .noRows, values: [.retentionByDay: nothing]), period: .oneMonth)
        #expect(try Self.box(.retention, never).phase == .empty("No reviews yet", detail: nil))
    }

    @Test func strip() throws {
        let p = Self.presenter
        #expect(p.strip(.studiedToday) == StatsStripFigure(value: "12", unit: "cards", detail: p.format.duration(seconds: 1_140),
                                                           loading: false, spoken: "12 cards, \(p.format.duration(seconds: 1_140))"))
        // 17 passed of 20.
        #expect(p.strip(.retentionToday)?.value == "85.0%")
        #expect(p.strip(.remainingToday)?.value == "20")
        #expect(p.strip(.remainingToday)?.detail == "4 tomorrow")
        #expect(p.strip(.streak)?.value == "3" && p.strip(.streak)?.unit == "days")
        #expect(p.strip(.mature)?.value == "60" && p.strip(.mature)?.detail == "of 100")
        let quiet = Fixture.presenter(Fixture.set(values: [.studiedToday: .studiedToday(StatsStudiedToday(answers: 0, cards: 0, timeMilliseconds: 0)),
                                                           .activityDays: .activityDays(.empty)]))
        #expect(quiet.strip(.studiedToday)?.value == "None yet")
        #expect(quiet.strip(.streak)?.value == "0" && quiet.strip(.streak)?.unit == "days")
        // A deck's own counts.
        let words = Fixture.presenter(Fixture.set(scope: .deck(1)), selection: StatsPageSelection(deck: 1, period: .oneMonth))
        #expect(words.strip(.remainingToday)?.value == "16")
    }

    @Test func everyTextComesFromTheCatalog() throws {
        // A key missing from the catalog would show as its own key ("stats.…").
        for period in StatsPagePeriod.allCases {
            let p = Fixture.presenter(Fixture.set(history: .allHistory), period: period)
            for id in StatsBoxID.allCases {
                let box = try Self.box(id, p)
                var texts = [box.figure?.value, box.figure?.sentence, box.footnote] + box.figures.flatMap { [$0.label, $0.value] }
                texts += box.data.series.map(\.name) + box.data.rules.map(\.name) + box.data.calloutLines.map(\.name)
                if case let .empty(line, detail) = box.phase { texts += [line, detail] }
                for text in texts.compactMap({ $0 }) { #expect(!text.contains("stats."), "\(id): \(text)") }
            }
            for id in StatsStripID.allCases {
                let figure = p.strip(id)
                for text in [figure?.value, figure?.unit, figure?.detail, figure?.spoken].compactMap({ $0 }) {
                    #expect(!text.contains("stats."), "\(id): \(text)")
                }
            }
        }
    }
}
