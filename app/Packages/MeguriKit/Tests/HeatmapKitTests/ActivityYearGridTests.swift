import CoreGraphics
import Testing
@testable import HeatmapKit

/// The Statistics page's year blocks (`stats/statistics-page` S7.5, N25.8): Today's squares,
/// opacities and AH7 levels over one calendar year from 1 January.
@Suite struct ActivityYearGridTests {
    @Test func aYearStartsOnItsWeekdayRow() {
        // 2026-01-01 is a Thursday (weekday 5): row 4 with weeks from Sunday, row 3 from Monday.
        let sunday = ActivityYearGrid(year: 2026, firstWeekday: 1, today: nil, counts: [])
        #expect(sunday.firstRow == 4 && sunday.days == 365 && sunday.weeks == 53)
        #expect(sunday.day(column: 0, row: 3) == nil, "before 1 January")
        #expect(sunday.day(column: 0, row: 4) == 0)
        #expect(sunday.day(column: 1, row: 0) == 3)
        // 2026-12-31 (day 364) is a Thursday too: row 4 of the last column; the rows after it are empty.
        #expect(sunday.day(column: 52, row: 4) == 364)
        #expect(sunday.day(column: 52, row: 5) == nil)
        let monday = ActivityYearGrid(year: 2026, firstWeekday: 2, today: nil, counts: [])
        #expect(monday.firstRow == 3)
        #expect(monday.cell(ofDay: 0) == (0, 3) && monday.cell(ofDay: 4) == (1, 0))
    }

    @Test func aLeapYearStartingOnTheLastWeekdayNeedsFiftyFourWeeks() {
        // 2028-01-01 is a Saturday (weekday 7) and 2028 has 366 days.
        let grid = ActivityYearGrid(year: 2028, firstWeekday: 1, today: nil, counts: [])
        #expect(grid.days == 366 && grid.firstRow == 6 && grid.weeks == 54)
        #expect(grid.day(column: 53, row: 0) == 365)
        #expect(Int(grid.width(.mac).rounded()) == 646, "54 columns of 12 pt, less the last gap")
        // From Monday the same year fits 53.
        #expect(ActivityYearGrid(year: 2028, firstWeekday: 2, today: nil, counts: []).weeks == 53)
    }

    @Test func theCurrentYearDrawsUpToToday() {
        var counts = [UInt32](repeating: 0, count: 365)
        counts[10] = 3
        counts[324] = 5
        counts[330] = 1_000 // after today: never drawn, never counted
        let grid = ActivityYearGrid(year: 2026, firstWeekday: 1, today: 324, counts: counts)
        #expect(grid.drawnDays == 325 && grid.today == 324)
        let today = grid.cell(ofDay: 324)
        #expect(grid.day(column: today.column, row: today.row) == 324)
        #expect(grid.day(column: today.column, row: today.row + 1) == nil, "tomorrow is not drawn")
        #expect(grid.total == 8 && grid.levels.maximum == 5)
        #expect(grid.level(day: 324) == 4 && grid.level(day: 10) == 1 && grid.level(day: 11) == 0)
        // The block keeps the whole year's columns, so stacked blocks share one width.
        #expect(grid.weeks == 53)
        #expect(grid.months.map(\.month) == Array(1...11))
        #expect(grid.months.last?.days == 304..<325)
    }

    @Test func levelsComeFromTheBlockAlone() {
        // AH7 per block: the same counts give the same levels in any year, whatever other years hold.
        let counts: [UInt32] = [10, 20, 30, 40, 50, 60, 70, 80]
        let grid = ActivityYearGrid(year: 2024, firstWeekday: 1, today: nil, counts: counts)
        #expect([grid.levels.t1, grid.levels.t2, grid.levels.t3] == [20, 40, 60])
        #expect((0..<8).map { grid.level(day: $0) } == [1, 1, 2, 2, 3, 3, 4, 4])
        #expect(grid.count(day: 400) == 0)
    }

    @Test func aPointFindsItsDay() {
        let grid = ActivityYearGrid(year: 2026, firstWeekday: 1, today: nil, counts: [])
        // Column 1, row 0 starts at x 12 on the Mac (10 pt squares, 2 pt gaps): day 3.
        #expect(grid.day(at: CGPoint(x: 13, y: 1), square: .mac) == 3)
        // The gap's halves belong to the squares beside it: row 4 holds 1 January (column 0) and 8 January.
        #expect(grid.day(at: CGPoint(x: 10.5, y: 49), square: .mac) == 0)
        #expect(grid.day(at: CGPoint(x: 11.5, y: 49), square: .mac) == 7)
        #expect(grid.day(at: CGPoint(x: 5, y: 1), square: .mac) == nil, "before 1 January")
        #expect(grid.day(at: CGPoint(x: -1, y: 1), square: .mac) == nil)
        // Right to left: the first column is at the right edge.
        let width = grid.width(.mac)
        #expect(grid.day(at: CGPoint(x: width - 5, y: 12 * 4 + 1), square: .mac, rightToLeft: true) == 0)
    }

    @Test func monthsGroupTheDaysDrawn() {
        let leap = ActivityYearGrid(year: 2024, firstWeekday: 1, today: nil, counts: [])
        #expect(leap.months.count == 12)
        #expect(leap.months[1].days == 31..<60, "29 days in a leap February")
        #expect(leap.months[11].days == 335..<366)
        #expect(leap.date(ofDay: 59) == (2, 29) && leap.date(ofDay: 365) == (12, 31))
    }

    /// Every label 15 pt wide, columns 12 pt apart (the Mac's pitch).
    private func labels(_ grid: ActivityYearGrid, labelWidth: CGFloat = 15, rightToLeft: Bool = false) -> [[Int]] {
        grid.monthLabels(pitch: 12, width: grid.width(.mac), rightToLeft: rightToLeft) { _ in labelWidth }
            .map { [$0.column, $0.month, Int($0.x.rounded())] }
    }

    @Test func eachDrawnFirstLabelsItsColumn() {
        // 2026 from Sunday: 1 January in column 0, 1 February (day 31) in column 5, … 1 December (day 334) in column 48.
        let grid = ActivityYearGrid(year: 2026, firstWeekday: 1, today: nil, counts: [])
        let found = labels(grid)
        #expect(found.count == 12)
        #expect(found.first == [0, 1, 0] && found[1] == [5, 2, 60] && found.last == [48, 12, 576])
        // In the current year a month not reached yet labels nothing.
        let current = ActivityYearGrid(year: 2026, firstWeekday: 1, today: 300, counts: [])
        #expect(labels(current).map { $0[1] } == Array(1...10))
    }

    @Test func overlappingLabelsGiveWayToNewerOnes() {
        let grid = ActivityYearGrid(year: 2026, firstWeekday: 1, today: nil, counts: [])
        // 70 pt labels: only every other month fits, placed from December back.
        let wide = labels(grid, labelWidth: 70).map { $0[1] }
        #expect(wide.last == 12)
        #expect(zip(wide, wide.dropFirst()).allSatisfy { $1 > $0 })
        #expect(wide.count < 12)
        // Right to left mirrors each label inside the block.
        let mirrored = labels(grid, rightToLeft: true)
        #expect(mirrored.first == [0, 1, Int(grid.width(.mac)) - 15])
    }
}

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
