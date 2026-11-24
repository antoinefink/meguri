import Foundation

/// How the chart system writes numbers, dates and bin names (S8.7): the system's
/// formatters in the app's locale. Pure values, so tests pin a locale, a calendar
/// and today.
nonisolated public struct StatsChartFormat: Sendable {
    public var locale: Locale
    public var calendar: Calendar
    /// Today, for "17 Nov" against "17 Nov 2025" (S8.7: the year outside the current one).
    public var now: Date

    public init(locale: Locale = .autoupdatingCurrent, calendar: Calendar = .autoupdatingCurrent, now: Date) {
        self.locale = locale
        var calendar = calendar
        calendar.locale = locale
        self.calendar = calendar
        self.now = now
    }

    // MARK: Numbers

    /// A grouped count ("12,345").
    public func count(_ value: Double) -> String {
        value.rounded().formatted(.number.precision(.fractionLength(0)).locale(locale))
    }

    /// A short duration ("19 min", "1 hr, 12 min"), seconds rounded to minutes
    /// once there is a minute.
    public func duration(seconds: Double) -> String {
        let seconds = max(0, seconds.rounded())
        let formatter = Self.durationFormatter(seconds < 60 ? [.second] : [.hour, .minute], calendar: calendar)
        return formatter.string(from: seconds < 60 ? seconds : (seconds / 60).rounded() * 60) ?? ""
    }

    // Formatters cost more to make than to use: the plot writes a label per
    // bin on every redraw, so each kind is made once per locale.
    // The page model writes its boxes off the main actor too, so the caches are shared under a
    // lock; a made formatter is only read afterwards, which Foundation's formatters allow from
    // any thread.
    private nonisolated(unsafe) static var durationFormatters: [String: DateComponentsFormatter] = [:]
    private nonisolated(unsafe) static var dateFormatters: [String: DateFormatter] = [:]
    private static let formattersLock = NSLock()

    private static func durationFormatter(_ units: NSCalendar.Unit, calendar: Calendar) -> DateComponentsFormatter {
        let key = "\(calendar.locale?.identifier ?? "")|\(units.rawValue)"
        formattersLock.lock()
        defer { formattersLock.unlock() }
        if let formatter = durationFormatters[key] { return formatter }
        let formatter = DateComponentsFormatter()
        formatter.unitsStyle = .short
        formatter.allowedUnits = units
        formatter.calendar = calendar
        durationFormatters[key] = formatter
        return formatter
    }

    /// A percentage with one decimal ("91.4%") from a fraction.
    public func percent(_ fraction: Double) -> String {
        fraction.formatted(.percent.precision(.fractionLength(1)).locale(locale))
    }

    /// A value in its unit, as the callout and Copy Values write it.
    public func value(_ value: Double, unit: StatsValueUnit) -> String {
        switch unit {
        case .count: count(value)
        case .duration: duration(seconds: value)
        case .fraction: percent(value)
        }
    }

    /// An axis value: compact counts ("12K"), whole percentages, minutes or hours.
    public func axisValue(_ value: Double, unit: StatsValueUnit) -> String {
        switch unit {
        case .count:
            value.formatted(.number.notation(.compactName).locale(locale))
        case .fraction:
            value.formatted(.percent.precision(.fractionLength(0)).locale(locale))
        case .duration:
            {
                let units: Set<Duration.UnitsFormatStyle.Unit> = value >= 3600 ? [.hours] : value >= 60 ? [.minutes] : [.seconds]
                return Duration.seconds(value.rounded()).formatted(.units(allowed: units, width: .narrow).locale(locale))
            }()
        }
    }

    // MARK: Dates

    private func isThisYear(_ date: Date) -> Bool {
        calendar.isDate(date, equalTo: now, toGranularity: .year)
    }

    /// A date written from a template of date fields ("dMMM": "17 Nov"), in
    /// the locale's own order.
    private func formatted(_ date: Date, template: String) -> String {
        let key = "\(locale.identifier)|\(calendar.identifier)|\(calendar.timeZone.identifier)|\(template)"
        let formatter: DateFormatter
        Self.formattersLock.lock()
        if let cached = Self.dateFormatters[key] {
            formatter = cached
        } else {
            formatter = DateFormatter()
            formatter.locale = locale
            formatter.calendar = calendar
            formatter.timeZone = calendar.timeZone
            formatter.setLocalizedDateFormatFromTemplate(template)
            Self.dateFormatters[key] = formatter
        }
        Self.formattersLock.unlock()
        return formatter.string(from: date)
    }

    /// A short date: "17 Nov", or "17 Nov 2025" outside the current year.
    public func shortDate(_ date: Date) -> String {
        formatted(date, template: isThisYear(date) ? "dMMM" : "dMMMy")
    }

    private func monthYear(_ date: Date) -> String {
        date.formatted(Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone).month(.wide).year())
    }

    private func hour(_ hour: Int) -> String {
        // In a zone with no clock changes, so every hour exists and has its own name.
        var utc = calendar
        utc.timeZone = TimeZone(identifier: "UTC") ?? calendar.timeZone
        let reference = utc.date(from: DateComponents(year: 2001, month: 1, day: 15, hour: hour)) ?? now
        return reference.formatted(Date.FormatStyle(locale: locale, calendar: utc, timeZone: utc.timeZone).hour())
    }

    // MARK: Bin names

    /// The period's full name: the callout's title, the accessibility element's
    /// first words and the audio graph's category (S8.3, P18.5).
    public func name(_ period: StatsPeriod) -> String {
        switch period {
        case let .day(date): shortDate(date)
        case let .week(start): String(localized: "stats.chart.weekOf \(shortDate(start))", bundle: .module)
        case let .month(date): monthYear(date)
        case let .hour(value): hour(value)
        case let .bucket(label): label
        case .overdue: String(localized: "stats.chart.overdue", bundle: .module)
        case .later: String(localized: "stats.chart.later", bundle: .module)
        }
    }

    /// The period's axis label: a short date for days and weeks, the month
    /// ("Mar") or the year at January ("2025") for months, the hour, the bin's name.
    /// On an axis over more than one year every month carries its whole year
    /// ("Oct 2028"): a two-digit year after a month reads as a day ("Oct 28").
    public func axisLabel(_ period: StatsPeriod, monthsShowYear: Bool, spansYears: Bool = false) -> String {
        switch period {
        case let .day(date), let .week(date):
            return formatted(date, template: "dMMM")
        case let .month(date):
            let style = Date.FormatStyle(locale: locale, calendar: calendar, timeZone: calendar.timeZone)
            if monthsShowYear || (!spansYears && calendar.component(.month, from: date) == 1) {
                return date.formatted(style.year())
            }
            return formatted(date, template: spansYears ? "MMMy" : "MMM")
        case let .hour(value):
            return hour(value)
        case .bucket, .overdue, .later:
            return name(period)
        }
    }

    /// The bins that carry an axis label, at most about `maximum` of them, counted back from
    /// the last bin so the current period is always labelled; on an axis with Overdue or Later
    /// (Reviews Due, S4.2, which looks ahead) counted on from the first bin, the current one there.
    /// Overdue and Later are always labelled, and take a label's room: no date stands right
    /// beside them, so in a small box a date never touches them. Hours keep even hours when not
    /// all fit (S7.11); a long run of months labels only Januaries.
    public func labelledBins(_ bins: [StatsPeriod], maximum: Int) -> [Int: String] {
        guard !bins.isEmpty, maximum > 0 else { return [:] }
        var labels: [Int: String] = [:]
        let specials = bins.indices.filter {
            switch bins[$0] {
            case .overdue, .later: true
            default: false
            }
        }
        let regular = bins.indices.filter { !specials.contains($0) }
        for index in specials {
            labels[index] = axisLabel(bins[index], monthsShowYear: false)
        }
        // The room the special labels leave the others.
        let room = max(1, maximum - specials.count)
        let months = regular.allSatisfy { if case .month = bins[$0] { true } else { false } }
        let januaries = regular.filter {
            if case let .month(date) = bins[$0] { calendar.component(.month, from: date) == 1 } else { false }
        }
        if months, regular.count > maximum, januaries.count >= 3 {
            // Many years of months: the years, at each January.
            let step = max(1, Int((Double(januaries.count) / Double(room)).rounded(.up)))
            for (n, index) in januaries.reversed().enumerated() where n % step == 0 {
                labels[index] = axisLabel(bins[index], monthsShowYear: true)
            }
            return clearingSpecials(labels, specials: specials, fallback: 12 * step)
        }
        let hours = regular.allSatisfy { if case .hour = bins[$0] { true } else { false } }
        if hours, regular.count > maximum {
            let step = max(2, Int((Double(regular.count) / Double(maximum)).rounded(.up)))
            for index in regular {
                if case let .hour(value) = bins[index], value % step == 0 {
                    labels[index] = axisLabel(bins[index], monthsShowYear: false)
                }
            }
            return labels
        }
        let spansYears: Bool = {
            let years = Set(regular.compactMap { index -> Int? in
                if case let .month(date) = bins[index] { return calendar.component(.year, from: date) }
                return nil
            })
            return years.count > 1
        }()
        // A label with its year is about twice as wide: fewer of them, so they fit a lead box
        // at compact width.
        let fitting = spansYears ? min(room, max(3, maximum * 3 / 5)) : room
        let step = max(1, Int((Double(regular.count) / Double(max(1, fitting))).rounded(.up)))
        // Overdue and Later belong to Reviews Due, which looks ahead: its current period is the first.
        let order = specials.isEmpty ? regular.reversed() : regular
        for (n, index) in order.enumerated() where n % step == 0 {
            labels[index] = axisLabel(bins[index], monthsShowYear: false, spansYears: spansYears)
        }
        return clearingSpecials(labels, specials: specials, fallback: step)
    }

    /// The labels without those that would touch Overdue or Later: within two thirds of the
    /// distance the labels keep from each other (at least two bins), where a date and "Overdue"
    /// would touch.
    private func clearingSpecials(_ labels: [Int: String], specials: [Int], fallback: Int) -> [Int: String] {
        guard !specials.isEmpty else { return labels }
        let chosen = labels.keys.filter { !specials.contains($0) }.sorted()
        let spacing = zip(chosen, chosen.dropFirst()).map { $1 - $0 }.min() ?? fallback
        let clearance = max(2, (spacing * 2 + 2) / 3)
        var kept = labels
        for index in chosen where specials.contains(where: { abs($0 - index) < clearance }) {
            kept[index] = nil
        }
        return kept
    }

    // MARK: Callout and values

    /// One line per series that has a value at `bin`, in legend order; a series whose point
    /// there is open says it has too few reviews to trust (S7.2); then the callout lines, then
    /// the rules (Retention: the period's retention, its counts, then the desired retention):
    /// `(name, value text, colour)`.
    func calloutRows(_ data: StatsChartData, bin: Int) -> [(name: String, value: String, color: StatsSeriesColor?)] {
        var rows: [(name: String, value: String, color: StatsSeriesColor?)] = []
        for series in data.series {
            guard let value = data.value(series, at: bin) else { continue }
            let written = series.valueTexts.flatMap { $0.indices.contains(bin) ? $0[bin] : nil } ?? self.value(value, unit: data.unit)
            rows.append((series.name, written, series.color))
            if series.openPoints.contains(bin) {
                rows.append((String(localized: "stats.chart.openPoint", bundle: .module), "", nil))
            }
        }
        for line in data.calloutLines {
            guard let text = line.text(at: bin, format: self, unit: data.unit) else { continue }
            rows.append((line.name, text, line.color))
        }
        for rule in data.rules {
            rows.append((rule.name, self.value(rule.value, unit: data.unit), rule.color))
        }
        for marker in data.markers {
            guard marker.position != nil, let text = marker.valueText else { continue }
            rows.append((marker.name, text, marker.color))
        }
        return rows
    }

    /// The callout's text as one sentence: the accessibility element's label
    /// (S15: "each reading the callout's text").
    public func calloutText(_ data: StatsChartData, bin: Int) -> String {
        guard data.bins.indices.contains(bin) else { return "" }
        let rows = calloutRows(data, bin: bin).map { row in
            row.value.isEmpty ? row.name : row.name.isEmpty ? row.value : "\(row.name) \(row.value)"
        }
        return ([name(data.bins[bin])] + rows).joined(separator: ", ")
    }

    /// The runs of ten bins (P18.2), each named "<first> to <last>", or by its
    /// one bin's name.
    public func runs(_ data: StatsChartData) -> [(bins: Range<Int>, label: String)] {
        stride(from: 0, to: data.bins.count, by: 10).map { start in
            let range = start..<min(start + 10, data.bins.count)
            let first = name(data.bins[range.lowerBound])
            let label = range.count == 1
                ? first
                : String(localized: "stats.chart.run \(first) \(name(data.bins[range.upperBound - 1]))", bundle: .module)
            return (range, label)
        }
    }

    /// The table Copy Values puts on the pasteboard: tab-separated, one row per
    /// bin, the period then each series' value as the callout writes it (S9).
    public func tabSeparatedValues(_ data: StatsChartData) -> String {
        let period = String(localized: "stats.chart.period", bundle: .module)
        let header = ([period] + data.series.map(\.name) + data.calloutLines.map(\.name)).joined(separator: "\t")
        let rows = data.bins.indices.map { bin in
            let values = data.series.map { series in data.value(series, at: bin).map { value($0, unit: data.unit) } ?? "" }
            let extras = data.calloutLines.map { $0.text(at: bin, format: self, unit: data.unit) ?? "" }
            return ([name(data.bins[bin])] + values + extras).joined(separator: "\t")
        }
        return ([header] + rows).joined(separator: "\n")
    }
}
