import Foundation

// The chart system's input (plan U295). The page model (U294) and, through it,
// the core's parts (U103, U292) fill these values; the chart system only draws
// them. Bins arrive already made (S4 is the model's): the chart names them,
// labels the axis and stacks the series.

/// What one bin of a chart stands for: the x category, named in the callout,
/// on the axis and in the accessibility elements (S8.3, S8.7, P18.5).
public enum StatsPeriod: Hashable, Sendable {
    /// One day, by the date of the day (`stats/history-reads` H1).
    case day(Date)
    /// Seven days from `start`, the system's first weekday (S4.1).
    case week(start: Date)
    /// A calendar month, by any date inside it.
    case month(Date)
    /// An hour of the day, 0 to 23 (Time of Day).
    case hour(Int)
    /// A value bin that its section names, such as "1–3 days" or "40%–50%".
    case bucket(String)
    /// Reviews Due's bar of overdue cards (S4.2).
    case overdue
    /// Reviews Due's last bar, past the 24th month (S4.2).
    case later
}

/// How a series is drawn.
public enum StatsMark: Hashable, Sendable {
    /// A segment of a bar stacked from the baseline, in series order.
    case stackedBar
    /// A bar from zero, not stacked (a single measure).
    case bar
    /// A line; `dashed` for a reference such as Studied, `points` to mark every
    /// value. A line of fewer than three values always shows its points (S8.5).
    case line(dashed: Bool = false, points: Bool = false)
    /// A band of an area stacked from the baseline, with a thin separating line
    /// above the band below it (Cards Over Time, S7.6).
    case stackedArea
}

/// The unit of a chart's values: how the axis, the callout and Copy Values write them.
public enum StatsValueUnit: Hashable, Sendable {
    /// A count of cards or reviews.
    case count
    /// A duration in seconds.
    case duration
    /// A fraction from 0 to 1, written as a percentage.
    case fraction
}

/// Where the y axis starts.
public enum StatsYScale: Hashable, Sendable {
    /// From zero (bars, areas, counts).
    case zero
    /// Near the lowest value, so a slow change is visible; nothing is filled
    /// (Knowledge Over Time, S7.1).
    case fitted
    /// A fraction axis from `lower` to 100%, extended down when a value is
    /// lower (Retention, S7.2: 70% to 100%).
    case fraction(lower: Double)
}

/// One series of a chart, aligned with its bins.
public struct StatsSeries: Identifiable, Hashable, Sendable {
    public var id: String
    /// The localized name the legend, the callout and VoiceOver read.
    public var name: String
    public var color: StatsSeriesColor
    public var mark: StatsMark
    /// One value per bin, in bin order; nil where the bin has no value, which
    /// breaks a line (S7.2) and counts as zero in a stack.
    public var values: [Double?]
    /// Bins whose point is drawn open and left out of the line: a rate from too
    /// few reviews to trust (S7.2's fewer than 20).
    public var openPoints: IndexSet

    public init(
        id: String, name: String, color: StatsSeriesColor, mark: StatsMark,
        values: [Double?], openPoints: IndexSet = []
    ) {
        self.id = id
        self.name = name
        self.color = color
        self.mark = mark
        self.values = values
        self.openPoints = openPoints
    }

    var isStacked: Bool {
        switch mark {
        case .stackedBar, .stackedArea: true
        case .bar, .line: false
        }
    }

    var isLine: Bool {
        if case .line = mark { return true }
        return false
    }
}

/// A dashed horizontal rule across the plot, named in the legend (the desired
/// retention, S7.2 and S7.12).
public struct StatsRule: Identifiable, Hashable, Sendable {
    public var id: String
    public var name: String
    public var value: Double
    public var color: StatsSeriesColor

    public init(id: String, name: String, value: Double, color: StatsSeriesColor = .rule) {
        self.id = id
        self.name = name
        self.value = value
        self.color = color
    }
}

/// A line of the callout that is not a drawn series, such as Reviews Due's
/// "Total by then" (S7.9) or a stack's total (S7.4): the running total lives
/// here, never on a second axis (S8.2).
public struct StatsCalloutLine: Identifiable, Hashable, Sendable {
    public var id: String
    public var name: String
    public var values: [Double?]
    /// The line's own unit when it differs from the chart's (Retention's
    /// remembered and forgotten counts on a percentage chart); nil for the chart's.
    public var unit: StatsValueUnit?
    /// The line's values already written, per bin, where a value is more than one number
    /// (Knowledge's "Likely forgotten 547 (6.5%)", S7.1); nil writes `values` in the unit.
    public var texts: [String?]?

    public init(id: String, name: String, values: [Double?], unit: StatsValueUnit? = nil, texts: [String?]? = nil) {
        self.id = id
        self.name = name
        self.values = values
        self.unit = unit
        self.texts = texts
    }

    /// The line's text at `bin`, or nil where it has no value.
    func text(at bin: Int, format: StatsChartFormat, unit chartUnit: StatsValueUnit) -> String? {
        guard values.indices.contains(bin), let value = values[bin] else { return nil }
        if let texts, texts.indices.contains(bin), let text = texts[bin] { return text }
        return format.value(value, unit: unit ?? chartUnit)
    }
}

