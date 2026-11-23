import CoreBridge
import Foundation
import Testing

@testable import Stats

/// `stats/statistics-page` S4's bins (S20's model tests): days from the result's context
/// (AH3's `T0`, the rollover hour, a zone change), weeks from the calendar's first weekday,
/// month and year boundaries, a leap day, rows after today, All's axis and its fallback,
/// Reviews Due ahead with Overdue and Later, and S4.3's windows.
@MainActor
@Suite struct StatsBinsTests {
    static func days(_ context: StatsContext = Fixture.context(), calendar: Calendar = Fixture.calendar()) -> StatsDays {
        StatsDays(context: context, calendar: calendar)
    }

    static func ymd(_ days: StatsDays, _ day: Int) -> [Int] {
        let c = days.display.dateComponents([.year, .month, .day], from: days.date(ofDay: day))
        return [c.year!, c.month!, c.day!]
    }

    // MARK: Days

    @Test func dayZeroIsTheDateDayZeroBeganOn() {
        let days = Self.days()
        #expect(Self.ymd(days, 0) == [2026, 10, 12])
        #expect(Self.ymd(days, -1) == [2026, 10, 11])
        #expect(Self.ymd(days, -366) == [2025, 10, 11])
    }

    @Test func theRolloverHourDecidesTheDate() {
        // N at 23:00 local with a rollover hour of 23: day 0 began at 23:00 on the 11th.
        let late = Fixture.context(nextDayAt: Fixture.nextDayAt - 5 * 3_600, rollover: 23)
        #expect(Self.ymd(Self.days(late), 0) == [2026, 10, 11])
        // Rollover at midnight: N is midnight of the 13th, day 0 the 12th.
        let midnight = Fixture.context(nextDayAt: Fixture.nextDayAt - 4 * 3_600, rollover: 0)
        #expect(Self.ymd(Self.days(midnight), 0) == [2026, 10, 12])
    }

    @Test func aZoneChangeNeverMovesAResultsDays() {
        // The result was read at +2; the device now shows Honolulu (−10). Day 0 is still the 12th,
        // and its date is written as the 12th in the new zone.
        let honolulu = Fixture.calendar(zone: "Pacific/Honolulu")
        let days = Self.days(calendar: honolulu)
        #expect(Self.ymd(days, 0) == [2026, 10, 12])
        let format = StatsChartFormat(locale: Fixture.locale, calendar: honolulu, now: days.date(ofDay: 0))
        let paris = StatsChartFormat(locale: Fixture.locale, calendar: Fixture.calendar(), now: days.date(ofDay: 0))
        #expect(format.shortDate(days.date(ofDay: 0)) == paris.shortDate(Self.days().date(ofDay: 0)))
        // A result read after the move (offset −10) has its own day 0.
        let after = Fixture.context(nextDayAt: Fixture.nextDayAt + 12 * 3_600, offset: -36_000)
        #expect(Self.ymd(Self.days(after, calendar: honolulu), 0) == [2026, 10, 12])
    }

    // MARK: Periods of the past (S4.1)

    @Test func oneMonthIsThirtyDays() {
        let axis = StatsAxes.past(.oneMonth, days: Self.days(), allFirstDay: -400)
        #expect(axis.bins.count == 30)
        #expect(axis.bins.first?.first == -29 && axis.bins.last?.first == 0 && axis.bins.last?.last == 0)
        #expect(axis.bins.allSatisfy { $0.singleDay != nil })
        #expect(axis.days == 30)
    }

