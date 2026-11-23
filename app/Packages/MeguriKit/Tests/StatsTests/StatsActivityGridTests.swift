import CoreBridge
import Foundation
import HeatmapKit
import Testing

@testable import Stats

/// Activity's grid as the model gives it to the view (`stats/statistics-page` S7.5, N25.8,
/// Option E): one grid from the scope's first review to today, AH7's levels over all of it,
/// the system's first weekday; the keys of S14; a pinned day across a rollover.
@MainActor
@Suite struct StatsActivityGridTests {
    /// The fixture: today is Monday 2026-10-12 (day 0); reviews on days −3, −2 and −1, and
    /// 400 days back (H9's oldest review too); weeks start on Monday.
    static func activity() throws -> StatsActivity {
        let box = try #require(Fixture.content(.activity, Fixture.presenter(Fixture.set())))
        guard case let .activity(activity)? = box.detail else { throw Missing() }
        return activity
    }

    struct Missing: Error {}

    @Test func oneGridFromTheFirstReviewToToday() throws {
        let activity = try Self.activity()
        #expect(activity.span.firstDay == -400 && activity.span.years == [2026, 2025])
        #expect(activity.span.todayYear == 2026 && activity.span.todayMonth == 10 && activity.span.todayDay == 12)
        let grid = activity.grid
        #expect(grid.days == 401 && grid.total == 17)
        // 2025-09-07, 400 days back, is a Sunday: the last row of the first week.
        #expect(grid.firstRow == 6 && grid.todayRow == 0 && grid.columns == 59)
        #expect((1...3).map { grid.count(daysAgo: $0) } == [5, 6, 4])
        #expect(grid.count(daysAgo: 0) == 0, "today not studied yet")
        #expect(grid.count(daysAgo: 400) == 2)
        // AH7 over every day drawn: the busiest is level 4.
        #expect(grid.level(daysAgo: 2) == 4)
        #expect(activity.total == 17)
    }

    @Test func theWeekStartFollowsTheCalendar() throws {
        let presenter = Fixture.presenter(Fixture.set(), calendar: Fixture.calendar(firstWeekday: 1))
        let box = try #require(Fixture.content(.activity, presenter))
        guard case let .activity(activity)? = box.detail else { Issue.record("no activity"); return }
        #expect(activity.span.firstWeekday == 1 && activity.grid.firstRow == 0 && activity.grid.todayRow == 1)
    }

    /// The span the loading grid is sized from is the one the drawn grid uses (S10.3).
    @Test func theLoadingSpanIsTheGridsSpan() throws {
        let activity = try Self.activity()
        let days = StatsDays(context: Fixture.context(), calendar: Fixture.calendar())
        let span = StatsActivitySpan(firstDay: -400, days: days)
        #expect(span == activity.span)
        #expect(span.grid().columns == activity.grid.columns)
    }

    /// A pinned day is kept as an absolute day: after a rollover (every H1 day one further
    /// back) it still names the same date, on the same weekday row.
    @Test func aPinnedDayKeepsItsDateAcrossARollover() throws {
        let days = StatsDays(context: Fixture.context(), calendar: Fixture.calendar())
        let today = StatsActivity(span: StatsActivitySpan(firstDay: -10, days: days), counts: [:], years: [], currentStreak: 0,
                                  longestStreak: 0, daysStudiedThisYear: 0, days: days)
        // 2026-10-12 is day 20,738 since 1970-01-01.
        #expect(today.absoluteDay(ofDay: 0) == 20_738)
        let pinned = today.absoluteDay(ofDay: -2)
        let nextDays = StatsDays(context: Fixture.context(nextDayAt: Fixture.nextDayAt + 86_400), calendar: Fixture.calendar())
        let tomorrow = StatsActivity(span: StatsActivitySpan(firstDay: -11, days: nextDays), counts: [:], years: [],
                                     currentStreak: 0, longestStreak: 0, daysStudiedThisYear: 0, days: nextDays)
        #expect(tomorrow.day(ofAbsoluteDay: pinned) == -3)
        #expect(tomorrow.date(ofDay: -3) == today.date(ofDay: -2))
        #expect(tomorrow.grid.cell(daysAgo: 3).row == today.grid.cell(daysAgo: 2).row)
    }

    /// N25.4: the callout sits beside the day where a side has room, else under or over it;
    /// never over the day, always inside the grid.
    @Test func theCalloutNeverCoversTheDay() {
        let grid = CGSize(width: 330, height: 400)
        let left = StatsActivityBox.calloutOrigin(day: CGRect(x: 20, y: 50, width: 16, height: 16), callout: 200, 60, in: grid)
        #expect(left == CGPoint(x: 44, y: 42), "first half: to its right")
        let right = StatsActivityBox.calloutOrigin(day: CGRect(x: 300, y: 50, width: 16, height: 16), callout: 200, 60, in: grid)
        #expect(right == CGPoint(x: 92, y: 42), "second half: to its left")
        // Neither side has room for 200 pt beside a day in the middle: under it.
        let middle = CGRect(x: 160, y: 50, width: 16, height: 16)
        let under = StatsActivityBox.calloutOrigin(day: middle, callout: 200, 60, in: grid)
        #expect(under == CGPoint(x: 68, y: 74))
        #expect(!CGRect(origin: under, size: CGSize(width: 200, height: 60)).intersects(middle))
        // At the grid's bottom edge: over it.
        let lowest = CGRect(x: 160, y: 380, width: 16, height: 16)
        let over = StatsActivityBox.calloutOrigin(day: lowest, callout: 200, 60, in: grid)
        #expect(over == CGPoint(x: 68, y: 312))
        // A grid too short for it above or below: under the day all the same, past the grid.
        let short = CGSize(width: 330, height: 100)
        let past = StatsActivityBox.calloutOrigin(day: CGRect(x: 160, y: 40, width: 16, height: 16), callout: 200, 80, in: short)
        #expect(past == CGPoint(x: 68, y: 64))
    }

    /// S14 on the grid: Left and Right a week (the square beside), Up and Down a day (the
    /// square above or below), kept between the first day and today; none selected: today.
    @Test func keysMoveAWeekSidewaysAndADayUpOrDown() {
        typealias Box = StatsActivityBox
        #expect(Box.moved(nil, key: .left, first: -650, last: 0) == 0, "none selected: today")
        #expect(Box.moved(-20, key: .left, first: -650, last: 0) == -27)
        #expect(Box.moved(-20, key: .right, first: -650, last: 0) == -13)
        #expect(Box.moved(-20, key: .up, first: -650, last: 0) == -21)
        #expect(Box.moved(-20, key: .down, first: -650, last: 0) == -19)
        #expect(Box.moved(-645, key: .left, first: -650, last: 0) == -650, "kept at the first day")
        #expect(Box.moved(-3, key: .right, first: -650, last: 0) == 0, "kept at today")
        #expect(Box.moved(0, key: .down, first: -650, last: 0) == 0)
        // Right to left: the weeks run from the right.
        #expect(Box.moved(-20, key: .left, first: -650, last: 0, rightToLeft: true) == -13)
        #expect(Box.moved(-20, key: .right, first: -650, last: 0, rightToLeft: true) == -27)
        #expect(Box.moved(-20, key: .up, first: -650, last: 0, rightToLeft: true) == -21)
    }
}
