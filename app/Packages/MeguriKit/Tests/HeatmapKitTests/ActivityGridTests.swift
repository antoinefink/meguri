import CoreGraphics
import Testing
@testable import HeatmapKit

@Suite struct ActivityGridTests {
    /// `stats/activity-heatmap` AH7's table: the days studied, the thresholds, and each day's level.
    @Test(arguments: [
        ([10, 20, 30, 40, 50, 60, 70, 80], [20, 40, 60], [10: 1, 20: 1, 30: 2, 40: 2, 50: 3, 60: 3, 70: 4, 80: 4]),
        ([150, 150, 160, 170, 180, 1_400], [150, 160, 170], [150: 1, 160: 2, 170: 3, 180: 4, 1_400: 4]),
        ([1, 1, 1, 100], [1, 1, 1], [1: 1, 100: 4]),
        ([7], [7, 7, 7], [7: 4]),
        ([5, 5, 5], [5, 5, 5], [5: 4]),
        ([3, 9], [3, 3, 3], [3: 1, 9: 4]),
    ] as [([UInt32], [UInt32], [UInt32: Int])])
    func levelsSplitTheDaysStudiedIntoQuarters(studied: [UInt32], thresholds: [UInt32], levels: [UInt32: Int]) {
        // Days with no review and the order of the days change nothing.
        // A fixed order unlike the sorted one, with empty days among them.
        let days: [UInt32] = [0] + Array(studied.reversed()) + [0, 0]
        let found = ActivityGrid.Levels(counts: days)
        #expect([found.t1, found.t2, found.t3] == thresholds)
        #expect(found.maximum == studied.max())
        for (count, level) in levels { #expect(found.level(count) == level, "\(count)") }
        #expect(found.level(0) == 0)
    }

    @Test func levelsComeFromTheDaysShownOnly() {
        // One catch-up day outside the weeks shown does not move the days inside them.
        var counts = [UInt32](repeating: 0, count: 371)
        for day in 0..<8 { counts[day] = UInt32(10 * (day + 1)) }
        counts[200] = 1_400
        let grid = ActivityGrid(counts: counts, weeks: 2, todayWeekday: 7, firstWeekday: 1)
        #expect(grid.maximum == 80)
        #expect(grid.level(daysAgo: 0) == 1 && grid.level(daysAgo: 7) == 4)
        #expect(ActivityGrid(counts: [UInt32](repeating: 0, count: 371), weeks: 53, todayWeekday: 1, firstWeekday: 1).maximum == 0)
    }

    /// N24.6: the accent's opacity per level, with and without Increase Contrast; level 0 keeps the fill.
    @Test func levelOpacitiesAreFixed() {
        #expect((0...4).map { ActivityGrid.opacity(level: $0, increasedContrast: false) } == [nil, 0.35, 0.55, 0.8, 1])
        #expect((0...4).map { ActivityGrid.opacity(level: $0, increasedContrast: true) } == [nil, 0.4, 0.6, 0.8, 1])
        // Dark: stronger steps, so level 1 stands clear of the empty day's fill.
        for increased in [false, true] {
            #expect((0...4).map { ActivityGrid.opacity(level: $0, increasedContrast: increased, dark: true) } == [nil, 0.5, 0.68, 0.84, 1])
        }
    }

    // MARK: Month labels (AH9)

    /// Every label 15 pt wide unless a test says otherwise, columns 10 pt apart, the grid
    /// ending 2 pt short of its last column's pitch. Each label as [column, month, x].
    private func labels(_ grid: ActivityGrid, _ date: (Int, Int, Int), labelWidth: CGFloat = 15,
                        rightToLeft: Bool = false) -> [[Int]] {
        grid.monthLabels(year: date.0, month: date.1, day: date.2, pitch: 10, width: CGFloat(grid.weeks) * 10 - 2,
                         rightToLeft: rightToLeft) { _ in labelWidth }
            .map { [$0.column, $0.month, Int($0.x.rounded())] }
    }

    @Test func aFirstOfTheMonthLabelsItsColumn() {
        // 2026-11-21, weeks from weekday 1: ten columns from 2026-09-13. Month 10 starts in
        // column index 2 and month 11 in column index 7; column index 0 has three columns of month 9.
        let grid = ActivityGrid(counts: [], weeks: 10, todayWeekday: 7, firstWeekday: 1)
        #expect(labels(grid, (2026, 11, 21)) == [[0, 9, 0], [2, 10, 20], [7, 11, 70]])
        // Weeks from weekday 2 move 2026-11-01 (weekday 1) to the column before.
        let later = ActivityGrid(counts: [], weeks: 10, todayWeekday: 7, firstWeekday: 2)
        #expect(labels(later, (2026, 11, 21)).map { $0[0] }.contains(6))
    }

    @Test func aFirstAfterTodayInTheCurrentWeekLabelsNothing() {
        // 2026-06-29 (weekday 2): the current week holds 2026-07-01, which is not drawn yet.
        let early = ActivityGrid(counts: [], weeks: 2, todayWeekday: 2, firstWeekday: 1)
        #expect(labels(early, (2026, 6, 29)).isEmpty)
        // On 2026-07-01 (weekday 4) that day is drawn and labels the current week. Its label
        // would run past the grid's edge (10 + 15 > 18): it is pulled back to end there.
        let midweek = ActivityGrid(counts: [], weeks: 2, todayWeekday: 4, firstWeekday: 1)
        #expect(labels(midweek, (2026, 7, 1)) == [[1, 7, 3]])
        // A label that fits stays at its column.
        #expect(labels(midweek, (2026, 7, 1), labelWidth: 8) == [[1, 7, 10]])
    }

    @Test func theFirstColumnNeedsThreeColumnsOfItsMonth() {
        // From 2026-10-18: two columns of month 10 before the one holding 2026-11-01, so no label for it.
        let two = ActivityGrid(counts: [], weeks: 5, todayWeekday: 7, firstWeekday: 1)
        #expect(labels(two, (2026, 11, 21)) == [[2, 11, 20]])
        // From 2026-10-11: three, so the first column names month 10.
        let three = ActivityGrid(counts: [], weeks: 6, todayWeekday: 7, firstWeekday: 1)
        #expect(labels(three, (2026, 11, 21)) == [[0, 10, 0], [3, 11, 30]])
    }

    @Test func aLabelThatWouldOverlapIsLeftOut() {
        let grid = ActivityGrid(counts: [], weeks: 10, todayWeekday: 7, firstWeekday: 1)
        // 25 pt labels: month 11 (x 70) and month 10 (x 20) fit; month 9 at x 0 would reach
        // month 10's start, so it is left out. Placement runs from the newest column back.
        #expect(labels(grid, (2026, 11, 21), labelWidth: 25) == [[2, 10, 20], [7, 11, 70]])
        // 29 pt: the newest would run past the edge (70 + 29 > 98), so it ends at the edge
        // (x 69), which still clears the label before it (20 + 29).
        #expect(labels(grid, (2026, 11, 21), labelWidth: 29) == [[2, 10, 20], [7, 11, 69]])
        // 60 pt: pulled back to x 38 it would overlap the label before it (20 + 60), so the
        // newest is left out and the one before it stays.
        #expect(labels(grid, (2026, 11, 21), labelWidth: 60) == [[2, 10, 20]])
    }

    @Test func rightToLeftMirrorsTheLabels() {
        // Columns run from the right: each label ends where its column's leading edge is.
        let grid = ActivityGrid(counts: [], weeks: 10, todayWeekday: 7, firstWeekday: 1)
        #expect(labels(grid, (2026, 11, 21), rightToLeft: true) == [[0, 9, 83], [2, 10, 63], [7, 11, 13]])
        #expect(labels(grid, (2026, 11, 21), labelWidth: 29, rightToLeft: true) == [[2, 10, 49], [7, 11, 0]])
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
