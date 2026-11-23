import CoreGraphics
import Testing
@testable import HeatmapKit

/// The Statistics page's Activity grid (`stats/statistics-page` S7.5, N25.8, Option E): one grid
/// for all of history, wrapped onto lines like text. Today is Monday 2026-11-23 throughout.
@Suite struct ActivityContinuousGridTests {
    static let sunday = 1
    static let monday = 2

    static func grid(days: Int, firstWeekday: Int = monday, counts: [UInt32] = []) -> ActivityContinuousGrid {
        ActivityContinuousGrid(year: 2026, month: 11, day: 23, days: days, firstWeekday: firstWeekday, counts: counts)
    }

    /// 2020-11-23 to 2026-11-23: six years, 313 weeks apart, so the first day is a Monday too.
    static let sixYears = 2_192

    static func geometry(width: CGFloat, columns: Int, square: ActivityGrid.Square = .mac,
                         band: CGFloat = 13) -> ActivityContinuousGrid.Geometry {
        .init(width: width, columns: columns, square: square, band: band, labelGap: 4, lineGap: 8)
    }

    // MARK: Days

    @Test func oneWeekStartingMidWeek() {
        // Tuesday 17 to Monday 23: weeks from Sunday put the first day on row 2.
        let sunday = Self.grid(days: 7, firstWeekday: Self.sunday)
        #expect(sunday.todayRow == 1 && sunday.firstRow == 2 && sunday.columns == 2)
        #expect(sunday.cell(daysAgo: 6) == (0, 2) && sunday.cell(daysAgo: 0) == (1, 1))
        #expect(sunday.daysAgo(column: 0, row: 6) == 2, "Saturday 21")
        #expect(sunday.daysAgo(column: 0, row: 1) == nil, "before the first day")
        #expect(sunday.daysAgo(column: 1, row: 2) == nil, "after today")
        // Weeks from Monday: today opens its week.
        let monday = Self.grid(days: 7)
        #expect(monday.todayRow == 0 && monday.firstRow == 1 && monday.columns == 2)
        #expect(monday.cell(daysAgo: 0) == (1, 0))
        // One day: one column.
        #expect(Self.grid(days: 1).columns == 1)
    }

    @Test func fiftyThreeWeeks() {
        // 371 days that do not start a week take 54 columns; a year to a Monday from Monday, 53.
        #expect(Self.grid(days: 371, firstWeekday: Self.sunday).columns == 54)
        #expect(Self.grid(days: 371).columns == 54)
        #expect(Self.grid(days: 365).firstRow == 0 && Self.grid(days: 365).columns == 53)
    }

    @Test func sixYearsAndTodaysCell() {
        let grid = Self.grid(days: Self.sixYears)
        #expect(grid.firstRow == 0 && grid.columns == 314)
        #expect(grid.cell(daysAgo: 0) == (313, 0))
        #expect(grid.daysAgo(column: 313, row: 1) == nil, "tomorrow is not drawn")
        #expect(grid.date(daysAgo: Self.sixYears - 1) == (2020, 11, 23))
        #expect(grid.date(daysAgo: 0) == (2026, 11, 23))
    }

    @Test func levelsAreOverAllOfHistory() {
        var counts = [UInt32](repeating: 0, count: 401)
        counts[0] = 1
        counts[3] = 10
        counts[400] = 2
        let grid = Self.grid(days: 401, counts: counts)
        #expect(grid.total == 13 && grid.levels.maximum == 10)
        #expect(grid.level(daysAgo: 3) == 4 && grid.level(daysAgo: 1) == 0 && grid.level(daysAgo: 400) > 0)
    }

    @Test func monthsCoverEveryDay() {
        let week = Self.grid(days: 7)
        #expect(week.months.count == 1 && week.months[0].startsHere == false && week.months[0].daysAgo == 0..<7)
        let grid = Self.grid(days: Self.sixYears)
        // November 2020 to November 2026.
        #expect(grid.months.count == 73)
        #expect(grid.months.first?.startsHere == false && grid.months.dropFirst().allSatisfy(\.startsHere))
        #expect(grid.months.first?.daysAgo.upperBound == Self.sixYears && grid.months.last?.daysAgo.lowerBound == 0)
        #expect(zip(grid.months, grid.months.dropFirst()).allSatisfy { $0.daysAgo.lowerBound == $1.daysAgo.upperBound })
        #expect(grid.months.last?.year == 2026 && grid.months.last?.month == 11)
        // A grid that starts on a 1st.
        #expect(Self.grid(days: 23).months.map(\.startsHere) == [true])
    }

