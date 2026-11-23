import CoreBridge
import Foundation
import HeatmapKit
import Testing

@testable import Stats

/// Activity's year blocks as the view draws them (`stats/statistics-page` S7.5, N25.8): one block
/// per year from the model's years, its days counted from 1 January, today's square in the
/// current year only, and AH7's levels from the block's own days.
@MainActor
@Suite struct StatsActivityBlocksTests {
    /// The fixture: today is 2026-10-12 (day 0); reviews on days −3, −2 and −1, and 400 days back.
    static func activity() throws -> StatsActivity {
        let box = try #require(Fixture.content(.activity, Fixture.presenter(Fixture.set())))
        guard case let .activity(activity)? = box.detail else { throw Missing() }
        return activity
    }

    struct Missing: Error {}

    @Test func oneBlockPerYearWithTodayInTheCurrentOne() throws {
        let activity = try Self.activity()
        let blocks = StatsActivityBox.blocks(activity, firstWeekday: 2)
        #expect(blocks.map(\.grid.year) == [2026, 2025])
        let current = try #require(blocks.first?.grid)
        // 2026-10-12 is the year's 285th day: day 284 from 1 January.
        #expect(current.today == 284 && current.drawnDays == 285)
        #expect(current.total == 15)
        #expect((281...283).map { current.count(day: $0) } == [4, 6, 5])
        #expect(current.count(day: 284) == 0, "today not studied yet")
        // 2026-01-01 is a Thursday: row 3 with weeks from Monday.
        #expect(current.firstRow == 3)
        let past = try #require(blocks.last?.grid)
        #expect(past.today == nil && past.drawnDays == 365 && past.total == 2)
        // 400 days before 2026-10-12 is 2025-09-07: day 249 of 2025.
        #expect(past.count(day: 249) == 2)
        // AH7 per block: the busiest day of each block is level 4.
        #expect(current.level(day: 282) == 4 && past.level(day: 249) == 4)
    }

    @Test func theWeekStartFollowsTheCalendar() throws {
        let activity = try Self.activity()
        #expect(StatsActivityBox.blocks(activity, firstWeekday: 1).first?.grid.firstRow == 4)
        #expect(StatsActivityBox.blocks(activity, firstWeekday: 2).first?.grid.firstRow == 3)
    }

    /// A pinned day is kept as an absolute day: after a rollover (every H1 day one further
    /// back) it still names the same date.
    @Test func aPinnedDayKeepsItsDateAcrossARollover() throws {
        let today = StatsActivity(years: [], currentStreak: 0, longestStreak: 0, daysStudiedThisYear: 0,
                                  days: StatsDays(context: Fixture.context(), calendar: Fixture.calendar()))
        // 2026-10-12 is day 20,738 since 1970-01-01.
        #expect(today.absoluteDay(ofDay: 0) == 20_738)
        let pinned = today.absoluteDay(ofDay: -2)
        let tomorrow = StatsActivity(years: [], currentStreak: 0, longestStreak: 0, daysStudiedThisYear: 0,
                                     days: StatsDays(context: Fixture.context(nextDayAt: Fixture.nextDayAt + 86_400),
                                                     calendar: Fixture.calendar()))
        #expect(tomorrow.day(ofAbsoluteDay: pinned) == -3)
        #expect(tomorrow.date(ofDay: -3) == today.date(ofDay: -2))
    }

    /// S14: Left and Right move one day, kept between the oldest block's 1 January and today.
    @Test func keysMoveOneDayWithinTheBlocks() {
        #expect(StatsActivityBox.moved(nil, by: -1, first: -650, last: 0) == 0, "none selected: today")
        #expect(StatsActivityBox.moved(-5, by: -1, first: -650, last: 0) == -6)
        #expect(StatsActivityBox.moved(-650, by: -1, first: -650, last: 0) == -650)
        #expect(StatsActivityBox.moved(0, by: 1, first: -650, last: 0) == 0)
    }

    /// A narrow grid scrolls a key's day into view: the day's column centred, kept inside the block.
    @Test func aRevealedDayIsCentredInsideTheBlock() {
        let grid = ActivityYearGrid(year: 2026, firstWeekday: 1, today: nil, counts: [])
        // Mac squares: column 30 starts at 360 pt; centred in a 300 pt viewport: 360 + 5 − 150.
        #expect(StatsActivityBox.revealOffset(column: 30, grid: grid, square: .mac, viewport: 300, rightToLeft: false) == 215)
        #expect(StatsActivityBox.revealOffset(column: 0, grid: grid, square: .mac, viewport: 300, rightToLeft: false) == 0)
        // The last column cannot scroll past the block's end (634 − 300).
        #expect(StatsActivityBox.revealOffset(column: 52, grid: grid, square: .mac, viewport: 300, rightToLeft: false) == 334)
        // Right to left: the first column is at the right edge.
        #expect(StatsActivityBox.revealOffset(column: 0, grid: grid, square: .mac, viewport: 300, rightToLeft: true) == 334)
    }
}