/// Everything one plot draws. A chart with no bins draws nothing but its legend.
public struct StatsChartData: Hashable, Sendable {
    public var bins: [StatsPeriod]
    /// In stack order from the baseline; the legend follows this order, then the rules.
    public var series: [StatsSeries]
    public var rules: [StatsRule]
    public var unit: StatsValueUnit
    public var yScale: StatsYScale
    public var calloutLines: [StatsCalloutLine]
    /// Bins that offer Show in Browse (S8.4); the box's caller runs it.
    public var browsable: IndexSet
    /// The Audio Graph's name for the y axis (P18.3: cards, reviews, minutes,
    /// percent); nil names it by the unit.
    public var unitName: String?

    public init(
        bins: [StatsPeriod], series: [StatsSeries], rules: [StatsRule] = [],
        unit: StatsValueUnit = .count, yScale: StatsYScale = .zero,
        calloutLines: [StatsCalloutLine] = [], browsable: IndexSet = [], unitName: String? = nil
    ) {
        self.bins = bins
        self.series = series
        self.rules = rules
        self.unit = unit
        self.yScale = yScale
        self.calloutLines = calloutLines
        self.browsable = browsable
        self.unitName = unitName
    }

    /// The series' definitions with no values: what a box draws while its data
    /// is loading, empty or failed, so its legend and size are already final (S10.3).
    public func skeleton() -> StatsChartData {
        var copy = self
        copy.bins = []
        copy.series = series.map { var s = $0; s.values = []; s.openPoints = []; return s }
        copy.calloutLines = []
        copy.browsable = []
        return copy
    }

    /// The value of `series` at `bin`, or nil.
    func value(_ series: StatsSeries, at bin: Int) -> Double? {
        series.values.indices.contains(bin) ? series.values[bin] : nil
    }

    /// How many bins hold a value above zero in any series.
    public var binsWithData: Int {
        bins.indices.count(where: { bin in series.contains { (value($0, at: bin) ?? 0) > 0 } })
    }

    /// Fewer than three bins with data: bars keep a fixed width and lines show
    /// their points (S8.5, S12.4).
    public var isSparse: Bool { binsWithData < 3 }

    /// Whether every series keeps to one colour family (S8.1).
    public var keepsOneFamily: Bool {
        StatsSeriesColor.keepsOneFamily(series.map(\.color) + rules.map(\.color))
    }

    /// The names in legend order: the series in stack order, then the rules.
    public var legendNames: [String] { series.map(\.name) + rules.map(\.name) }
}

// MARK: - Stacking and the y domain

/// One drawn piece of a stacked series: from `lower` to `upper` in data units.
struct StatsSegment: Hashable, Sendable {
    var series: Int
    var bin: Int
    var lower: Double
    var upper: Double
}

extension StatsChartData {
    /// The stacked series' segments from the baseline, in series order. A nil
    /// value stacks as zero and draws nothing.
    func stackedSegments() -> [StatsSegment] {
        var segments: [StatsSegment] = []
        for bin in bins.indices {
            var base = 0.0
            for (index, series) in series.enumerated() where series.isStacked {
                let value = max(0, value(series, at: bin) ?? 0)
                segments.append(StatsSegment(series: index, bin: bin, lower: base, upper: base + value))
                base += value
            }
        }
        return segments
    }

    /// The total of the stacked series at `bin`.
    func stackTotal(at bin: Int) -> Double {
        series.filter(\.isStacked).reduce(0) { $0 + max(0, value($1, at: bin) ?? 0) }
    }

    /// The y domain the plot draws, and the one its gaps are measured in.
    var yDomain: ClosedRange<Double> {
        let stacks = series.contains(where: \.isStacked) ? bins.indices.map { stackTotal(at: $0) } : []
        let singles = series.filter { !$0.isStacked }.flatMap { $0.values.compactMap { $0 } }
        let ruleValues = rules.map(\.value)
        let all = stacks + singles + ruleValues
        switch yScale {
        case .zero:
            return 0...StatsChartData.niceCeiling(all.max() ?? 0)
        case .fitted:
            let values = singles + stacks.filter { $0 > 0 } + ruleValues
            guard let low = values.min(), let high = values.max() else { return 0...1 }
            let pad = max((high - low) * 0.15, max(high * 0.02, 1))
            let lower = max(0, StatsChartData.niceFloor(low - pad))
            let upper = StatsChartData.fineCeiling(high + pad * 0.5)
            return lower...max(upper, lower + 1)
        case let .fraction(lower):
            let lowest = all.min() ?? lower
            let floor = min(lower, (lowest * 20).rounded(.down) / 20)
            return max(0, floor)...1
        }
    }

    /// The smallest "nice" number (1, 2, 2.5 or 5 times a power of ten) at or
    /// above `value`, so the axis ends on a labelled value.
    static func niceCeiling(_ value: Double) -> Double {
        guard value > 0, value.isFinite else { return 1 }
        let magnitude = pow(10, (log10(value)).rounded(.down))
        for step in [1.0, 2, 2.5, 5, 10] where step * magnitude >= value {
            return step * magnitude
        }
        return 10 * magnitude
    }

    /// The smallest multiple of a tenth of `value`'s magnitude at or above it,
    /// so a fitted axis ends just above its data.
    static func fineCeiling(_ value: Double) -> Double {
        guard value > 0, value.isFinite else { return 1 }
        let step = pow(10, (log10(value)).rounded(.down)) / 10
        return (value / step).rounded(.up) * step
    }

    /// The largest nice number at or below `value`, in steps of a tenth of its magnitude.
    static func niceFloor(_ value: Double) -> Double {
        guard value > 0, value.isFinite else { return 0 }
        let magnitude = pow(10, (log10(value)).rounded(.down))
        let step = magnitude / 10
        return (value / step).rounded(.down) * step
    }
}
