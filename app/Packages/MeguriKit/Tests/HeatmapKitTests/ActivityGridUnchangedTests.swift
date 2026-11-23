import CoreGraphics
import Testing
@testable import HeatmapKit

/// Today's heatmap is not changed by the year blocks (plan U298): its grid's layout, levels
/// and month labels for fixed inputs, pinned as they were before the year blocks landed.
@Suite struct ActivityGridUnchangedTests {
    /// A fixed, uneven year of counts.
    private static let counts: [UInt32] = (0..<371).map { day in
        let value = (day * 37 + 11) % 23
        return day % 5 == 0 ? 0 : UInt32(value * (day % 3 + 1))
    }

    /// The grid's numbers, then how many cells have each level, then a checksum of every cell's
    /// level by its place (oldest week first, rows top to bottom; 9 for a day not drawn).
    private static func digest(_ grid: ActivityGrid) -> [Int] {
        var histogram = [0, 0, 0, 0, 0]
        var checksum = 0
        for column in 0..<grid.weeks {
            for row in 0..<7 {
                let level = grid.daysAgo(column: column, row: row).map { grid.level(daysAgo: $0) } ?? 9
                if level < 5 { histogram[level] += 1 }
                checksum = (checksum * 31 + level + 1) % 1_000_000_007
            }
        }
        return [grid.weeks, grid.todayRow, grid.total, Int(grid.levels.t1), Int(grid.levels.t2), Int(grid.levels.t3),
                Int(grid.maximum)] + histogram + [checksum]
    }

    @Test func todaysGridIsUnchanged() {
        let grid = ActivityGrid(counts: Self.counts, weeks: 53, todayWeekday: 3, firstWeekday: 1)
        #expect(Self.digest(grid) == [53, 2, 6_389, 10, 18, 34, 66, 87, 79, 63, 71, 67, 985_072_364])
        let labels = grid.monthLabels(year: 2026, month: 11, day: 24, pitch: 12, width: 634) { _ in 18 }
            .map { [$0.column, $0.month, Int($0.x.rounded())] }
        #expect(labels == [[1, 12, 12], [5, 1, 60], [10, 2, 120], [14, 3, 168], [18, 4, 216], [22, 5, 264], [27, 6, 324],
                           [31, 7, 372], [35, 8, 420], [40, 9, 480], [44, 10, 528], [49, 11, 588]])
        // A narrower width keeps the newest weeks and their own levels.
        let narrow = ActivityGrid(counts: Self.counts, weeks: 17, todayWeekday: 3, firstWeekday: 2)
        #expect(Self.digest(narrow) == [17, 1, 1_982, 9, 18, 33, 66, 27, 22, 22, 21, 22, 110_198_718])
    }
}
