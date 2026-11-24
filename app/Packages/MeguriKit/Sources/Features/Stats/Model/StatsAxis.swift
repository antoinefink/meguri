import CoreBridge
import Foundation

/// One bar of a time axis: what it stands for and the days it sums (H1 day indexes, inclusive).
nonisolated public struct StatsAxisBin: Hashable, Sendable {
    public let period: StatsPeriod
    public let first: Int
    public let last: Int

    public init(period: StatsPeriod, first: Int, last: Int) {
        self.period = period
        self.first = first
        self.last = last
    }

    /// One day's bar: Show in Browse for a day (S8.4) needs a bar of one day.
    public var singleDay: Int? { first == last ? first : nil }
}

/// A time axis of `stats/statistics-page` S4: the bars in order, and where a day's values go.
nonisolated public struct StatsTimeAxis: Hashable, Sendable {
    public let bins: [StatsAxisBin]
    /// All's first bar also takes every day before it (S4.1: counts dated before the first month).
    public let opensBefore: Bool

    public init(bins: [StatsAxisBin], opensBefore: Bool) {
        self.bins = bins
        self.opensBefore = opensBefore
    }

    public var periods: [StatsPeriod] { bins.map(\.period) }
    public var firstDay: Int { bins.first?.first ?? 0 }
    /// The days the bars cover, from the first bar's first day to today.
    public var days: Int { max(1, 1 - firstDay) }

    /// The bar a day's values go to: a day of 1 or more counts on day 0 (S4.1's rows after
    /// today); a day before the first bar goes to it only on All's axis.
    public func index(ofDay day: Int) -> Int? {
        let day = min(day, 0)
        guard let first = bins.first, let last = bins.last else { return nil }
        if day < first.first { return opensBefore ? 0 : nil }
        if day > last.last { return nil }
        var low = 0
        var high = bins.count - 1
        while low < high {
            let middle = (low + high + 1) / 2
            if bins[middle].first <= day { low = middle } else { high = middle - 1 }
        }
        return bins[low].first <= day && day <= bins[low].last ? low : nil
    }

    /// Each bar's sum of `value` over the map's days (counts, times, retention counts).
    public func sums<Value>(_ map: StatsSparseMap<Value>, _ value: (Value) -> Double) -> [Double] {
        var out = Array(repeating: 0.0, count: bins.count)
        for (key, entry) in zip(map.keys, map.values) {
            if let index = index(ofDay: key) { out[index] += value(entry) }
        }
        return out
    }

    /// Whether any day of the map lands on a bar.
    public func covers<Value>(_ map: StatsSparseMap<Value>, where keep: (Value) -> Bool = { _ in true }) -> Bool {
        zip(map.keys, map.values).contains { index(ofDay: $0) != nil && keep($1) }
    }
}

/// S4's axes.
nonisolated enum StatsAxes {
    /// S4.1: the bars of a chart of the past for `period`. `allFirstDay` is the day All's first
    /// month holds (H9's oldest day, or S4.1's fallback).
    static func past(_ period: StatsPagePeriod, days: StatsDays, allFirstDay: Int) -> StatsTimeAxis {
        switch period {
        case .oneMonth:
            return StatsTimeAxis(bins: (-29...0).map { StatsAxisBin(period: .day(days.date(ofDay: $0)), first: $0, last: $0) },
                                 opensBefore: false)
        case .threeMonths, .sixMonths, .oneYear:
            let count = period == .threeMonths ? 13 : period == .sixMonths ? 26 : 52
            let current = days.weekStart(ofDay: 0)
            let bins = (0..<count).map { n -> StatsAxisBin in
                let start = current - 7 * (count - 1 - n)
                return StatsAxisBin(period: .week(start: days.date(ofDay: start)), first: start, last: min(start + 6, 0))
            }
            return StatsTimeAxis(bins: bins, opensBefore: false)
        case .all:
            var bins: [StatsAxisBin] = []
            var start = days.monthStart(ofDay: min(allFirstDay, 0))
            let current = days.monthStart(ofDay: 0)
            while start <= current {
                let next = days.monthStart(ofDay: start, adding: 1)
                guard next > start else { break }
                bins.append(StatsAxisBin(period: .month(days.date(ofDay: start)), first: start, last: min(next - 1, 0)))
                start = next
            }
            return StatsTimeAxis(bins: bins, opensBefore: true)
        }
    }

    /// S4.2: Reviews Due's bars ahead, Overdue first when some cards are overdue and Later last
    /// when All has cards past its 24th month. Keys are `future_due`'s days from today.
    static func ahead(_ period: StatsPagePeriod, days: StatsDays, due: StatsSparseMap<Int>) -> StatsTimeAxis {
        var bins: [StatsAxisBin] = []
        if due.keys.first.map({ $0 < 0 }) == true {
            bins.append(StatsAxisBin(period: .overdue, first: Int.min, last: -1))
        }
        switch period {
        case .oneMonth:
            bins += (0...29).map { StatsAxisBin(period: .day(days.date(ofDay: $0)), first: $0, last: $0) }
        case .threeMonths, .sixMonths, .oneYear:
            let count = period == .threeMonths ? 13 : period == .sixMonths ? 26 : 52
            let current = days.weekStart(ofDay: 0)
            bins += (0..<count).map { n in
                let start = current + 7 * n
                return StatsAxisBin(period: .week(start: days.date(ofDay: start)), first: max(start, 0), last: start + 6)
            }
        case .all:
            // From the current month to the later of the 12th and the last month with a card due,
            // never past the 24th; the rest goes to Later.
            let lastDue = due.keys.last.map { max($0, 0) } ?? 0
            var months: [(first: Int, last: Int)] = []
            var start = days.monthStart(ofDay: 0)
            for _ in 0..<24 {
                let next = days.monthStart(ofDay: start, adding: 1)
                months.append((max(start, 0), next - 1))
                start = next
            }
            let needed = months.firstIndex { $0.last >= lastDue }.map { $0 + 1 } ?? 24
            let shown = min(24, max(12, needed))
            bins += months.prefix(shown).map {
                StatsAxisBin(period: .month(days.date(ofDay: $0.first)), first: $0.first, last: $0.last)
            }
            if let lastShown = months.prefix(shown).last, lastDue > lastShown.last {
                bins.append(StatsAxisBin(period: .later, first: lastShown.last + 1, last: Int.max))
            }
        }
        return StatsTimeAxis(bins: bins, opensBefore: false)
    }
}

/// Sums ahead: `future_due`'s keys are not folded (a key of 1 is tomorrow), so Reviews Due's
/// axis looks bars up by key alone.
nonisolated extension StatsTimeAxis {
    func aheadIndex(ofKey key: Int) -> Int? {
        bins.firstIndex { $0.first <= key && key <= $0.last }
    }

    func aheadSums(_ map: StatsSparseMap<Int>) -> [Double] {
        var out = Array(repeating: 0.0, count: bins.count)
        for (key, value) in zip(map.keys, map.values) {
            if let index = aheadIndex(ofKey: key) { out[index] += Double(value) }
        }
        return out
    }
}