    // MARK: Lines

    @Test func columnsPerLineForManyWidths() {
        let cases: [(CGFloat, Int)] = [(0, 1), (10, 1), (46, 4), (58, 5), (633, 52), (634, 53), (1_600, 133)]
        for (width, columns) in cases {
            #expect(ActivityContinuousGrid.Geometry.perLine(width: width, square: .mac) == columns, "width \(width)")
        }
        // Touch squares: 19 pt a column.
        #expect(ActivityContinuousGrid.Geometry.perLine(width: 361, square: .touch) == 19)
    }

    @Test func linesForManyHistories() {
        // (days, width, lines) with Mac squares, weeks from Monday.
        let cases: [(Int, CGFloat, Int)] = [
            (1, 560, 1), (7, 560, 1), (371, 1_600, 1), (371, 646, 1), (371, 634, 2), (365, 634, 1), (365, 633, 2), (371, 560, 2),
            (Self.sixYears, 1_600, 3), (Self.sixYears, 560, 7), (Self.sixYears, 46, 79),
        ]
        for (days, width, lines) in cases {
            let grid = Self.grid(days: days)
            #expect(Self.geometry(width: width, columns: grid.columns).lines == lines, "\(days) days at \(width)")
        }
        // The iPhone's column: six years in 17 lines of 19 weeks.
        let touch = Self.geometry(width: 361, columns: Self.grid(days: Self.sixYears).columns, square: .touch, band: 16)
        #expect(touch.perLine == 19 && touch.lines == 17)
        #expect(touch.height == CGFloat(17 * 150 + 16 * 8))
    }

    @Test func heightAndTheBandFromTheSize() {
        let geometry = Self.geometry(width: 634, columns: 54)
        #expect(geometry.perLine == 53 && geometry.lines == 2)
        #expect(geometry.lineHeight == 99 && geometry.height == 206)
        #expect(geometry.lineTop(1) == 107 && geometry.rowsTop(1) == 124)
        let drawn = ActivityContinuousGrid.Geometry(size: CGSize(width: 634, height: 206), columns: 54, square: .mac,
                                                    labelGap: 4, lineGap: 8)
        #expect(drawn == geometry)
        let odd = Self.geometry(width: 500, columns: 314, band: 13.5)
        let back = ActivityContinuousGrid.Geometry(size: CGSize(width: 500, height: odd.height), columns: 314, square: .mac,
                                                   labelGap: 4, lineGap: 8)
        #expect(abs(back.band - 13.5) < 1e-9)
    }

    // MARK: Hit-testing and keys

    @Test func hitTestingAcrossALineEnd() {
        let grid = Self.grid(days: Self.sixYears)
        let geometry = Self.geometry(width: 560, columns: grid.columns)
        #expect(geometry.perLine == 46)
        // The last square of the first line, row 3, and the next week's at the next line's start.
        #expect(geometry.rect(column: 45, row: 3, rightToLeft: false) == CGRect(x: 540, y: 53, width: 10, height: 10))
        #expect(geometry.cell(at: CGPoint(x: 545, y: 58), rightToLeft: false)! == (45, 3))
        #expect(geometry.rect(column: 46, row: 3, rightToLeft: false) == CGRect(x: 0, y: 160, width: 10, height: 10))
        #expect(geometry.cell(at: CGPoint(x: 5, y: 165), rightToLeft: false)! == (46, 3))
        // A gap's half belongs to the square beside it.
        #expect(geometry.cell(at: CGPoint(x: 10.9, y: 58), rightToLeft: false)! == (0, 3))
        #expect(geometry.cell(at: CGPoint(x: 11.1, y: 58), rightToLeft: false)! == (1, 3))
        // A label band, the space between lines and past the width are no day.
        #expect(geometry.cell(at: CGPoint(x: 5, y: 110), rightToLeft: false) == nil)
        #expect(geometry.cell(at: CGPoint(x: 5, y: 102), rightToLeft: false) == nil)
        #expect(geometry.cell(at: CGPoint(x: 559, y: 58), rightToLeft: false) == nil)
        // The last line is partial: 38 weeks, then nothing.
        let last = geometry.rowsTop(6) + 5
        #expect(geometry.cell(at: CGPoint(x: 37 * 12 + 5, y: last), rightToLeft: false)! == (313, 0))
        #expect(geometry.cell(at: CGPoint(x: 38 * 12 + 5, y: last), rightToLeft: false) == nil)
        // Right to left: lines start at the right edge.
        #expect(geometry.rect(column: 0, row: 0, rightToLeft: true).minX == 550)
        #expect(geometry.cell(at: CGPoint(x: 555, y: 20), rightToLeft: true)! == (0, 0))
        #expect(geometry.cell(at: CGPoint(x: 15, y: 58), rightToLeft: true)! == (45, 3))
    }

