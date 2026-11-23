import CoreGraphics
import Testing
@testable import HeatmapKit

/// The narrow page's shorter Activity grid (the coordinator's decision of 2026-11-23, pending the
/// owner's look): only the lines holding the last 53 weeks, rounded up to whole lines so the block
/// starts at a line's first column and today stays at the bottom right. Today is Monday
/// 2026-11-23, weeks from Monday, at the touch squares (19 pt a column).
@Suite struct ActivityRecentLinesTests {
    static func grid(days: Int) -> ActivityContinuousGrid {
        ActivityContinuousGrid(year: 2026, month: 11, day: 23, days: days, firstWeekday: 2, counts: [])
    }

    static func geometry(width: CGFloat, columns: Int, recent: Int? = 53) -> ActivityContinuousGrid.Geometry {
        .init(width: width, columns: columns, square: .touch, band: 13, labelGap: 4, lineGap: 8, recentWeeks: recent)
    }

    /// Six years: 314 week columns.
    static let sixYears = grid(days: 2_192)

    @Test(arguments: [
        // width, columns a line, first column shown, lines shown
        (311.0, 16, 256, 4),
        (360.0, 19, 247, 4),
        (398.0, 21, 252, 3),
        (240.0, 12, 252, 6),
    ])
    func theRecentLinesStartAtALinesFirstColumn(width: CGFloat, perLine: Int, first: Int, lines: Int) {
        let geometry = Self.geometry(width: width, columns: Self.sixYears.columns)
        #expect(geometry.perLine == perLine)
        #expect(geometry.firstColumn == first)
        #expect(geometry.firstColumn % geometry.perLine == 0, "The block starts at a line's first column")
        #expect(Self.sixYears.columns - geometry.firstColumn >= 53, "At least the last 53 weeks")
        #expect(Self.sixYears.columns - geometry.firstColumn < 53 + geometry.perLine, "Rounded up by less than a line")
        #expect(geometry.lines == lines)
        #expect(geometry.hidesColumns)
        // Today, in the last column, is on the last line shown.
        #expect(geometry.place(ofColumn: Self.sixYears.columns - 1).line == geometry.lines - 1)
        // The first column shown is the first line's first slot; the one before it is not drawn.
        #expect(geometry.place(ofColumn: geometry.firstColumn) == (0, 0))
        #expect(!geometry.shows(column: geometry.firstColumn - 1))
        // The full grid at the same width is taller by the lines left out.
        let full = Self.geometry(width: width, columns: Self.sixYears.columns, recent: nil)
        #expect(full.lines == geometry.lines + geometry.firstColumn / geometry.perLine)
    }

    @Test(arguments: [7, 300, 365, 371, 400])
    func aHistoryOfAYearOrLittleMoreHidesNothing(days: Int) {
        // 53 or 54 week columns, or a few more than 53 but less than a line more: nothing to show.
        let grid = Self.grid(days: days)
        for width in [240.0, 311.0, 398.0] {
            let geometry = Self.geometry(width: width, columns: grid.columns)
            #expect(geometry.firstColumn == 0 && !geometry.hidesColumns, "\(days) days at \(width)")
            #expect(grid.visibleDays(geometry) == 0..<grid.days)
        }
    }

    @Test func onlyTheDaysShownCanBeHitAndAreElements() throws {
        let grid = Self.sixYears
        let geometry = Self.geometry(width: 311, columns: grid.columns)
        // Column 256 holds the week of Monday 2025-10-20: 399 days ago, the oldest day shown.
        let shown = grid.visibleDays(geometry)
        #expect(shown == 0..<400)
        #expect(grid.date(daysAgo: 399) == (2025, 10, 20))
        // The first line's first square is that day; nothing above the first line is hit.
        #expect(grid.daysAgo(at: CGPoint(x: 2, y: geometry.rowsTop(0) + 2), geometry: geometry) == 399)
        #expect(grid.daysAgo(at: CGPoint(x: 2, y: 2), geometry: geometry) == nil, "the label band")
        // Every day shown is hit at its own square, and only those.
        for daysAgo in [0, 1, 6, 7, 200, 399] {
            let rect = grid.rect(daysAgo: daysAgo, geometry: geometry)
            #expect(grid.daysAgo(at: CGPoint(x: rect.midX, y: rect.midY), geometry: geometry) == daysAgo)
        }
        // The months shown begin with October 2025 cut at the 20th; none before it.
        let months = grid.visibleMonths(geometry)
        let first = try #require(months.first)
        #expect(first.year == 2025 && first.month == 10 && !first.startsHere && first.daysAgo.upperBound == 400)
        #expect(months.last?.year == 2026 && months.last?.month == 11)
        #expect(months.reduce(0) { $0 + $1.daysAgo.count } == 400)
    }

    @Test func theFirstLineShownNamesItsYear() {
        let grid = Self.sixYears
        let geometry = Self.geometry(width: 311, columns: grid.columns)
        let labels = grid.labels(geometry, monthWidth: { _ in 20 }, yearWidth: { _ in 28 })
        let first = labels.items.first
        #expect(first?.kind == .year(2025) && first?.line == 0 && first?.x == 0 && first?.rule == nil,
                "The first line shown carries its year, as the grid's first column does")
        // 1 January 2026 is on a line shown, with its rule; nothing names a year before 2025.
        #expect(labels.items.contains { $0.kind == .year(2026) && $0.rule != nil })
        #expect(labels.items.allSatisfy { $0.line >= 0 && $0.line < geometry.lines })
        #expect(!labels.items.contains { if case let .year(year) = $0.kind { year < 2025 } else { false } })
        // November 2025's 1st, in the next column, gives way to the year; December's is named.
        #expect(!labels.items.contains { $0.kind == .month(11) && $0.line == 0 })
        #expect(labels.items.contains { $0.kind == .month(12) && $0.line == 0 && $0.column == 262 })
    }

    @Test func withoutRecentWeeksTheGridIsUnchanged() {
        let grid = Self.sixYears
        let full = Self.geometry(width: 311, columns: grid.columns, recent: nil)
        #expect(full.firstColumn == 0 && full.lines == 20)
        #expect(grid.visibleDays(full) == 0..<grid.days)
        #expect(grid.visibleMonths(full) == grid.months)
    }
}
