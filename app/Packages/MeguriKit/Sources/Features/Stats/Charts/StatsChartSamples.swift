import Foundation

/// SAMPLE data for the chart system's gallery and tests (plan U295): made-up
/// values in the shapes the page model (U294) will feed, never read from a
/// collection. The bins follow `stats/statistics-page` S4.1's shapes so each
/// period's axis can be looked at; the model owns the real bins.
public enum StatsSamplePeriod: String, CaseIterable, Hashable, Sendable {
    case month = "1m", threeMonths = "3m", sixMonths = "6m", year = "1y", all

    /// The span named by an empty line (S8.5); nil for All.
    public var span: StatsEmptyReason.Span? {
        switch self {
        case .month: .lastMonth
        case .threeMonths: .last3Months
        case .sixMonths: .last6Months
        case .year: .lastYear
        case .all: nil
        }
    }
}

public struct StatsChartSamples: Sendable {
    public var period: StatsSamplePeriod
    public var calendar: Calendar
    public var now: Date

    public init(period: StatsSamplePeriod, calendar: Calendar, now: Date) {
        self.period = period
        self.calendar = calendar
        self.now = now
    }

    // MARK: Bins (S4.1's shapes)

    public var bins: [StatsPeriod] {
        let today = calendar.startOfDay(for: now)
        switch period {
        case .month:
            return (0..<30).reversed().compactMap { calendar.date(byAdding: .day, value: -$0, to: today).map(StatsPeriod.day) }
        case .threeMonths, .sixMonths, .year:
            let weeks = period == .threeMonths ? 13 : period == .sixMonths ? 26 : 52
            let start = calendar.dateInterval(of: .weekOfYear, for: today)?.start ?? today
            return (0..<weeks).reversed().compactMap { calendar.date(byAdding: .weekOfYear, value: -$0, to: start).map { StatsPeriod.week(start: $0) } }
        case .all:
            // A collection studied since January of last year.
            let year = calendar.component(.year, from: today) - 1
            guard let first = calendar.date(from: DateComponents(year: year, month: 1, day: 1)),
                  let months = calendar.dateComponents([.month], from: first, to: today).month
            else { return [] }
            return (0...months).compactMap { calendar.date(byAdding: .month, value: $0, to: first).map(StatsPeriod.month) }
        }
    }

    /// How many days a bin holds, so sums scale with the period.
    private var daysPerBin: Double {
        switch period {
        case .month: 1
        case .threeMonths, .sixMonths, .year: 7
        case .all: 30
        }
    }

    // MARK: Values

    /// A smooth, repeatable wave in 0...1 (no randomness, so renders are stable).
    private func wave(_ index: Int, _ seed: Double) -> Double {
        let x = Double(index)
        return 0.5 + 0.3 * sin(x * 0.7 + seed) + 0.2 * sin(x * 1.9 + seed * 2.3)
    }

    private func sums(_ count: Int, perDay: Double, seed: Double) -> [Double?] {
        (0..<count).map { ((perDay * daysPerBin) * (0.55 + 0.6 * wave($0, seed))).rounded() }
    }

    public var knowledge: StatsChartData {
        let n = bins.count
        let known = (0..<n).map { i -> Double? in (2_900 + 612 * Double(i + 1) / Double(n) + 40 * wave(i, 1)).rounded() }
        let studied = (0..<n).map { i -> Double? in (3_330 + 620 * Double(i + 1) / Double(n)).rounded() }
        return StatsChartData(
            bins: bins,
            series: [
                StatsSeries(id: "known", name: "Known", color: .remembered, mark: .line(), values: known),
                StatsSeries(id: "studied", name: "Studied", color: .reference, mark: .line(dashed: true), values: studied),
            ],
            yScale: .fitted
        )
    }

