import CoreGraphics
import Foundation

/// The activity heatmap's grid (contract N14.11, N24.6, `stats/activity-heatmap` AH7 to AH9):
/// how many whole weeks fit a width, where each day sits, its level and the month labels.
/// Pure values, so a width change only re-runs this, never the query.
public nonisolated struct ActivityGrid: Sendable, Equatable {
    /// 52 full weeks and the current one: the 371 days the query returns.
    public static let maximumWeeks = 53

    /// N14.11's baseline square, in points. Mac Home can resize it to fill its row.
    public nonisolated struct Square: Sendable, Equatable {
        public let side: CGFloat
        /// The space between two squares, across and down.
        public let gap: CGFloat
        /// A fifth of the side, as round 10 draws it (2 pt on its 10 pt square).
        public var cornerRadius: CGFloat { side / 5 }

        public init(side: CGFloat, gap: CGFloat) { self.side = side; self.gap = gap }

        /// Round 10's Mac sketch, drawn at its real size: 53 weeks are 634 pt wide.
        public static let mac = Square(side: 10, gap: 2)
        /// Round 10's iPhone sketch in points (drawn at about 0.63 of an iPhone's width).
        public static let touch = Square(side: 16, gap: 3)
    }

    /// Whole weeks selected at the baseline size, and the grid's own size. By default
    /// squares keep that size; Mac Home opts into filling the row with square cells.
    public nonisolated struct Geometry: Sendable, Equatable {
        public let weeks: Int
        public let square: Square
        public var side: CGFloat { square.side }
        public var gap: CGFloat { square.gap }
        /// A column and the gap after it: column `n` starts at `n × pitch`.
        public var pitch: CGFloat { side + gap }
        /// `weeks` columns and the gaps between them.
        public var width: CGFloat { weeks == 0 ? 0 : CGFloat(weeks) * pitch - gap }
        /// Seven rows; before measurement the baseline height is reserved.
        public var height: CGFloat { 7 * side + 6 * gap }

        /// The most recent whole weeks that fit `available`, at most 53: ⌊(available + gap) / pitch⌋,
        /// and at least one once a width is known. With `fillingWidth`, those weeks fill
        /// the row by resizing the side alone, keeping the gap fixed even above 53 weeks'
        /// baseline width. A positive width smaller than one baseline cell shows one
        /// smaller cell; an unknown or invalid width keeps the baseline placeholder height.
        public init(width available: CGFloat, square: Square, fillingWidth: Bool = false) {
            guard available > 0, available.isFinite, square.side > 0 else {
                weeks = 0
                self.square = square
                return
            }
            // n columns need n·pitch − gap; the small allowance absorbs layout rounding.
            let fitting = ((available + square.gap) / (square.side + square.gap) + 1e-6).rounded(.down)
            weeks = Int(min(CGFloat(ActivityGrid.maximumWeeks), max(1, fitting)))
            self.square = fillingWidth
                ? Square(side: (available - CGFloat(weeks - 1) * square.gap) / CGFloat(weeks), gap: square.gap)
                : square
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

    /// N24.6: a level's opacity of the system green; nil for level 0, which keeps the
    /// system's secondary fill. The standard table serves light and dark: a graded scale
    /// whose first step is light on purpose (N24.6's exception). With Increase Contrast
    /// every step reaches 3:1 against the surface, even before the empty day's fill
    /// drawn under it: in light the steps sit between 3:1 and the green's own ratio, in
    /// dark they start higher.
    public static func opacity(level: Int, increasedContrast: Bool = false, dark: Bool = false) -> Double? {
        guard (1...4).contains(level) else { return nil }
        let steps: [Double] = switch (increasedContrast, dark) {
        case (false, _): [0.3, 0.5, 0.75, 1]
        case (true, false): [0.8, 0.87, 0.94, 1]
        case (true, true): [0.55, 0.7, 0.85, 1]
        }
        return steps[level - 1]
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
    /// when it would run past the grid's trailing edge, is pulled back to end there (older
    /// labels then give way to it); one wider than the grid is left out.
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
        for label in labelled {
            let size = labelWidth(label.month)
            var x = CGFloat(label.column) * pitch
            // Only the first label placed can run past the edge: it ends there instead.
            // Older labels are placed after it, so they give way to it.
            if placed.isEmpty, x + size > width {
                x = width - size
                guard x >= 0 else { continue }
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
