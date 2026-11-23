import CoreBridge
import Foundation

/// The calendar dates of a result's days (`stats/history-reads` H1): day 0 falls on
/// `stats/activity-heatmap` AH3's `T0`, read from the result's own context (its `N`, rollover
/// hour and offset), and day `d` on `T0 + d`. Every result carries its context, so the page
/// needs no clock of its own and a time-zone change never moves a result's days.
///
/// Day arithmetic runs in UTC with the display calendar's system, first weekday and week rule
/// (a day is then always one day), and a date is shown at noon of the same calendar date in the
/// display calendar, so the formatters (S8.7) write it whatever the display zone.
nonisolated struct StatsDays: Sendable, Hashable {
    let civil: Calendar
    let display: Calendar
    /// `T0` at 00:00 UTC in `civil`.
    let dayZero: Date

    init(context: StatsContext, calendar: Calendar) {
        display = calendar
        var civil = calendar
        civil.timeZone = TimeZone(secondsFromGMT: 0) ?? calendar.timeZone
        self.civil = civil
        // AH3: the start of day 0 at the rollover hour, read twelve hours later under the offset.
        let instant = context.nextDayAt - 86_400 - Int64(context.rolloverHour) * 3_600 + 43_200
            + Int64(context.utcOffsetSeconds)
        dayZero = civil.startOfDay(for: Date(timeIntervalSince1970: TimeInterval(instant)))
    }

    /// The civil date of `day` (00:00 UTC).
    func civilDate(_ day: Int) -> Date {
        civil.date(byAdding: .day, value: day, to: dayZero) ?? dayZero
    }

    /// The day of a civil date.
    func day(ofCivil date: Date) -> Int {
        civil.dateComponents([.day], from: dayZero, to: civil.startOfDay(for: date)).day ?? 0
    }

    /// The date the formatters are given for `day`: noon of its calendar date in the display calendar.
    func date(ofDay day: Int) -> Date {
        var components = civil.dateComponents([.era, .year, .month, .day], from: civilDate(day))
        components.hour = 12
        return display.date(from: components) ?? civilDate(day)
    }

    /// The first day of the week holding `day`: weeks start on the calendar's first weekday (S4.1).
    func weekStart(ofDay day: Int) -> Int {
        guard let start = civil.dateInterval(of: .weekOfYear, for: civilDate(day))?.start else { return day }
        return self.day(ofCivil: start)
    }

    /// The first day of the calendar month holding `day`.
    func monthStart(ofDay day: Int) -> Int {
        guard let start = civil.dateInterval(of: .month, for: civilDate(day))?.start else { return day }
        return self.day(ofCivil: start)
    }

    /// The first day of the month `months` after the one holding `day`.
    func monthStart(ofDay day: Int, adding months: Int) -> Int {
        let start = civilDate(monthStart(ofDay: day))
        guard let moved = civil.date(byAdding: .month, value: months, to: start) else { return day }
        return self.day(ofCivil: moved)
    }

    /// The calendar year of `day`.
    func year(ofDay day: Int) -> Int {
        civil.component(.year, from: civilDate(day))
    }

    /// The first day of the calendar year `year`, or nil when the calendar has no such date.
    func firstDay(ofYear year: Int) -> Int? {
        var components = civil.dateComponents([.era, .year], from: dayZero)
        components.year = year
        components.month = 1
        components.day = 1
        guard let date = civil.date(from: components) else { return nil }
        return day(ofCivil: date)
    }
}
