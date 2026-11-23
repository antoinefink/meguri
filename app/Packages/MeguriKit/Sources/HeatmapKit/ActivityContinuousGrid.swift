import CoreGraphics
import Foundation

/// The Statistics page's Activity grid (`stats/statistics-page` S7.5, contract N25.8, the
/// owner's Option E of 2026-11-23): one grid for all of history, a column per week with the
/// seven weekdays down it, running unbroken from the week of the first day drawn to today's
/// week. Today's fixed squares (`ActivityGrid.Square`), opacities (`ActivityGrid.opacity`) and
/// AH7 levels (`ActivityGrid.Levels`, over every day drawn). It never scrolls sideways: it fills
/// the width with as many week columns as fit and wraps onto a new line, like text, oldest at
/// the top left (`Geometry`). Days before the first day and after today are not drawn.
/// Today's own grid (`ActivityGrid`) is a trailing window and is not changed by this type.
///
/// Days are counted back from today (`daysAgo`, 0 is today), as the core's H1 days are
/// (day `d` is `-d` days ago); dates are plain Gregorian dates counted back from today's.
public nonisolated struct ActivityContinuousGrid: Sendable, Equatable, Hashable {
    /// Days drawn: the first day through today, at least one.
    public let days: Int
    /// The first day's row, 0 at the top (the first weekday).
    public let firstRow: Int
    /// Today's row.
    public let todayRow: Int
    /// Week columns from the first day's week to today's; column 0 is the oldest.
    public let columns: Int
    /// Today's Gregorian date.
    public let year: Int
    public let month: Int
    public let day: Int
    /// Reviews per day, index = days ago.
    private let counts: [UInt32]
    /// AH7's levels over every day drawn.
    public let levels: ActivityGrid.Levels
    /// Reviews over every day drawn.
    public let total: Int
    /// The months whose days are drawn, oldest first.
    public let months: [Month]

    /// A month whose days are drawn.
    public nonisolated struct Month: Sendable, Equatable, Hashable, Identifiable {
        public var id: Int { year * 12 + month }
        public let year: Int
        /// 1 to 12.
        public let month: Int
        /// Its days drawn, in days ago: `upperBound - 1` is its oldest drawn day.
        public let daysAgo: Range<Int>
        /// Its 1st is drawn (false only for the first day's month when the grid starts after its 1st).
        public let startsHere: Bool
    }

    /// - Parameters:
    ///   - year, month, day: today's Gregorian date.
    ///   - days: days drawn, the first day through today (at least one).
    ///   - firstWeekday: the system's first day of the week, 1 Sunday to 7 Saturday.
    ///   - counts: reviews per day, index = days ago; missing days count 0.
    public init(year: Int, month: Int, day: Int, days: Int, firstWeekday: Int, counts: [UInt32]) {
        let days = max(1, days)
        self.days = days
        self.year = year
        self.month = month
        self.day = day
        let calendar = Self.calendar
        let today = calendar.date(from: DateComponents(year: year, month: month, day: day))
        let weekday = today.map { calendar.component(.weekday, from: $0) } ?? firstWeekday
        todayRow = ((weekday - firstWeekday) % 7 + 7) % 7
        firstRow = ((todayRow - (days - 1) % 7) % 7 + 7) % 7
        columns = (firstRow + days - 1) / 7 + 1
        let shown = Array(counts.prefix(days))
        self.counts = shown
        levels = ActivityGrid.Levels(counts: shown)
        total = shown.reduce(0) { $0 + Int($1) }
        months = today.map { Self.months(today: $0, days: days) } ?? []
    }

    public static func == (lhs: Self, rhs: Self) -> Bool {
        lhs.days == rhs.days && lhs.firstRow == rhs.firstRow && lhs.year == rhs.year && lhs.month == rhs.month
            && lhs.day == rhs.day && lhs.counts == rhs.counts
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(days)
        hasher.combine(firstRow)
        hasher.combine(year)
        hasher.combine(month)
        hasher.combine(day)
        hasher.combine(counts)
    }

    /// The months drawn, from the first day (`days - 1` ago) to today.
    private static func months(today: Date, days: Int) -> [Month] {
        let calendar = Self.calendar
        guard let first = calendar.date(byAdding: .day, value: -(days - 1), to: today) else { return [] }
        let start = calendar.dateComponents([.year, .month, .day], from: first)
        guard var year = start.year, var month = start.month else { return [] }
        var result: [Month] = []
        // Days from the first day: `index` is where the month's first drawn day is.
        var index = 0
        var startsHere = start.day == 1
        while index < days {
            let nextYear = month == 12 ? year + 1 : year
            let nextMonth = month == 12 ? 1 : month + 1
            let next = calendar.date(from: DateComponents(year: nextYear, month: nextMonth, day: 1))
                .flatMap { calendar.dateComponents([.day], from: first, to: $0).day } ?? days
            let end = min(max(next, index + 1), days)
            result.append(Month(year: year, month: month, daysAgo: (days - end)..<(days - index), startsHere: startsHere))
            index = end
            year = nextYear
            month = nextMonth
            startsHere = true
        }
        return result
    }

    // MARK: Days

    /// Where a day sits: its week column (0 the oldest) and its row.
    public func cell(daysAgo: Int) -> (column: Int, row: Int) {
        let place = firstRow + (days - 1 - daysAgo)
        return (place / 7, place % 7)
    }

    /// The day at `column` and `row`, in days ago; nil outside the grid, before the first day or after today.
    public func daysAgo(column: Int, row: Int) -> Int? {
        guard (0..<columns).contains(column), (0..<7).contains(row) else { return nil }
        let index = column * 7 + row - firstRow
        guard (0..<days).contains(index) else { return nil }
        return days - 1 - index
    }

    public func count(daysAgo: Int) -> UInt32 {
        counts.indices.contains(daysAgo) ? counts[daysAgo] : 0
    }

    public func level(daysAgo: Int) -> Int { levels.level(count(daysAgo: daysAgo)) }

    /// A day's Gregorian date.
    public func date(daysAgo: Int) -> (year: Int, month: Int, day: Int) {
        let calendar = Self.calendar
        guard let today = calendar.date(from: DateComponents(year: year, month: month, day: day)),
              let date = calendar.date(byAdding: .day, value: -daysAgo, to: today) else { return (year, month, day) }
        let parts = calendar.dateComponents([.year, .month, .day], from: date)
        return (parts.year ?? year, parts.month ?? month, parts.day ?? day)
    }

    /// The month holding a day.
    public func month(daysAgo: Int) -> Month? {
        months.first { $0.daysAgo.contains(daysAgo) }
    }

    /// The month of a column's top drawn day, its oldest.
    private func columnMonth(_ column: Int) -> Month? {
        (0..<7).lazy.compactMap { daysAgo(column: column, row: $0) }.first.flatMap(month(daysAgo:))
    }

    // MARK: Geometry

    /// How the grid wraps at a width: as many whole week columns as fit on a line, then the next
    /// line, like text. Each line is its label band (month and year labels), a small gap, then
    /// seven rows of squares; lines are a little further apart than the rows.
    public nonisolated struct Geometry: Sendable, Equatable {
        public let square: ActivityGrid.Square
        /// The width filled: lines start at its leading edge.
        public let width: CGFloat
        public let columns: Int
        public let perLine: Int
        public let lines: Int
        /// The label band's height: a caption line.
        public let band: CGFloat
        /// Between a line's labels and its squares.
        public let labelGap: CGFloat
        /// Between a line's squares and the next line's labels.
        public let lineGap: CGFloat

        public var pitch: CGFloat { square.side + square.gap }
        /// Seven rows of squares.
        public var rowsHeight: CGFloat { 7 * square.side + 6 * square.gap }
        public var lineHeight: CGFloat { band + labelGap + rowsHeight }
        public var height: CGFloat { CGFloat(lines) * lineHeight + CGFloat(max(0, lines - 1)) * lineGap }
        /// A full line's drawn width: its columns and the gaps between them.
        public var lineWidth: CGFloat { CGFloat(min(perLine, columns)) * pitch - square.gap }

        /// Whole columns that fit `width` (⌊(width + gap) / pitch⌋), at least one.
        public static func perLine(width: CGFloat, square: ActivityGrid.Square) -> Int {
            guard width > 0, width.isFinite, square.side > 0 else { return 1 }
            // n columns need n·pitch − gap; the small allowance absorbs layout rounding.
            return max(1, Int(((width + square.gap) / (square.side + square.gap) + 1e-6).rounded(.down)))
        }

        public init(width: CGFloat, columns: Int, square: ActivityGrid.Square, band: CGFloat, labelGap: CGFloat,
                    lineGap: CGFloat) {
            self.square = square
            self.width = width.isFinite ? max(0, width) : 0
            self.columns = max(1, columns)
            perLine = Self.perLine(width: width, square: square)
            lines = (self.columns + perLine - 1) / perLine
            self.band = max(0, band)
            self.labelGap = labelGap
            self.lineGap = lineGap
        }

        /// The geometry of a grid drawn at `size` by a layout made with the same values: the
        /// band is the height left once the lines are known, so a canvas and its overlays need
        /// no measurement of their own.
        public init(size: CGSize, columns: Int, square: ActivityGrid.Square, labelGap: CGFloat, lineGap: CGFloat) {
            let perLine = Self.perLine(width: size.width, square: square)
            let lines = (max(1, columns) + perLine - 1) / perLine
            let rows = 7 * square.side + 6 * square.gap
            let band = (size.height - CGFloat(lines) * (labelGap + rows) - CGFloat(lines - 1) * lineGap) / CGFloat(lines)
            self.init(width: size.width, columns: columns, square: square, band: band, labelGap: labelGap, lineGap: lineGap)
        }

        /// The top of a line's label band.
        public func lineTop(_ line: Int) -> CGFloat { CGFloat(line) * (lineHeight + lineGap) }

        /// The top of a line's first row of squares.
        public func rowsTop(_ line: Int) -> CGFloat { lineTop(line) + band + labelGap }

        /// A column's line and its place on the line.
        public func place(ofColumn column: Int) -> (line: Int, slot: Int) { (column / perLine, column % perLine) }

        /// The leading edge of the squares in a line's slot, mirrored under right-to-left layout
        /// (lines then start at the right edge).
        public func x(slot: Int, rightToLeft: Bool) -> CGFloat {
            rightToLeft ? width - CGFloat(slot) * pitch - square.side : CGFloat(slot) * pitch
        }

        /// A square's frame.
        public func rect(column: Int, row: Int, rightToLeft: Bool) -> CGRect {
            let place = place(ofColumn: column)
            return CGRect(x: x(slot: place.slot, rightToLeft: rightToLeft), y: rowsTop(place.line) + CGFloat(row) * pitch,
                          width: square.side, height: square.side)
        }

        /// The column and row under a point; nil in a label band, between lines or past the last
        /// column. Each square takes half the gap on each side, so no point between two squares
        /// of a line is dead.
        public func cell(at point: CGPoint, rightToLeft: Bool) -> (column: Int, row: Int)? {
            guard point.y >= 0, point.x >= 0, point.x <= width else { return nil }
            let line = Int((point.y / (lineHeight + lineGap)).rounded(.down))
            guard line < lines else { return nil }
            let inLine = point.y - rowsTop(line)
            guard inLine >= -square.gap / 2, inLine < rowsHeight + square.gap / 2 else { return nil }
            let row = min(6, max(0, Int(((inLine + square.gap / 2) / pitch).rounded(.down))))
            let x = rightToLeft ? width - point.x : point.x
            let slot = Int(((x + square.gap / 2) / pitch).rounded(.down))
            guard (0..<perLine).contains(slot) else { return nil }
            let column = line * perLine + slot
            guard column < columns else { return nil }
            return (column, row)
        }
    }

    /// The day under a point, in days ago; nil where no day is drawn.
    public func daysAgo(at point: CGPoint, geometry: Geometry, rightToLeft: Bool = false) -> Int? {
        geometry.cell(at: point, rightToLeft: rightToLeft).flatMap { daysAgo(column: $0.column, row: $0.row) }
    }

    /// A day's square.
    public func rect(daysAgo: Int, geometry: Geometry, rightToLeft: Bool = false) -> CGRect {
        let cell = cell(daysAgo: daysAgo)
        return geometry.rect(column: cell.column, row: cell.row, rightToLeft: rightToLeft)
    }

    // MARK: Labels

    /// A label in a line's band.
    public nonisolated struct Label: Sendable, Equatable {
        public enum Kind: Sendable, Equatable {
            /// A month's short name, 1 to 12.
            case month(Int)
            /// A year, bold: at the column holding 1 January (with its rule), or the grid's first column.
            case year(Int)
        }

        public let kind: Kind
        public let line: Int
        public let column: Int
        /// The text's leading edge, after mirroring.
        public let x: CGFloat
        public let width: CGFloat
        /// The year rule's left edge, after mirroring: the column's leading edge; nil for no rule.
        public let rule: CGFloat?
    }

    /// The labels and the month step they keep.
    public nonisolated struct Labels: Sendable, Equatable {
        public let items: [Label]
        /// Every `step`-th month is named (1, 2, 3, 4, 6 or 12), counted from January.
        public let step: Int
    }

    /// The month steps tried, from every month to the years alone.
    public static let monthSteps = [1, 2, 3, 4, 6, 12]

    /// AH9's labels over each line: a month's name at the column holding its 1st, leading-aligned
    /// with it; at the column holding 1 January a thin rule and the year in bold instead of the
    /// month; the grid's first column also carries its year (no rule), so the oldest line says
    /// when it starts. A later line whose first column holds no 1st is labelled with its month
    /// when at least three of its columns before the next 1st are that month, as Today's first
    /// column is. When the
    /// months do not fit, every second, third, fourth, sixth month is named, or the years alone:
    /// the smallest step at which no two names at a 1st overlap. Years are placed first, then
    /// the grid's first year, then a line's first month, then the months; a label that would
    /// overlap one already placed, or run past the trailing edge, is left out, except a year,
    /// which is never left out: past the edge its rule stays at its column and its text ends
    /// just before the rule. A line's first month follows the step too. Under right-to-left
    /// layout everything mirrors.
    /// - Parameters:
    ///   - ruleWidth: the rule's width; the year's text starts `ruleSpacing` after it.
    ///   - spacing: the least space between two labels.
    ///   - monthWidth, yearWidth: the drawn widths of a month's name and a year.
    public func labels(_ geometry: Geometry, rightToLeft: Bool = false, ruleWidth: CGFloat = 1, ruleSpacing: CGFloat = 2,
                       spacing: CGFloat = 4, monthWidth: (Int) -> CGFloat, yearWidth: (Int) -> CGFloat) -> Labels {
        struct Candidate {
            enum Rank: Int { case year, gridStart, lineStart, month }
            let kind: Label.Kind
            let column: Int
            let rank: Rank
            let rule: Bool
            let width: CGFloat
            /// The rule, its spacing and the text.
            let extent: CGFloat
        }
        func candidate(_ kind: Label.Kind, column: Int, rank: Candidate.Rank, rule: Bool) -> Candidate {
            let width: CGFloat = switch kind {
            case let .month(month): monthWidth(month)
            case let .year(year): yearWidth(year)
            }
            return Candidate(kind: kind, column: column, rank: rank, rule: rule, width: width,
                             extent: rule ? width + ruleWidth + ruleSpacing : width)
        }
        let pitch = geometry.pitch
        func left(_ column: Int) -> CGFloat { CGFloat(geometry.place(ofColumn: column).slot) * pitch }

        // The 1sts drawn: a year at 1 January, else the month.
        let firsts: [Candidate] = months.filter(\.startsHere).map { month in
            let column = cell(daysAgo: month.daysAgo.upperBound - 1).column
            return month.month == 1
                ? candidate(.year(month.year), column: column, rank: .year, rule: true)
                : candidate(.month(month.month), column: column, rank: .month, rule: false)
        }

        // The smallest step at which no two names at a 1st on a line overlap.
        func fits(_ step: Int) -> Bool {
            let kept = firsts.filter { if case let .month(month) = $0.kind { (month - 1) % step == 0 } else { true } }
            for (a, b) in zip(kept, kept.dropFirst()) where geometry.place(ofColumn: a.column).line == geometry.place(ofColumn: b.column).line {
                if left(a.column) + a.extent + spacing > left(b.column) { return false }
            }
            return true
        }
        let step = Self.monthSteps.first(where: fits) ?? 12

        var candidates = firsts.filter { if case let .month(month) = $0.kind { (month - 1) % step == 0 } else { true } }
        if let first = months.first, !(first.startsHere && first.month == 1 && cell(daysAgo: first.daysAgo.upperBound - 1).column == 0) {
            candidates.append(candidate(.year(first.year), column: 0, rank: .gridStart, rule: false))
        }
        let firstColumns = Set(firsts.map(\.column))
        for line in 1..<max(1, geometry.lines) {
            let column = line * geometry.perLine
            guard column < columns, !firstColumns.contains(column), let month = columnMonth(column) else { continue }
            let end = min(columns, column + geometry.perLine)
            // Only the columns before the next 1st count, so two names never sit side by side.
            let same = (column..<end).prefix { !firstColumns.contains($0) && columnMonth($0) == month }.count
            // Named only as the month step names it, and never when only the years fit.
            let yearsOnly = step == Self.monthSteps.last
            if same >= 3, !yearsOnly, (month.month - 1) % step == 0 { candidates.append(candidate(.month(month.month), column: column, rank: .lineStart, rule: false)) }
        }

        // Each placed label's span on its line (rule and text), its text's left edge and its rule.
        var placed: [(candidate: Candidate, line: Int, start: CGFloat, end: CGFloat, text: CGFloat, rule: CGFloat?)] = []
        for item in candidates.sorted(by: { ($0.rank.rawValue, $0.column) < ($1.rank.rawValue, $1.column) }) {
            let line = geometry.place(ofColumn: item.column).line
            let left = left(item.column)
            var start = left, end = left + item.extent
            var text = item.rule ? left + ruleWidth + ruleSpacing : left
            if end > geometry.width {
                // Only a year stays: its rule keeps 1 January's column and its text ends before it.
                guard item.rank == .year else { continue }
                text = max(0, left - ruleSpacing - item.width)
                start = text
                end = left + ruleWidth
            }
            let overlaps = placed.contains { other in
                other.line == line && start < other.end + spacing && other.start < end + spacing
            }
            if overlaps && item.rank != .year { continue }
            placed.append((item, line, start, end, text, item.rule ? left : nil))
        }
        let items = placed.sorted { ($0.line, $0.start) < ($1.line, $1.start) }.map { item -> Label in
            guard rightToLeft else {
                return Label(kind: item.candidate.kind, line: item.line, column: item.candidate.column, x: item.text,
                             width: item.candidate.width, rule: item.rule)
            }
            return Label(kind: item.candidate.kind, line: item.line, column: item.candidate.column,
                         x: geometry.width - item.text - item.candidate.width, width: item.candidate.width,
                         rule: item.rule.map { geometry.width - $0 - ruleWidth })
        }
        return Labels(items: items, step: step)
    }

    private static let calendar: Calendar = {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = .gmt
        return calendar
    }()
}