    @Test(arguments: [(StatsPagePeriod.threeMonths, 13), (.sixMonths, 26), (.oneYear, 52)])
    func weeksStartOnTheCalendarsFirstWeekday(period: StatsPagePeriod, count: Int) throws {
        // T0 is a Monday. Monday first: the current week starts today.
        let monday = StatsAxes.past(period, days: Self.days(calendar: Fixture.calendar(firstWeekday: 2)), allFirstDay: -400)
        #expect(monday.bins.count == count)
        #expect(monday.bins.last.map { [$0.first, $0.last] } == [0, 0])
        #expect(monday.bins.first?.first == -7 * (count - 1))
        #expect(zip(monday.bins, monday.bins.dropFirst()).allSatisfy { $1.first == $0.last + 1 })
        // Sunday first: the current week started yesterday.
        let sundayDays = Self.days(calendar: Fixture.calendar(firstWeekday: 1))
        let sunday = StatsAxes.past(period, days: sundayDays, allFirstDay: -400)
        #expect(sunday.bins.count == count)
        #expect(sunday.bins.last.map { [$0.first, $0.last] } == [-1, 0])
        let start = try #require(sunday.bins.first)
        #expect(sundayDays.display.component(.weekday, from: sundayDays.date(ofDay: start.first)) == 1)
        #expect(sunday.bins.dropLast().allSatisfy { $0.last - $0.first == 6 })
    }

    @Test func allHasAMonthPerBarAcrossAYearBoundary() throws {
        let days = Self.days()
        // Day −400 is 7 Sept 2025: September 2025 to October 2026, 14 months.
        let axis = StatsAxes.past(.all, days: days, allFirstDay: -400)
        #expect(axis.bins.count == 14)
        #expect(axis.opensBefore)
        #expect(Self.ymd(days, axis.bins[0].first) == [2025, 9, 1])
        let december = axis.bins[3], january = axis.bins[4]
        #expect(Self.ymd(days, december.last) == [2025, 12, 31])
        #expect(Self.ymd(days, january.first) == [2026, 1, 1])
        #expect(january.first == december.last + 1)
        #expect(axis.bins.last.map { [$0.first, $0.last] } == [-11, 0])
        // Counts before the first month go into the first bar on All, nowhere on 1M.
        let map = Fixture.map([-1_000: 3.0, -1: 1])
        #expect(axis.sums(map) { $0 }.first == 3)
        let month = StatsAxes.past(.oneMonth, days: days, allFirstDay: -400)
        #expect(month.sums(map) { $0 }.reduce(0, +) == 1)
    }

    @Test func aLeapFebruaryHas29Days() throws {
        let days = Self.days()
        // 15 February 2024.
        let day = try #require(days.firstDay(ofYear: 2024)) + 45
        let axis = StatsAxes.past(.all, days: days, allFirstDay: day)
        let february = try #require(axis.bins.first)
        #expect(Self.ymd(days, february.first) == [2024, 2, 1])
        #expect(february.last - february.first + 1 == 29)
    }

    @Test func rowsAfterTodayCountToday() {
        let axis = StatsAxes.past(.oneMonth, days: Self.days(), allFirstDay: -400)
        let sums = axis.sums(Fixture.map([0: 1.0, 1: 2, 5: 4])) { $0 }
        #expect(sums.last == 7)
        #expect(axis.index(ofDay: -30) == nil)
    }

    @Test func allStartsAtTheOldestReviewMonthOrTheOldestCardWithNoReview() throws {
        let days = Self.days()
        // H9's oldest day decides.
        let reviewed = Fixture.presenter(Fixture.set(oldest: .day(-40)), period: .all)
        #expect(reviewed.pastAxis.bins.count == 2)
        // No review: the oldest creation day (Cards Over Time's first day, −700) …
        let none = Fixture.presenter(Fixture.set(oldest: .noRows), period: .all)
        #expect(Self.ymd(days, none.pastAxis.firstDay) == Self.ymd(days, days.monthStart(ofDay: -700)))
        // … never before January ten years back.
        var old = Fixture.set(oldest: .noRows)
        old = Fixture.set(oldest: .noRows, values: [
            .cardStatesOverTime: .cardStatesOverTime(StatsCardStatesOverTime(start: .zero, deltas: .empty, leftOut: 0,
                                                                             firstDay: -20_000)),
            .added: .added(Fixture.map([-20_000: 1])),
            .started: .started(StatsStarted(days: .empty, firstDay: nil)),
        ])
        let clamped = Fixture.presenter(old, period: .all)
        #expect(Self.ymd(days, clamped.pastAxis.firstDay) == [2016, 1, 1])
    }