    @Test func noDayBeforeTheFirstOrAfterToday() {
        let grid = Self.grid(days: 7)
        let geometry = Self.geometry(width: 560, columns: grid.columns)
        #expect(grid.daysAgo(at: CGPoint(x: 5, y: 20), geometry: geometry) == nil, "Monday 16, before the first day")
        #expect(grid.daysAgo(at: CGPoint(x: 5, y: 32), geometry: geometry) == 6, "Tuesday 17")
        #expect(grid.daysAgo(at: CGPoint(x: 17, y: 20), geometry: geometry) == 0, "today")
        #expect(grid.daysAgo(at: CGPoint(x: 17, y: 32), geometry: geometry) == nil, "tomorrow")
    }

    /// The keys move by the grid's days (S14): a week beside, a day above or below; a line's
    /// end continues on the next line.
    @Test func keyMovesAcrossALineEnd() throws {
        let grid = Self.grid(days: Self.sixYears)
        let geometry = Self.geometry(width: 560, columns: grid.columns)
        let day = try #require(grid.daysAgo(column: 45, row: 3))
        let nextWeek = grid.cell(daysAgo: day - 7)
        #expect(nextWeek == (46, 3) && geometry.place(ofColumn: nextWeek.column) == (1, 0))
        #expect(grid.cell(daysAgo: day + 7) == (44, 3))
        #expect(grid.cell(daysAgo: day + 1) == (45, 2) && grid.cell(daysAgo: day - 1) == (45, 4))
        // A day up from a line's first square, top row: the last square of the line before.
        let top = try #require(grid.daysAgo(column: 46, row: 0))
        #expect(grid.cell(daysAgo: top + 1) == (45, 6))
    }

