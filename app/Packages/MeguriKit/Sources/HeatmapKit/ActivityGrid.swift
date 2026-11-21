import CoreGraphics
import Foundation

/// The activity heatmap's grid (contract N14.11, N24.6, `stats/activity-heatmap` AH7 to AH9):
/// how many whole weeks fit a width, where each day sits, its level and the month labels.
/// Pure values, so a width change only re-runs this, never the query.
public nonisolated struct ActivityGrid: Sendable, Equatable {
    /// 52 full weeks and the current one: the 371 days the query returns.
    public static let maximumWeeks = 53
    /// The gap between squares as a fraction of a square's side.
    public static let gapRatio: CGFloat = 0.2

    /// Square size and spacing for a width: as many whole weeks as fit at the
    /// minimum column pitch, then stretched so the columns span the width exactly.
    public nonisolated struct Geometry: Sendable, Equatable {
        public let weeks: Int
        public let side: CGFloat
        public var gap: CGFloat { side * ActivityGrid.gapRatio }
        /// A column and the gap after it: column `n` starts at `n × pitch`.
        public var pitch: CGFloat { side + gap }
        public var height: CGFloat { weeks == 0 ? 0 : 7 * side + 6 * gap }

        public init(width: CGFloat, minimumPitch: CGFloat) {
            guard width > 0, minimumPitch > 0 else { weeks = 0; side = 0; return }
            // n columns need n·s + (n−1)·r·s of width, with s ≥ pitch / (1 + r).
            let minimumSide = minimumPitch / (1 + ActivityGrid.gapRatio)
            let fitting = Int(((width + ActivityGrid.gapRatio * minimumSide) / minimumPitch).rounded(.down))
            weeks = min(ActivityGrid.maximumWeeks, max(1, fitting))
            side = width / (CGFloat(weeks) * (1 + ActivityGrid.gapRatio) - ActivityGrid.gapRatio)
        }
    }

    /// AH7's thresholds over the days shown: each step holds about a quarter of the days studied.
    public nonisolated struct Levels: Sendable, Equatable {
        public let t1: UInt32
        public let t2: UInt32
        public let t3: UInt32
        /// The busiest day shown (AH7's M); 0 when no day shown has a review.
        public let maximum: UInt32

        public init(counts: some Sequence<UInt32>) {
            let studied = counts.filter { $0 > 0 }.sorted()
            guard let last = studied.last else { t1 = 0; t2 = 0; t3 = 0; maximum = 0; return }
            let n = studied.count
            // Integer arithmetic: multiply, then divide.
            t1 = studied[(n - 1) / 4]
            t2 = studied[(2 * (n - 1)) / 4]
            t3 = studied[(3 * (n - 1)) / 4]
            maximum = last
        }

        /// 0 for no reviews and 4 for the busiest day; otherwise the quarter the count falls in.
        public func level(_ count: UInt32) -> Int {
            guard count > 0, maximum > 0 else { return 0 }
            if count >= maximum { return 4 }
            if count <= t1 { return 1 }
            if count <= t2 { return 2 }
            if count <= t3 { return 3 }
            return 4
        }
    }

    /// N24.6: a level's opacity of the accent colour; nil for level 0, which keeps the
    /// system's secondary fill. With Increase Contrast the light steps sit further apart.
    public static func opacity(level: Int, increasedContrast: Bool) -> Double? {
        switch level {
        case 1: increasedContrast ? 0.4 : 0.35
        case 2: increasedContrast ? 0.6 : 0.55
        case 3: 0.8
        case 4: 1
        default: nil
        }
    }

    /// AH9: a month's name leading-aligned with its column.
    public nonisolated struct MonthLabel: Sendable, Equatable {
        public let column: Int
        /// The calendar's month number, 1 to 12.
        public let month: Int
        /// Where the label's left edge is drawn: its column's leading edge, mirrored under
        /// right-to-left layout, or pulled back so the newest label ends at the grid's edge.
        public let x: CGFloat
        public init(column: Int, month: Int, x: CGFloat) { self.column = column; self.month = month; self.x = x }
    }

    public let weeks: Int
    /// Today's row, 0 at the top (the locale's first weekday).
    public let todayRow: Int
    /// Counts per day, index = days ago, as the core returns them.
    private let counts: [UInt32]
    /// AH7's levels for the days shown.
    public let levels: Levels
    /// The busiest day shown (AH7's M).
    public var maximum: UInt32 { levels.maximum }
    /// Reviews over the days shown (AH8).
    public let total: Int

    /// - Parameters:
    ///   - todayWeekday: 1 Sunday to 7 Saturday, as `Calendar` numbers weekdays.
    ///   - firstWeekday: the locale's first day of the week, same numbering.
    public init(counts: [UInt32], weeks: Int, todayWeekday: Int, firstWeekday: Int) {
        self.weeks = max(0, min(ActivityGrid.maximumWeeks, weeks))
        todayRow = ((todayWeekday - firstWeekday) % 7 + 7) % 7
        self.counts = counts
        let shown = counts.prefix(max(0, (self.weeks - 1) * 7 + todayRow + 1))
        levels = Levels(counts: shown)
        total = shown.reduce(0) { $0 + Int($1) }
    }

    /// Days before today of the square at `column` (0 the oldest week) and `row`;
    /// nil for a day after today in the current week, which is not drawn.
    public func daysAgo(column: Int, row: Int) -> Int? {
        let days = (weeks - 1 - column) * 7 + todayRow - row
        return days >= 0 ? days : nil
    }

    public func count(daysAgo: Int) -> UInt32 {
        counts.indices.contains(daysAgo) ? counts[daysAgo] : 0
    }

    public func level(daysAgo: Int) -> Int { levels.level(count(daysAgo: daysAgo)) }

    /// AH9: the columns that carry a month's name, given today's date (AH6's proleptic
    /// Gregorian date). A column is labelled when one of its drawn days is a 1st, with that
    /// 1st's month; the first column is also labelled with its own month when at least
    /// three columns of that month are shown. Labels are placed from the newest column
    /// back; one that would overlap a label already placed is left out. The newest label,
    /// when it would run past the grid's trailing edge, is pulled back to end there if it
    /// still clears the label before it, and is left out otherwise.
    /// - Parameters:
    ///   - pitch: where each column starts (`Geometry.pitch`).
    ///   - width: the grid's width.
    ///   - rightToLeft: the layout's direction; columns then run from the right, the newest leftmost.
    ///   - labelWidth: the drawn width of a month's label.
    public func monthLabels(year: Int, month: Int, day: Int, pitch: CGFloat, width: CGFloat, rightToLeft: Bool = false,
                            labelWidth: (Int) -> CGFloat) -> [MonthLabel] {
        guard weeks > 0 else { return [] }
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .gmt
        guard let today = calendar.date(from: DateComponents(year: year, month: month, day: day)) else { return [] }
        func date(_ daysAgo: Int) -> DateComponents? {
            calendar.date(byAdding: .day, value: -daysAgo, to: today).map { calendar.dateComponents([.month, .day], from: $0) }
        }
        /// A column's month is the month of its top drawn day, the oldest day of that week.
        func columnMonth(_ column: Int) -> Int? { daysAgo(column: column, row: 0).flatMap(date)?.month }

        var candidates: [Int?] = (0..<weeks).map { column in
            (0..<7).compactMap { row in daysAgo(column: column, row: row).flatMap(date) }
                .first { $0.day == 1 }?.month
        }
        if candidates[0] == nil, let first = columnMonth(0) {
            let columns = (0..<weeks).prefix { columnMonth($0) == first }.count
            if columns >= 3 { candidates[0] = first }
        }

        // Leading-edge positions first, as if left to right; mirrored at the end.
        let labelled = (0..<weeks).reversed().compactMap { column in candidates[column].map { (column: column, month: $0) } }
        var placed: [(column: Int, month: Int, x: CGFloat, width: CGFloat)] = []
        var limit = width
        for (index, label) in labelled.enumerated() {
            let size = labelWidth(label.month)
            var x = CGFloat(label.column) * pitch
            if placed.isEmpty, x + size > width {
                x = width - size
                let previous = labelled.dropFirst(index + 1).first
                let clears = previous.map { CGFloat($0.column) * pitch + labelWidth($0.month) <= x } ?? true
                guard x >= 0, clears else { continue }
            }
            guard x + size <= limit else { continue }
            placed.append((label.column, label.month, x, size))
            limit = x
        }
        return placed.reversed().map {
            MonthLabel(column: $0.column, month: $0.month, x: rightToLeft ? width - $0.x - $0.width : $0.x)
        }
    }
}