    // MARK: Ahead (S4.2)

    @Test func reviewsDueAheadHasOverdueThenTheDays() {
        let days = Self.days()
        let due = Fixture.map([-2: 3, 0: 5, 1: 4, 40: 2])
        let month = StatsAxes.ahead(.oneMonth, days: days, due: due)
        #expect(month.bins.count == 31)
        #expect(month.bins.first?.period == .overdue)
        #expect(month.aheadSums(due).prefix(3) == [3, 5, 4])
        #expect(month.aheadSums(due).reduce(0, +) == 12)
        // No overdue card: no Overdue bar.
        #expect(StatsAxes.ahead(.oneMonth, days: days, due: Fixture.map([0: 1])).bins.first?.period != .overdue)
        // Weeks start with the current week, from today.
        let weeks = StatsAxes.ahead(.threeMonths, days: days, due: due)
        #expect(weeks.bins.count == 14)
        #expect(weeks.bins[1].first == 0 && weeks.bins[1].last == 6)
        #expect(weeks.aheadSums(due)[6] == 2)
    }

    @Test func reviewsDueAllLooksAtLeastAYearAndAtMostTwoAheadThenLater() {
        let days = Self.days()
        let soon = StatsAxes.ahead(.all, days: days, due: Fixture.map([0: 1, 40: 2]))
        #expect(soon.bins.count == 12)
        #expect(soon.bins.first?.first == 0)
        let far = Fixture.map([0: 1, 500: 2, 900: 4])
        let later = StatsAxes.ahead(.all, days: days, due: far)
        #expect(later.bins.count == 25)
        #expect(later.bins.last?.period == .later)
        #expect(later.aheadSums(far).last == 4)
        let mid = StatsAxes.ahead(.all, days: days, due: Fixture.map([0: 1, 500: 2]))
        #expect((12...24).contains(mid.bins.count) && mid.bins.last?.period != .later)
        #expect(mid.aheadSums(Fixture.map([0: 1, 500: 2])).reduce(0, +) == 3)
    }

    // MARK: Windows (S4.3)

    @Test(arguments: [(StatsPagePeriod.oneMonth, 1), (.threeMonths, 2), (.sixMonths, 3), (.oneYear, 4), (.all, 5)])
    func theChartsWithNoTimeAxisReadTheirPeriodsWindow(period: StatsPagePeriod, presses: Int) throws {
        func range(_ n: Int) -> StatsButtonPresses { StatsButtonPresses(learning: [n, 0, 0, 0], young: [0, 0, 0, 0], mature: [0, 0, 0, 0]) }
        let buttons = StatsPartValue.buttons(StatsButtons(ranges: StatsRanges(oneMonth: range(1), threeMonths: range(2),
                                                                              sixMonths: range(3), oneYear: range(4),
                                                                              allTime: range(5))))
        let set = Fixture.set(history: .allHistory, values: [.buttons: buttons])
        let content = try #require(Fixture.content(.answers, Fixture.presenter(set, period: period)))
        #expect(content.figure?.value == "\(presses)")
    }

    @Test func allWithOnlyTheLastYearHasNoAllTimeWindow() {
        let presenter = Fixture.presenter(Fixture.set(history: .last12Months), period: .all)
        if case .unavailable = presenter.box(.answers) {} else { Issue.record("Answers drew a year as all time") }
        if case .unavailable = presenter.box(.timeOfDay) {} else { Issue.record("Time of Day drew a year as all time") }
        if case .unavailable = presenter.box(.byDeck) {} else { Issue.record("By Deck drew a year as all time") }
    }
}