    public func reviewsByType(browsable: Bool) -> StatsChartData {
        let n = bins.count
        return StatsChartData(
            bins: bins,
            series: [
                StatsSeries(id: "mature", name: "Mature", color: .mature, mark: .stackedBar, values: sums(n, perDay: 70, seed: 0.2)),
                StatsSeries(id: "young", name: "Young", color: .young, mark: .stackedBar, values: sums(n, perDay: 34, seed: 1.1)),
                StatsSeries(id: "relearning", name: "Relearning", color: .relearning, mark: .stackedBar, values: sums(n, perDay: 6, seed: 2.0)),
                StatsSeries(id: "learning", name: "Learning", color: .learning, mark: .stackedBar, values: sums(n, perDay: 12, seed: 2.7)),
            ],
            calloutLines: [StatsCalloutLine(id: "total", name: "Total", values: (0..<n).map { i in
                [sums(n, perDay: 70, seed: 0.2), sums(n, perDay: 34, seed: 1.1), sums(n, perDay: 6, seed: 2.0), sums(n, perDay: 12, seed: 2.7)]
                    .reduce(0) { $0 + ($1[i] ?? 0) }
            })],
            browsable: browsable && period == .month ? IndexSet(0..<n) : []
        )
    }

    public var reviewsByAnswer: StatsChartData {
        let n = bins.count
        return StatsChartData(
            bins: bins,
            series: [
                StatsSeries(id: "again", name: "Again", color: .again, mark: .stackedBar, values: sums(n, perDay: 11, seed: 0.4)),
                StatsSeries(id: "hard", name: "Hard", color: .hard, mark: .stackedBar, values: sums(n, perDay: 8, seed: 1.4)),
                StatsSeries(id: "good", name: "Good", color: .good, mark: .stackedBar, values: sums(n, perDay: 90, seed: 2.2)),
                StatsSeries(id: "easy", name: "Easy", color: .easy, mark: .stackedBar, values: sums(n, perDay: 13, seed: 3.1)),
            ]
        )
    }

    public var retention: StatsChartData {
        let n = bins.count
        let values = (0..<n).map { i -> Double? in i == n / 3 ? nil : 0.86 + 0.08 * wave(i, 0.9) }
        // Two bins with fewer than 20 counted reviews.
        let open = IndexSet([1, n - 4].filter { $0 >= 0 && $0 < n && $0 != n / 3 })
        return StatsChartData(
            bins: bins,
            series: [StatsSeries(id: "retention", name: "Retention", color: .remembered, mark: .line(points: true), values: values, openPoints: open)],
            rules: [StatsRule(id: "desired", name: "Desired retention", value: 0.9)],
            unit: .fraction,
            yScale: .fraction(lower: 0.7),
            calloutLines: [
                // Open bins have fewer than 20 counted reviews.
                StatsCalloutLine(id: "remembered", name: "Remembered", values: (0..<n).map { i in values[i].map { ($0 * (open.contains(i) ? 15 : 1_200)).rounded() } }, unit: .count),
                StatsCalloutLine(id: "forgotten", name: "Forgotten", values: (0..<n).map { i in values[i].map { ((1 - $0) * (open.contains(i) ? 15 : 1_200)).rounded() } }, unit: .count),
            ]
        )
    }

    public var forgotten: StatsChartData {
        StatsChartData(
            bins: bins,
            series: [StatsSeries(id: "forgotten", name: "Forgotten", color: .forgotten, mark: .bar, values: sums(bins.count, perDay: 11, seed: 1.7))],
            browsable: period == .month ? IndexSet(bins.indices) : []
        )
    }

