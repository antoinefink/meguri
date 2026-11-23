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
}
