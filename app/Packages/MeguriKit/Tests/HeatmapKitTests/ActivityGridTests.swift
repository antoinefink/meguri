import CoreGraphics
import Testing
@testable import HeatmapKit

@Suite struct ActivityGridTests {
    /// `stats/activity-heatmap` AH7's table.
    @Test(arguments: [
        (20, 0, 0), (20, 1, 1), (20, 5, 1), (20, 6, 2), (20, 10, 2), (20, 11, 3), (20, 15, 3), (20, 16, 4), (20, 20, 4),
        (400, 1, 1), (400, 100, 1), (400, 101, 2), (400, 399, 4), (400, 400, 4), (1, 1, 4),
    ] as [(UInt32, UInt32, Int)])
    func levelsAreQuartileStepsOfTheBusiestDayShown(maximum: UInt32, count: UInt32, level: Int) {
        #expect(ActivityGrid.level(count: count, maximum: maximum) == level)
    }

    @Test func wholeWeeksFillTheWidthEdgeToEdge() {
        // An iPhone row (about 322 pt) at the touch pitch: 16 weeks.
        let phone = ActivityGrid.Geometry(width: 322, minimumPitch: 20)
        #expect(phone.weeks == 16)
        // The Mac's pitch packs 53 weeks into 700 pt and never more.
        #expect(ActivityGrid.Geometry(width: 700, minimumPitch: 13).weeks == 53)
        #expect(ActivityGrid.Geometry(width: 2_000, minimumPitch: 13).weeks == 53)
        for width in stride(from: CGFloat(40), through: 1_400, by: 37) {
            for pitch in [CGFloat(13), 20] {
                let geometry = ActivityGrid.Geometry(width: width, minimumPitch: pitch)
                let used = CGFloat(geometry.weeks) * geometry.side + CGFloat(geometry.weeks - 1) * geometry.gap
                #expect(abs(used - width) < 0.001, "\(width) \(pitch)")
                #expect(geometry.side + geometry.gap >= pitch - 0.001 || geometry.weeks == 1)
                if geometry.weeks < ActivityGrid.maximumWeeks {
                    // One more week would not fit at the minimum pitch.
                    let tighter = width / (CGFloat(geometry.weeks + 1) * 1.2 - 0.2)
                    #expect(tighter * 1.2 < pitch)
                }
                #expect(abs(geometry.height - (7 * geometry.side + 6 * geometry.gap)) < 0.001)
            }
        }
        #expect(ActivityGrid.Geometry(width: 0, minimumPitch: 20).height == 0)
    }

    @Test func daysSitByWeekdayWithTheNewestWeekTrailing() {
        // Today is weekday 4 and weeks start on weekday 1: today sits in row 3 of the last column.
        let counts = (0..<371).map { UInt32($0) }
        let grid = ActivityGrid(counts: counts, weeks: 3, todayWeekday: 4, firstWeekday: 1)
        #expect(grid.todayRow == 3)
        #expect(grid.daysAgo(column: 2, row: 3) == 0)
        #expect(grid.daysAgo(column: 2, row: 0) == 3)
        #expect(grid.daysAgo(column: 2, row: 4) == nil, "the next weekday is after today")
        #expect(grid.daysAgo(column: 1, row: 6) == 4)
        #expect(grid.daysAgo(column: 0, row: 0) == 17)
        // Shown: days 0 to 17. Total and maximum cover only those.
        #expect(grid.total == (0...17).reduce(0, +))
        #expect(grid.maximum == 17)
        #expect(grid.level(daysAgo: 17) == 4)
        #expect(grid.level(daysAgo: 0) == 0)
        // Weeks starting on weekday 2 put weekday 4 in row 2; starting on weekday 7, in row 4.
        #expect(ActivityGrid(counts: counts, weeks: 1, todayWeekday: 4, firstWeekday: 2).todayRow == 2)
        #expect(ActivityGrid(counts: counts, weeks: 1, todayWeekday: 4, firstWeekday: 7).todayRow == 4)
    }

    @Test func fiftyThreeWeeksReachTheLastDayTheCoreReturns() {
        let counts = [UInt32](repeating: 1, count: 371)
        let grid = ActivityGrid(counts: counts, weeks: 99, todayWeekday: 7, firstWeekday: 1)
        #expect(grid.weeks == 53)
        #expect(grid.daysAgo(column: 0, row: 0) == 370)
        #expect(grid.total == 371)
        // An empty period shown hides the heatmap even with older reviews.
        var older = [UInt32](repeating: 0, count: 371)
        older[200] = 5
        #expect(ActivityGrid(counts: older, weeks: 16, todayWeekday: 4, firstWeekday: 1).total == 0)
        #expect(ActivityGrid(counts: older, weeks: 53, todayWeekday: 4, firstWeekday: 1).total == 5)
    }
}
