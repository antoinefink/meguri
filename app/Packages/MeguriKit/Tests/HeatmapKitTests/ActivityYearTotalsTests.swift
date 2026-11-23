import CoreGraphics
import Testing
@testable import HeatmapKit

/// Each year's total beside its label on the grid (the owner's Option B, 2026-11-23): the whole
/// total where it fits before the next label and the line's end, else the number alone, else
/// none; never overlapping or past the edge; the months give way to it. 2025-12-01 (a Monday) to
/// Monday 2026-11-23 at the Mac's squares (12 pt a column): 1 January 2026 is in column 4.
@Suite struct ActivityYearTotalsTests {
    static let grid = ActivityContinuousGrid(year: 2026, month: 11, day: 23, days: 358, firstWeekday: 2, counts: [])

    static func labels(width: CGFloat, rightToLeft: Bool = false) -> [ActivityContinuousGrid.Label] {
        let geometry = ActivityContinuousGrid.Geometry(width: width, columns: grid.columns, square: .mac, band: 13,
                                                       labelGap: 4, lineGap: 8)
        return grid.labels(geometry, rightToLeft: rightToLeft, monthWidth: { _ in 15 }, yearWidth: { _ in 22 },
                           yearTotal: { _ in ActivityContinuousGrid.TotalWidths(full: 90, number: 40) }).items
    }

    static func year(_ year: Int, in labels: [ActivityContinuousGrid.Label]) -> ActivityContinuousGrid.Label? {
        labels.first { $0.kind == .year(year) }
    }

    @Test func aWideLineShowsTheWholeTotalAndTheMonthsGiveWay() throws {
        let labels = Self.labels(width: 1_600)
        let year = try #require(Self.year(2026, in: labels))
        #expect(year.total == .full && year.totalX == 77, "4 pt after the year's text (51 + 22)")
        // February's and March's names would run into the total: they give way; April's is clear.
        #expect(!labels.contains { $0.kind == .month(2) || $0.kind == .month(3) })
        #expect(labels.contains { $0.kind == .month(4) })
        // The grid's first year has no room before 1 January's rule: its total is left out.
        let first = try #require(Self.year(2025, in: labels))
        #expect(first.total == .none && first.totalX == nil)
    }

    @Test(arguments: [(1_600.0, ActivityContinuousGrid.Label.Total.full), (150.0, .number), (100.0, .none)])
    func theTotalShortensToTheNumberThenGoes(width: CGFloat, total: ActivityContinuousGrid.Label.Total) throws {
        let labels = Self.labels(width: width)
        let year = try #require(Self.year(2026, in: labels))
        #expect(year.total == total)
        if let x = year.totalX {
            let end = x + (total == .full ? 90 : 40)
            #expect(end <= width, "Never past the line's end")
            // Nothing else on the line starts inside it.
            #expect(!labels.contains { $0.line == year.line && $0.x > year.x && $0.x < end })
        }
    }

    @Test func theTotalMirrorsRightToLeft() throws {
        let year = try #require(Self.year(2026, in: Self.labels(width: 1_600, rightToLeft: true)))
        #expect(year.total == .full && year.totalX == 1_600 - 77 - 90)
    }

    @Test func withoutTotalsTheLabelsAreUnchanged() {
        let geometry = ActivityContinuousGrid.Geometry(width: 1_600, columns: Self.grid.columns, square: .mac, band: 13,
                                                       labelGap: 4, lineGap: 8)
        let labels = Self.grid.labels(geometry, monthWidth: { _ in 15 }, yearWidth: { _ in 22 }).items
        #expect(labels.allSatisfy { $0.total == .none && $0.totalX == nil })
        #expect(labels.contains { $0.kind == .month(2) })
    }
}
