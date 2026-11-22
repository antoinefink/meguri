import AppModel
import CoreBridge
import Foundation

/// The table's cells as text (`browser/workspace` V5, `browser/columns` C7
/// to C9, C12, C14), made off the main thread when a page of rows arrives.
/// It holds Browse's strings, read once on the main actor, and the device's
/// locale, calendar and time zone.
nonisolated struct BrowseFormatter: BrowseRowPresenting {
    let locale: Locale
    let calendar: Calendar
    let now: @Sendable () -> Date
    private let strings: Strings
    private let days: DayFormats

    /// V5.2's two day formats, the locale's own order for them. Formatting
    /// with a `DateFormatter` is safe from any thread.
    final class DayFormats: @unchecked Sendable {
        let thisYear = DateFormatter(), otherYear = DateFormatter()
        init(locale: Locale, calendar: Calendar) {
            for (formatter, template) in [(thisYear, "dMMM"), (otherYear, "dMMMy")] {
                formatter.locale = locale
                formatter.calendar = calendar
                formatter.timeZone = calendar.timeZone
                formatter.setLocalizedDateFormatFromTemplate(template)
            }
        }
    }

    /// The strings the cells use, in the app's language.
    struct Strings: Sendable {
        var today, tomorrow, filtered, newMarker, learningMarker: String
        /// "New #%@" and "(%@)" with one argument.
        var newPosition, deckCount: String
        var deleted, damaged, unreadable: String
        /// One unit's text for a whole amount (plural chosen by it), by C12's unit.
        var span: @Sendable (Unit, Int, String) -> String

        enum Unit: Sendable { case seconds, minutes, hours, days, months, years }
    }

    @MainActor init(locale: Locale, timeZone: TimeZone, now: @escaping @Sendable () -> Date) {
        self.locale = locale
        var calendar = locale.calendar
        calendar.timeZone = timeZone
        self.calendar = calendar
        self.now = now
        days = DayFormats(locale: locale, calendar: calendar)
        let bundle = BrowseStrings.bundle
        func text(_ key: String.LocalizationValue) -> String { String(localized: key, bundle: bundle, locale: locale) }
        strings = Strings(
            today: text("browse.date.today"), tomorrow: text("browse.date.tomorrow"),
            filtered: text("browse.cell.filtered"), newMarker: text("browse.cell.new"), learningMarker: text("browse.cell.learning"),
            newPosition: text("browse.cell.newPosition"), deckCount: text("browse.cell.deckCount"),
            deleted: text("browse.row.deleted"), damaged: text("browse.row.damaged"), unreadable: text("browse.row.unreadable"),
            span: { unit, count, amount in
                switch unit {
                case .seconds: String(localized: "browse.span.seconds", defaultValue: "\(count) \(amount)", bundle: bundle, locale: locale)
                case .minutes: String(localized: "browse.span.minutes", defaultValue: "\(count) \(amount)", bundle: bundle, locale: locale)
                case .hours: String(localized: "browse.span.hours", defaultValue: "\(count) \(amount)", bundle: bundle, locale: locale)
                case .days: String(localized: "browse.span.days", defaultValue: "\(count) \(amount)", bundle: bundle, locale: locale)
                case .months: String(localized: "browse.span.months", defaultValue: "\(count) \(amount)", bundle: bundle, locale: locale)
                case .years: String(localized: "browse.span.years", defaultValue: "\(count) \(amount)", bundle: bundle, locale: locale)
                }
            })
    }

    /// The key that tells one formatter's output from another's.
    var key: String {
        "\(locale.identifier)|\(calendar.timeZone.identifier)|\(calendar.startOfDay(for: now()).timeIntervalSince1970)"
    }

    // MARK: Rows

    func present(_ row: BrowseRowResult, columns: [String], mode: BrowseMode) -> BrowseShownRow {
        switch row {
        case .row(let row):
            let today = now()
            let cells = zip(columns, row.cells).map { key, cell in self.cell(cell, key: key, filtered: row.state.filtered, today: today) }
            return BrowseShownRow(status: .row, id: row.id, noteID: row.noteID, state: row.state, font: row.font,
                                  sortFieldRightToLeft: row.sortFieldRightToLeft, cells: cells)
        case .deleted: return error(.deleted, strings.deleted, columns)
        case .damaged: return error(.damaged, strings.damaged, columns)
        case .unreadable: return error(.unreadable, strings.unreadable, columns)
        }
    }

    func present(_ cells: BrowseQAResult, columns: [String]) -> [Int: BrowseShownCell] {
        guard case .cells(let question, let answer) = cells else { return [:] }
        var shown: [Int: BrowseShownCell] = [:]
        for (index, key) in columns.enumerated() {
            if key == "question", let question { shown[index] = self.question(question) }
            if key == "answer", let answer { shown[index] = self.answer(answer) }
        }
        return shown
    }

    /// C3.1: a row error shows its kind in the first column, nothing else.
    private func error(_ status: BrowseShownRow.Status, _ text: String, _ columns: [String]) -> BrowseShownRow {
        BrowseShownRow(status: status, id: nil, noteID: nil, state: nil, font: nil, sortFieldRightToLeft: false,
                       cells: columns.indices.map { $0 == 0 ? .text(text) : .empty })
    }

    // MARK: Cells

    func cell(_ cell: BrowseCell, key: String, filtered: Bool, today: Date) -> BrowseShownCell {
        switch cell {
        case .text(let text): key == "deck" ? deck(text, filtered: filtered) : .text(text)
        case .count(let n): .value(n.formatted(.number.locale(locale)))
        case .deckCount(let n): .value(String(format: strings.deckCount, n.formatted(.number.locale(locale))))
        case .percent(let n): .value((Double(n) / 100).formatted(.percent.precision(.fractionLength(0)).locale(locale)))
        case .percentReal(let value): .value(percent(value))
        case .position(let n): .value(n.formatted(.number.locale(locale)))
        // V5.2: no brackets for a suspended or buried card; the state column says it.
        case .instant(let seconds, _), .dateTime(let seconds): .value(date(seconds, today: today))
        case .newPosition(let n, _): .value(String(format: strings.newPosition, n.formatted(.number.locale(locale))))
        case .filtered: .value(strings.filtered)
        case .newMarker: .value(strings.newMarker)
        case .learningMarker: .value(strings.learningMarker)
        case .timeSpan(let seconds): .value(span(seconds))
        case .empty: .empty
        case .question(let question): self.question(question)
        case .answer(let answer): self.answer(answer)
        case .deferred: .pending
        case .noField: .noField
        case .media(let sound, let image): .media(sound: sound != nil, image: image != nil)
        }
    }

    private func question(_ question: ExcerptQuestion) -> BrowseShownCell {
        switch question {
        case .text(let text), .missingCloze(_, let text): .text(text)
        case .templateError: .empty
        }
    }

    private func answer(_ answer: BrowseAnswer) -> BrowseShownCell {
        if case .text(let text) = answer { return .text(text) }
        return .empty
    }

    /// V5.1: the deck's last component, its parent path after it with " › ".
    /// A filtered card's text is "<filtered deck> (<home deck>)" (C7.4): the
    /// filtered deck's last component, then the home deck's in parentheses.
    func deck(_ text: String, filtered: Bool) -> BrowseShownCell {
        var deck = text, home: String?
        if filtered, text.hasSuffix(")"), let open = Self.openingParenthesis(text) {
            deck = String(text[..<open])
            home = String(text[text.index(open, offsetBy: 2)..<text.index(before: text.endIndex)])
        }
        let parts = deck.components(separatedBy: "::")
        var name = parts.last ?? deck
        if let home { name += " (\(home.components(separatedBy: "::").last ?? home))" }
        let parent = parts.count > 1 ? parts.dropLast().joined(separator: " › ") : nil
        var full = parts.joined(separator: " › ")
        if let home { full += " (\(home.components(separatedBy: "::").joined(separator: " › ")))" }
        return .deck(name: name, parent: parent, full: full)
    }

    /// The " (" that opens the parenthesis closing `text`, its nesting counted.
    private static func openingParenthesis(_ text: String) -> String.Index? {
        var depth = 0, index = text.endIndex
        while index > text.startIndex {
            index = text.index(before: index)
            if text[index] == ")" { depth += 1 }
            if text[index] == "(" {
                depth -= 1
                if depth == 0 {
                    guard index > text.startIndex, text[text.index(before: index)] == " " else { return nil }
                    return text.index(before: index)
                }
            }
        }
        return nil
    }

    /// V5.2: Today and Tomorrow by name, other days as day and short month,
    /// with the year only for another year; no time of day.
    func date(_ seconds: Int64, today: Date) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(seconds))
        if calendar.isDate(date, inSameDayAs: today) { return strings.today }
        if let tomorrow = calendar.date(byAdding: .day, value: 1, to: today), calendar.isDate(date, inSameDayAs: tomorrow) {
            return strings.tomorrow
        }
        let sameYear = calendar.component(.year, from: date) == calendar.component(.year, from: today)
        return (sameYear ? days.thisYear : days.otherYear).string(from: date)
    }

    /// C12's rounded percent of an `f32`, halves to even.
    private func percent(_ value: Float) -> String {
        guard value.isFinite else { return "\(value.isNaN ? "NaN" : value > 0 ? "inf" : "-inf")%" }
        return (Double(value) / 100).formatted(.percent.precision(.fractionLength(0)).rounded(rule: .toNearestOrEven)
            .locale(locale))
    }

    /// C12's time span: the unit by the thresholds of the answer buttons,
    /// whole seconds and days, one decimal for the others.
    func span(_ seconds: Float) -> String {
        guard seconds.isFinite else { return "\(seconds)" }
        let magnitude = abs(seconds)
        let (unit, length): (Strings.Unit, Float) = switch magnitude {
        case ..<60: (.seconds, 1)
        case ..<3_600: (.minutes, 60)
        case ..<86_400: (.hours, 3_600)
        case ..<2_628_000: (.days, 86_400)
        case ..<31_536_000: (.months, 2_628_000)
        default: (.years, 31_536_000)
        }
        let quotient = seconds / length
        let amount: Float = unit == .seconds || unit == .days ? quotient.rounded(.toNearestOrAwayFromZero)
            : (quotient * 10).rounded(.toNearestOrAwayFromZero) / 10
        let text = Double(amount).formatted(.number.precision(.fractionLength(0...2)).locale(locale))
        // A whole amount picks its plural; any other reads as many.
        let count = amount == amount.rounded() && abs(amount) < Float(Int.max) ? Int(abs(amount)) : 2
        return strings.span(unit, count, text)
    }
}