    public var cardsOverTime: StatsChartData {
        let n = bins.count
        func level(_ start: Double, _ end: Double, _ seed: Double) -> [Double?] {
            (0..<n).map { i in (start + (end - start) * Double(i + 1) / Double(n) + 15 * wave(i, seed)).rounded() }
        }
        return StatsChartData(
            bins: bins,
            series: [
                StatsSeries(id: "mature", name: "Mature", color: .mature, mark: .stackedArea, values: level(1_400, 2_600, 0.1)),
                StatsSeries(id: "young", name: "Young", color: .young, mark: .stackedArea, values: level(700, 900, 0.8)),
                StatsSeries(id: "relearning", name: "Relearning", color: .relearning, mark: .stackedArea, values: level(40, 60, 1.6)),
                StatsSeries(id: "learning", name: "Learning", color: .learning, mark: .stackedArea, values: level(90, 120, 2.4)),
                StatsSeries(id: "new", name: "New", color: .new, mark: .stackedArea, values: level(900, 520, 3.0)),
            ]
        )
    }

    public func addedAndStarted(sparse: Bool = false) -> StatsChartData {
        let n = bins.count
        var added = sums(n, perDay: 9, seed: 0.6)
        var started = sums(n, perDay: 7, seed: 1.3)
        if sparse {
            // A three-day collection: two bins hold data.
            added = (0..<n).map { $0 >= n - 2 ? [140.0, 60][$0 - (n - 2)] : 0 }
            started = (0..<n).map { $0 >= n - 2 ? [35.0, 42][$0 - (n - 2)] : nil }
        }
        return StatsChartData(
            bins: bins,
            series: [
                StatsSeries(id: "added", name: "Added", color: .new, mark: .bar, values: added),
                StatsSeries(id: "started", name: "Started", color: .learning, mark: .line(), values: started),
            ],
            browsable: IndexSet(bins.indices)
        )
    }

    /// Reviews Due ahead (S4.2's shapes, here for 1M and All only): Overdue, then
    /// the days or months from today, then Later for All.
    public var reviewsDue: StatsChartData {
        let today = calendar.startOfDay(for: now)
        var ahead: [StatsPeriod] = [.overdue]
        if period == .all {
            ahead += (0..<13).compactMap { calendar.date(byAdding: .month, value: $0, to: today).map(StatsPeriod.month) }
            ahead.append(.later)
        } else {
            ahead += (0..<30).compactMap { calendar.date(byAdding: .day, value: $0, to: today).map(StatsPeriod.day) }
        }
        let n = ahead.count
        let scale = period == .all ? 25.0 : 1
        let mature = (0..<n).map { i -> Double? in i == 0 ? nil : ((60 + 50 * wave(i, 0.3)) * scale).rounded() }
        let young = (0..<n).map { i -> Double? in i == 0 ? nil : ((30 + 25 * wave(i, 1.9)) * scale * (i < 8 ? 1.6 : 0.8)).rounded() }
        let overdue = (0..<n).map { i -> Double? in i == 0 ? 42 : nil }
        var running = 0.0
        let total = (0..<n).map { i -> Double? in
            running += (overdue[i] ?? 0) + (mature[i] ?? 0) + (young[i] ?? 0)
            return running
        }
        return StatsChartData(
            bins: ahead,
            series: [
                StatsSeries(id: "overdue", name: "Overdue", color: .buried, mark: .stackedBar, values: overdue),
                StatsSeries(id: "mature", name: "Mature", color: .mature, mark: .stackedBar, values: mature),
                StatsSeries(id: "young", name: "Young", color: .young, mark: .stackedBar, values: young),
            ],
            calloutLines: [StatsCalloutLine(id: "totalByThen", name: "Total by then", values: total)],
            browsable: IndexSet(0..<n)
        )
    }

    /// Stability's doubling bins (S7.13), drawn in indigo.
    public static var stability: StatsChartData {
        let labels = ["< 1 d", "1–3 d", "3–7 d", "1–2 wk", "2–4 wk", "1–3 mo", "3–6 mo", "6–12 mo", "1–2 yr", "> 2 yr"]
        let values: [Double] = [180, 240, 310, 420, 560, 740, 690, 520, 260, 90]
        return StatsChartData(
            bins: labels.map(StatsPeriod.bucket),
            series: [StatsSeries(id: "cards", name: "Cards", color: .measure, mark: .bar, values: values)],
            browsable: IndexSet(labels.indices)
        )
    }
}
