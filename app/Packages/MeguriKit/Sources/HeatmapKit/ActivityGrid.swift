import CoreGraphics

/// The activity heatmap's grid (contract N14.11, `stats/activity-heatmap` AH7, AH8):
/// how many whole weeks fit a width, where each day sits, and its level. Pure values,
/// so a width change only re-runs this, never the query.
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

    public let weeks: Int
    /// Today's row, 0 at the top (the locale's first weekday).
    public let todayRow: Int
    /// Counts per day, index = days ago, as the core returns them.
    private let counts: [UInt32]
    /// The busiest day shown (AH7's M).
    public let maximum: UInt32
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
        maximum = shown.max() ?? 0
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

    public func level(daysAgo: Int) -> Int { Self.level(count: count(daysAgo: daysAgo), maximum: maximum) }

    /// AH7: 0 for no reviews, else ceil(4c / M), 1 to 4.
    public static func level(count: UInt32, maximum: UInt32) -> Int {
        guard count > 0, maximum > 0 else { return 0 }
        let c = UInt64(count), m = UInt64(maximum)
        return Int(min(4, (4 * c + m - 1) / m))
    }
}
