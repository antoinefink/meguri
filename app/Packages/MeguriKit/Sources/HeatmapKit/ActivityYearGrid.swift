import CoreGraphics
import Foundation

/// One calendar year of the Statistics page's Activity grid (`stats/statistics-page` S7.5,
/// contract N25.8, `stats/activity-heatmap` AH1's round 11 line): Today's heatmap extended to a
/// whole year, with the same fixed squares (`ActivityGrid.Square`), opacities
/// (`ActivityGrid.opacity`) and AH7 levels (`ActivityGrid.Levels`), computed from the days this
/// block shows. Weeks are columns from 1 January, days rows from the locale's first weekday.
/// Today's own grid (`ActivityGrid`) is a trailing window and is not changed by this type.
///
/// A block always spans its whole year's columns, so the blocks stacked one under another keep
/// one width; in the current year the days after today are not drawn, as in Today's grid.
/// Pure values: a block's days are plain Gregorian dates counted from 1 January.
public nonisolated struct ActivityYearGrid: Sendable, Equatable {
    /// The Gregorian year.
    public let year: Int
    /// Days in the year: 365 or 366.
    public let days: Int
    /// Days drawn from 1 January: the whole year, or up to today in the current year.
    public let drawnDays: Int
    /// Today's day of the block (0 is 1 January), in the current year's block only.
    public let today: Int?
    /// The row of 1 January, 0 at the top (the first weekday).
    public let firstRow: Int
    /// Week columns of the whole year: 53, or 54 when a leap year starts on the last weekday.
    public let weeks: Int
    /// Reviews per day of the block, 0 is 1 January.
    private let counts: [UInt32]
    /// AH7's levels over the days drawn.
    public let levels: ActivityGrid.Levels
    /// Reviews over the days drawn.
    public let total: Int

    /// - Parameters:
    ///   - firstWeekday: the locale's first day of the week, 1 Sunday to 7 Saturday.
    ///   - today: today's day of the block in the current year (0 is 1 January); nil for a past year.
    ///   - counts: reviews per day from 1 January; missing days count 0.
    public init(year: Int, firstWeekday: Int, today: Int?, counts: [UInt32]) {
        self.year = year
        let calendar = Self.calendar
        let start = calendar.date(from: DateComponents(year: year, month: 1, day: 1))
        let next = calendar.date(from: DateComponents(year: year + 1, month: 1, day: 1))
        let days = start.flatMap { start in next.flatMap { calendar.dateComponents([.day], from: start, to: $0).day } } ?? 365
        self.days = days
        let today = today.map { min(max($0, 0), days - 1) }
        self.today = today
        drawnDays = today.map { $0 + 1 } ?? days
        let weekday = start.map { calendar.component(.weekday, from: $0) } ?? firstWeekday
        firstRow = ((weekday - firstWeekday) % 7 + 7) % 7
        weeks = (firstRow + days + 6) / 7
        self.counts = counts
        let drawn = counts.prefix(drawnDays)
        levels = ActivityGrid.Levels(counts: drawn)
        total = drawn.reduce(0) { $0 + Int($1) }
    }

    /// The day of the block (0 is 1 January) at `column` and `row`; nil outside the year or
    /// after today, which is not drawn.
    public func day(column: Int, row: Int) -> Int? {
        guard (0..<weeks).contains(column), (0..<7).contains(row) else { return nil }
        let day = column * 7 + row - firstRow
        return (0..<drawnDays).contains(day) ? day : nil
    }

    /// Where a day of the block sits.
    public func cell(ofDay day: Int) -> (column: Int, row: Int) {
        ((firstRow + day) / 7, (firstRow + day) % 7)
    }

    public func count(day: Int) -> UInt32 {
        counts.indices.contains(day) ? counts[day] : 0
    }

    public func level(day: Int) -> Int { levels.level(count(day: day)) }

    /// The block's width at a square: its columns and the gaps between them.
    public func width(_ square: ActivityGrid.Square) -> CGFloat {
        CGFloat(weeks) * (square.side + square.gap) - square.gap
    }

    /// Seven rows, as Today's grid.
    public static func height(_ square: ActivityGrid.Square) -> CGFloat { 7 * square.side + 6 * square.gap }

    /// The day of the block (0 is 1 January) under a point of the block, at a square; nil
    /// in a gap's outer half, outside the block or on a day not drawn.
    public func day(at point: CGPoint, square: ActivityGrid.Square, rightToLeft: Bool = false) -> Int? {
        let pitch = square.side + square.gap
        let x = rightToLeft ? width(square) - point.x : point.x
        guard x >= 0, point.y >= 0 else { return nil }
        // Each square takes half the gap on each side, so no point between two squares is dead.
        let column = Int(((x + square.gap / 2) / pitch).rounded(.down))
        let row = Int(((point.y + square.gap / 2) / pitch).rounded(.down))
        return day(column: column, row: row)
    }

    /// A month whose days are drawn: its number (1 to 12) and its days of the block.
    public nonisolated struct Month: Sendable, Equatable, Identifiable {
        public var id: Int { month }
        public let month: Int
        public let days: Range<Int>
    }

    /// The months whose days are drawn, in date order. The accessibility tree groups the days by
    /// month (S15).
    public var months: [Month] {
        let calendar = Self.calendar
        var result: [Month] = []
        var start = 0
        for month in 1...12 {
            guard start < drawnDays,
                  let first = calendar.date(from: DateComponents(year: year, month: month, day: 1)),
                  let length = calendar.range(of: .day, in: .month, for: first)?.count else { break }
            result.append(Month(month: month, days: start..<min(start + length, drawnDays)))
            start += length
        }
        return result
    }

    /// The month and day of the month of a day of the block.
    public func date(ofDay day: Int) -> (month: Int, day: Int) {
        let calendar = Self.calendar
        guard let start = calendar.date(from: DateComponents(year: year, month: 1, day: 1)),
              let date = calendar.date(byAdding: .day, value: day, to: start) else { return (1, 1) }
        let components = calendar.dateComponents([.month, .day], from: date)
        return (components.month ?? 1, components.day ?? 1)
    }

    /// AH9's month labels for a year block: the column holding each drawn 1st carries that
    /// month's name, leading-aligned with it (1 January's column is the first). Labels are placed
    /// from the newest back; one that would overlap a label already placed is left out, and the
    /// newest, when it would run past the block's trailing edge, ends there instead. Under
    /// right-to-left layout the columns run from the right and the labels mirror with them.
    public func monthLabels(pitch: CGFloat, width: CGFloat, rightToLeft: Bool = false,
                            labelWidth: (Int) -> CGFloat) -> [ActivityGrid.MonthLabel] {
        let firsts = months.map { (column: cell(ofDay: $0.days.lowerBound).column, month: $0.month) }
        var placed: [(column: Int, month: Int, x: CGFloat, width: CGFloat)] = []
        var limit = width
        for label in firsts.reversed() {
            let size = labelWidth(label.month)
            var x = CGFloat(label.column) * pitch
            if placed.isEmpty, x + size > width {
                x = width - size
                guard x >= 0 else { continue }
            }
            guard x + size <= limit else { continue }
            placed.append((label.column, label.month, x, size))
            limit = x
        }
        return placed.reversed().map {
            ActivityGrid.MonthLabel(column: $0.column, month: $0.month, x: rightToLeft ? width - $0.x - $0.width : $0.x)
        }
    }

    private static let calendar: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .gmt
        return calendar
    }()
}