    /// A selection is a day: whatever the width, its square is found again under the pointer.
    @Test func aDaySurvivesAWidthChange() {
        let grid = Self.grid(days: Self.sixYears)
        for width in [120, 400, 560, 900, 1_600] as [CGFloat] {
            let geometry = Self.geometry(width: width, columns: grid.columns)
            for day in [0, 1, 6, 7, 100, 1_000, Self.sixYears - 1] {
                for rightToLeft in [false, true] {
                    let rect = grid.rect(daysAgo: day, geometry: geometry, rightToLeft: rightToLeft)
                    #expect(grid.daysAgo(at: CGPoint(x: rect.midX, y: rect.midY), geometry: geometry,
                                         rightToLeft: rightToLeft) == day, "day \(day) at \(width)")
                }
            }
        }
    }

    // MARK: Labels

    /// 2025-12-01 (a Monday) to today: 1 January 2026 is a Thursday in column 4.
    static let fromDecember = grid(days: 358)

    static func labels(_ grid: ActivityContinuousGrid, width: CGFloat, rightToLeft: Bool = false, month: CGFloat = 15,
                       year: CGFloat = 22) -> ActivityContinuousGrid.Labels {
        grid.labels(geometry(width: width, columns: grid.columns), rightToLeft: rightToLeft,
                    monthWidth: { _ in month }, yearWidth: { _ in year })
    }

    static func summary(_ labels: ActivityContinuousGrid.Labels) -> [String] {
        labels.items.map { label in
            let name = switch label.kind {
            case let .month(month): "m\(month)"
            case let .year(year): "y\(year)"
            }
            return "\(label.line):\(name)@\(Int(label.x.rounded()))" + (label.rule.map { "|\(Int($0.rounded()))" } ?? "")
        }
    }

    @Test func aYearStartingMidLine() {
        let grid = Self.fromDecember
        #expect(grid.columns == 52 && grid.cell(daysAgo: 357) == (0, 0))
        let labels = Self.labels(grid, width: 1_600)
        #expect(labels.step == 1)
        // The grid's first column names its year (December gives way to it); 1 January's column
        // carries the rule and the year; every other 1st its month.
        #expect(Self.summary(labels) == ["0:y2025@0", "0:y2026@51|48", "0:m2@96", "0:m3@144", "0:m4@204", "0:m5@252",
                                         "0:m6@312", "0:m7@360", "0:m8@408", "0:m9@468", "0:m10@516", "0:m11@564"])
    }

    @Test func aYearAtALinesFirstColumnAndLabelsAtTheLineEnds() {
        // Four weeks a line: 1 January's column opens the second line.
        let labels = Self.summary(Self.labels(Self.fromDecember, width: 46))
        #expect(labels.first == "0:y2025@0")
        #expect(labels.contains("1:y2026@3|0"))
        // April's 1st is in its line's second column; no line label, as its line holds two March columns.
        #expect(labels.filter { $0.hasPrefix("4:") } == ["4:m4@12"])
        // A line that starts with three June columns names June, then July's 1st.
        #expect(labels.filter { $0.hasPrefix("7:") } == ["7:m6@0", "7:m7@24"])
        // A 1st in a line's last column would run past the edge: left out, and the next line names it.
        #expect(labels.filter { $0.hasPrefix("9:") } == ["9:m8@0"])
        #expect(labels.filter { $0.hasPrefix("10:") } == ["10:m9@0"])
        #expect(!labels.contains { $0.contains("m9@36") })
    }

    @Test func aYearAtTheTrailingEdgeEndsThere() {
        // Five weeks a line (58 pt): 1 January's column is the line's last; its year ends at the edge.
        let labels = Self.summary(Self.labels(Self.fromDecember, width: 58))
        #expect(labels.prefix(2) == ["0:y2025@0", "0:y2026@36|33"])
    }

    @Test func aYearAtTheGridsFirstColumn() {
        // 2025-12-29 (a Monday) to today: 1 January is in the first column.
        let grid = Self.grid(days: 330)
        #expect(grid.cell(daysAgo: 326) == (0, 3))
        let labels = Self.summary(Self.labels(grid, width: 1_600))
        #expect(labels.first == "0:y2026@3|0")
        #expect(!labels.contains { $0.contains("y2025") })
    }

    @Test func monthsThinWhenTheyDoNotFit() {
        // 50 pt names: every second month; 100 pt: every third, as the design's sketch.
        let second = Self.labels(Self.fromDecember, width: 1_600, month: 50)
        #expect(second.step == 2)
        #expect(Self.summary(second) == ["0:y2025@0", "0:y2026@51|48", "0:m3@144", "0:m5@252", "0:m7@360", "0:m9@468",
                                         "0:m11@564"])
        let third = Self.labels(Self.fromDecember, width: 1_600, month: 100)
        #expect(third.step == 3)
        #expect(Self.summary(third) == ["0:y2025@0", "0:y2026@51|48", "0:m4@204", "0:m7@360", "0:m10@516"])
        // Names wider than half a year: the years alone.
        #expect(Self.labels(Self.grid(days: Self.sixYears), width: 1_600, month: 700).step == 12)
    }

    @Test func labelsMirrorRightToLeft() {
        let labels = Self.summary(Self.labels(Self.fromDecember, width: 1_600, rightToLeft: true))
        #expect(labels.prefix(4) == ["0:y2025@1578", "0:y2026@1527|1551", "0:m2@1489", "0:m3@1441"])
    }
}
