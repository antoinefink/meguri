import CoreBridge
import Foundation

/// Writes one box's values from one set (`stats/statistics-page` S4, S7, S8.5, S8.7): the bins,
/// the series, the figure and its sentence, the small figures and the footnote. Pure: the same
/// set, period, switches and decks give the same content, so the model can rebuild a box at any
/// time with no core call (S3.2).
struct StatsPresenter {
    /// What a box is when its set gives no value for the period yet (All's `allTime` ranges in a
    /// `last12Months` set): the model shows its progress.
    enum Outcome {
        case content(StatsBoxContent)
        case unavailable
    }

    let set: StatsPartSet
    let header: StatsPageHeader
    let period: StatsPagePeriod
    let selection: StatsPageSelection
    let decks: StatsDeckDirectory
    let days: StatsDays
    let format: StatsChartFormat
    let text: StatsText
    let kit: StatsSeriesKit
    let locale: Locale
    /// The axis of the past, made once per presenter (All's months cost calendar work).
    private let cache = Cache()

    private final class Cache {
        var pastAxis: StatsTimeAxis?
        var streaks: StatsStreaks??
    }

    /// Newer sets of the same scope for the values a box takes beyond its own parts.
    let extras: [StatsPartSet]

    init?(set: StatsPartSet, period: StatsPagePeriod, selection: StatsPageSelection, decks: StatsDeckDirectory,
          calendar: Calendar, locale: Locale, extras: [StatsPartSet] = []) {
        guard let header = set.header else { return nil }
        self.set = set
        self.extras = extras.filter { $0.scope == set.scope }
        self.header = header
        self.period = period
        self.selection = selection
        self.decks = decks
        self.locale = locale
        days = StatsDays(context: header.context, calendar: calendar)
        format = StatsChartFormat(locale: locale, calendar: calendar, now: days.date(ofDay: 0))
        text = StatsText(locale: locale)
        kit = StatsSeriesKit(text: text)
    }

    /// A value the set lacks, from the newest set of the scope that has it.
    func extraValue(_ part: StatsPart) -> StatsPartValue? {
        set.value(part) ?? extras.lazy.compactMap { $0.value(part) }.first
    }

    /// S7.5's streaks, once per presenter (Activity, Reviews' sentence and the strip read them).
    var streaks: StatsStreaks? {
        if let cached = cache.streaks { return cached }
        var made: StatsStreaks?
        if case let .activityDays(value)? = extraValue(.activityDays) { made = StatsStreaks(value) }
        cache.streaks = .some(made)
        return made
    }

    var source: StatsBoxSource {
        StatsBoxSource(scope: set.scope, context: header.context, browseScope: header.browseScope)
    }

    // MARK: Shared values

    /// The scope has no review at all (H9).
    var noRows: Bool { header.oldestDay == .noRows }

    var deckName: String? {
        if case let .deck(id) = set.scope { return decks.deck(id)?.name }
        return nil
    }

    /// S8.5's line for a chart with nothing in the period.
    var noReviews: StatsEmptyReason {
        if noRows { return .noReviewsYet }
        let span: StatsEmptyReason.Span? = switch period {
        case .oneMonth: .lastMonth
        case .threeMonths: .last3Months
        case .sixMonths: .last6Months
        case .oneYear: .lastYear
        case .all: nil
        }
        guard let span else { return .noReviewsYet }
        return .noReviews(in: span, deck: deckName)
    }

    /// All's first day (S4.1): H9's oldest day; with no review (or none known, H18) the oldest
    /// creation day of a scope card, never before January ten years back.
    var allFirstDay: Int {
        if case let .day(day) = header.oldestDay { return min(day, 0) }
        var candidates: [Int] = []
        if case let .cardStatesOverTime(states)? = set.value(.cardStatesOverTime), let first = states.firstDay {
            candidates.append(first)
        }
        if case let .added(added)? = set.value(.added), let first = added.keys.first { candidates.append(first) }
        if case let .started(started)? = set.value(.started), let first = started.firstDay { candidates.append(first) }
        if case .unavailable = header.oldestDay, case let .reviews(reviews)? = set.value(.reviews),
           let first = reviews.count.keys.first {
            candidates.append(first)
        }
        let floor = days.firstDay(ofYear: days.year(ofDay: 0) - 10) ?? Int.min
        return max(min(candidates.min() ?? 0, 0), floor)
    }

    var pastAxis: StatsTimeAxis {
        if let axis = cache.pastAxis { return axis }
        let axis = StatsAxes.past(period, days: days, allFirstDay: allFirstDay)
        cache.pastAxis = axis
        return axis
    }

    /// S4.3's window of a part with no time axis; nil when the set does not have it (All's
    /// `allTime`, read only with all history).
    func window<Value>(_ ranges: StatsRanges<Value>) -> Value? {
        switch period {
        case .oneMonth: ranges.oneMonth
        case .threeMonths: ranges.threeMonths
        case .sixMonths: ranges.sixMonths
        case .oneYear: ranges.oneYear
        case .all: ranges.allTime
        }
    }

    func count(_ value: Int) -> String { format.count(Double(value)) }
    func count(_ value: Double) -> String { format.count(value) }
    func percent(_ fraction: Double) -> String { format.percent(fraction) }
    func wholePercent(_ fraction: Double) -> String {
        fraction.formatted(.percent.precision(.fractionLength(0)).locale(locale))
    }
    func decimal(_ value: Double) -> String {
        value.formatted(.number.precision(.fractionLength(0...1)).locale(locale))
    }
    func signed(_ value: Int) -> String {
        Double(value).formatted(.number.sign(strategy: .always(includingZero: false)).locale(locale))
    }
    func cards(_ n: Int) -> String { text.cards(n, count(n)) }
    func seconds(milliseconds: Double) -> Double { milliseconds / 1_000 }
    /// A span of days in the short style: hours under a day, then days, months and years.
    func durationDays(_ value: Double) -> String {
        guard value.isFinite else { return text.noValue }
        if value < 1 { return format.duration(seconds: value * 86_400) }
        let total = Int(value.rounded())
        let formatter = DateComponentsFormatter()
        formatter.unitsStyle = .short
        formatter.maximumUnitCount = 2
        formatter.allowedUnits = [.year, .month, .day]
        var calendar = days.display
        calendar.locale = locale
        formatter.calendar = calendar
        let years = total / 365
        let months = (total - years * 365) / 30
        let rest = total - years * 365 - months * 30
        return formatter.string(from: DateComponents(year: years, month: months, day: rest)) ?? count(total)
    }
    func retention(passed: Int, failed: Int) -> Double? {
        passed + failed > 0 ? Double(passed) / Double(passed + failed) : nil
    }

    func content(_ phase: StatsBoxPhase, figure: StatsBoxFigure? = nil, data: StatsChartData, bins: [StatsAxisBin] = [],
                 figures: [StatsSmallFigure] = [], footnote: String? = nil, detail: StatsBoxDetail? = nil) -> Outcome {
        .content(StatsBoxContent(phase: phase, figure: figure, data: data, binDays: bins, figures: figures,
                                 footnote: footnote, detail: detail, source: source))
    }

    /// An empty box keeps its series' definitions, so its legend and size stay (S8.5).
    func empty(_ id: StatsBoxID, _ reason: StatsEmptyReason, detail: String? = nil, footnote: String? = nil) -> Outcome {
        content(.empty(reason.text, detail: detail), data: skeleton(id), figures: skeletonFigures(id), footnote: footnote)
    }

    // MARK: Boxes

    func box(_ id: StatsBoxID) -> Outcome {
        switch id {
        case .knowledgeOverTime: knowledge()
        case .retention: retentionBox()
        case .forgotten: forgottenBox()
        case .reviewsOverTime: reviewsBox()
        case .activity: activityBox()
        case .cardsOverTime: cardsOverTime()
        case .cardCounts: cardCounts()
        case .addedAndStarted: addedAndStarted()
        case .reviewsDue: reviewsDue()
        case .answers: answers()
        case .timeOfDay: timeOfDay()
        case .retrievability: retrievability()
        case .stability: stability()
        case .difficulty: difficulty()
        case .byDeck: byDeck()
        }
    }

    // S7.1
    func knowledge() -> Outcome {
        guard case let .knowledgeOverTime(value)? = set.value(.knowledgeOverTime) else { return .unavailable }
        // The suspended cards left out are said by the Estimated marker's help, not on the box
        // (Antoine, 2026-11-23); the chart's page (U302) says it in its About text.
        let footnote = value.failed > 0 ? text.couldNotEstimate(value.failed, count(value.failed)) : nil
        guard !value.samples.isEmpty else {
            return noRows
                ? empty(.knowledgeOverTime, .noReviewsYet, detail: text.fillsIn, footnote: footnote)
                : empty(.knowledgeOverTime, .noMemoryEstimate, footnote: footnote)
        }
        let axis = pastAxis
        var byDay: [Int: StatsKnowledgeSample] = [:]
        for sample in value.samples { byDay[sample.day] = sample }
        let known = axis.bins.map { byDay[$0.last]?.knowledge }
        let studied = axis.bins.map { byDay[$0.last].map { Double($0.studied) } }
        let now = byDay[0] ?? value.samples[value.samples.count - 1]
        let knownNow = Int(now.knowledge.rounded())
        // Before the first sample nothing was studied (the axis starts at the oldest review).
        let beforeDay = axis.firstDay - 1
        let earliest = value.samples.map(\.day).min() ?? 0
        let before = byDay[beforeDay].map { $0.knowledge } ?? (beforeDay < earliest ? 0 : nil)
        var sentence = text.knowledgeSentence(percent(now.studied > 0 ? now.knowledge / Double(now.studied) : 0),
                                              now.studied, count(now.studied))
        var change: Int?
        if let before {
            let delta = knownNow - Int(before.rounded())
            change = delta
            let date = format.shortDate(days.date(ofDay: beforeDay))
            sentence = delta < 0 ? text.down(sentence, count(-delta), date) : text.up(sentence, count(delta), date)
        }
        let forgotten = max(0, now.studied - knownNow)
        // The callout's gap with its share of the studied cards (S7.1: "Likely forgotten <n> (<p>%)").
        let gapTexts = zip(known, studied).map { pair -> String? in
            guard let k = pair.0, let s = pair.1 else { return nil }
            let gap = max(0, s - k.rounded())
            return text.withShare(count(gap), percent(s > 0 ? gap / s : 0))
        }
        let data = kit.knowledgeData(axis: axis, known: known, studied: studied, gapTexts: gapTexts)
        return content(.ready, figure: StatsBoxFigure(value: cards(knownNow), sentence: sentence), data: data,
                       bins: axis.bins,
                       figures: [
                           StatsSmallFigure(label: text.figureStudied, value: count(now.studied)),
                           StatsSmallFigure(label: text.figureLikelyForgotten,
                                            value: text.withShare(count(forgotten),
                                                                  percent(now.studied > 0 ? Double(forgotten) / Double(now.studied) : 0))),
                           StatsSmallFigure(label: text.figureChangeInPeriod, value: change.map(signed) ?? text.noValue),
                       ],
                       footnote: footnote)
    }


    /// H3.4: the review-weighted desired retention over `axis`'s days, or the mean of the
    /// retentions in force with no counted row; nil when the scope has none.
    func desiredRetention(_ value: StatsRetentionByDay, over axis: StatsTimeAxis?) -> Double? {
        if let axis {
            let sum = axis.sums(value.days) { $0.desiredSum }.reduce(0, +)
            let rows = axis.sums(value.days) { Double($0.desiredRows) }.reduce(0, +)
            if rows > 0 { return sum / rows }
        }
        guard !value.retentions.isEmpty else { return nil }
        return value.retentions.map(Double.init).reduce(0, +) / Double(value.retentions.count)
    }

    // S7.2
    func retentionBox() -> Outcome {
        guard case let .retentionByDay(value)? = set.value(.retentionByDay) else { return .unavailable }
        let axis = pastAxis
        guard axis.covers(value.days, where: { $0.youngPassed + $0.youngFailed + $0.maturePassed + $0.matureFailed > 0 }) else {
            return empty(.retention, noReviews)
        }
        let kind = selection.retentionKind
        func passed(_ day: StatsRetentionDay) -> Double {
            Double(kind == .young ? day.youngPassed : kind == .mature ? day.maturePassed : day.youngPassed + day.maturePassed)
        }
        func failed(_ day: StatsRetentionDay) -> Double {
            Double(kind == .young ? day.youngFailed : kind == .mature ? day.matureFailed : day.youngFailed + day.matureFailed)
        }
        let pass = axis.sums(value.days, passed)
        let fail = axis.sums(value.days, failed)
        var values: [Double?] = []
        var open = IndexSet()
        for index in axis.bins.indices {
            let total = pass[index] + fail[index]
            values.append(total > 0 ? pass[index] / total : nil)
            if total > 0, total < 20 { open.insert(index) }
        }
        let desired = desiredRetention(value, over: axis)
        let data = kit.retentionData(axis: axis, values: values, open: open, desired: desired, several: value.several,
                                 remembered: pass, forgotten: fail)
        let passedTotal = Int(pass.reduce(0, +)), failedTotal = Int(fail.reduce(0, +))
        let young = axis.sums(value.days) { Double($0.youngPassed) }.reduce(0, +)
        let youngFailed = axis.sums(value.days) { Double($0.youngFailed) }.reduce(0, +)
        let mature = axis.sums(value.days) { Double($0.maturePassed) }.reduce(0, +)
        let matureFailed = axis.sums(value.days) { Double($0.matureFailed) }.reduce(0, +)
        func rate(_ p: Double, _ f: Double) -> String { p + f > 0 ? percent(p / (p + f)) : text.noValue }
        let sentence = desired.map { value.several ? text.averageDesiredSentence(percent($0)) : text.desiredSentence(percent($0)) } ?? ""
        return content(.ready,
                       figure: StatsBoxFigure(value: rate(Double(passedTotal), Double(failedTotal)), sentence: sentence),
                       data: data, bins: axis.bins,
                       figures: [
                           StatsSmallFigure(label: text.figureRemembered, value: count(passedTotal)),
                           StatsSmallFigure(label: text.figureForgotten, value: count(failedTotal)),
                           StatsSmallFigure(label: text.figureYoung, value: rate(young, youngFailed)),
                           StatsSmallFigure(label: text.figureMature, value: rate(mature, matureFailed)),
                       ])
    }


    // S7.3
    func forgottenBox() -> Outcome {
        guard case let .retentionByDay(value)? = set.value(.retentionByDay) else { return .unavailable }
        let axis = pastAxis
        guard axis.covers(value.days, where: { $0.youngPassed + $0.youngFailed + $0.maturePassed + $0.matureFailed > 0 }) else {
            return empty(.forgotten, noReviews)
        }
        let fails = axis.sums(value.days) { Double($0.forgotten) }
        let young = Int(axis.sums(value.days) { Double($0.youngFailed) }.reduce(0, +))
        let mature = Int(axis.sums(value.days) { Double($0.matureFailed) }.reduce(0, +))
        let total = young + mature
        var data = kit.forgottenData(axis: axis, values: fails)
        if period == .oneMonth { data.browsable = IndexSet(fails.indices.filter { fails[$0] > 0 }) }
        return content(.ready,
                       figure: StatsBoxFigure(value: count(total), sentence: text.perDay(decimal(Double(total) / Double(axis.days)))),
                       data: data, bins: axis.bins,
                       figures: [StatsSmallFigure(label: text.figureYoung, value: count(young)),
                                 StatsSmallFigure(label: text.figureMature, value: count(mature))])
    }


    // S7.4
    func reviewsBox() -> Outcome {
        guard case let .reviews(reviews)? = set.value(.reviews) else { return .unavailable }
        let axis = pastAxis
        guard axis.covers(reviews.count, where: { $0.total > 0 }) else { return empty(.reviewsOverTime, noReviews) }
        let time = selection.reviewsMeasure == .time
        let scale: (Int) -> Double = { time ? Double($0) / 1_000 : Double($0) }
        var series: [StatsSeries]
        if selection.reviewsMode == .byAnswer {
            guard case let .byAnswer(answers)? = set.value(.byAnswer) else { return .unavailable }
            let map = time ? answers.time : answers.count
            series = kit.reviewsAnswerSeries([
                axis.sums(map) { scale($0.again) }, axis.sums(map) { scale($0.hard) },
                axis.sums(map) { scale($0.good) }, axis.sums(map) { scale($0.easy) },
            ])
        } else {
            let map = time ? reviews.time : reviews.count
            let filtered = axis.sums(reviews.count) { Double($0.filtered) }
            series = kit.reviewsTypeSeries([
                axis.sums(map) { scale($0.mature) }, axis.sums(map) { scale($0.young) },
                axis.sums(map) { scale($0.relearn) }, axis.sums(map) { scale($0.learn) },
            ], filtered: filtered.contains { $0 > 0 } ? axis.sums(map) { scale($0.filtered) } : nil)
        }
        // The callout's total and the figures are By Type's, unrated rows included (H6).
        let totals = axis.sums(time ? reviews.time : reviews.count) { scale($0.total) }
        var data = StatsChartData(bins: axis.periods, series: series, unit: time ? .duration : .count,
                                  calloutLines: [StatsCalloutLine(id: "total", name: text.total, values: totals.map { $0 })])
        if period == .oneMonth { data.browsable = IndexSet(totals.indices.filter { totals[$0] > 0 }) }
        let total = totals.reduce(0, +)
        let reviewCount = axis.sums(reviews.count) { Double($0.total) }
        let studiedDays = Set(reviews.count.keys.indices.compactMap { index -> Int? in
            reviews.count.values[index].total > 0 && axis.index(ofDay: reviews.count.keys[index]) != nil
                ? min(reviews.count.keys[index], 0) : nil
        }).count
        let streak = streaks?.current
        let sentence = (streak ?? 0) > 1
            ? text.daysInARow(streak ?? 0, count(streak ?? 0))
            : text.daysStudied(studiedDays, count(studiedDays))
        func written(_ value: Double) -> String { time ? format.duration(seconds: value) : count(value) }
        var figures = [
            StatsSmallFigure(label: text.figureDaysStudied, value: text.aOfB(count(studiedDays), count(axis.days))),
            StatsSmallFigure(label: text.figureAverageADay, value: written(total / Double(axis.days))),
            StatsSmallFigure(label: text.figureAverageOnDaysStudied,
                             value: studiedDays > 0 ? written(total / Double(studiedDays)) : text.noValue),
        ]
        if time {
            let answers = reviewCount.reduce(0, +)
            figures.append(StatsSmallFigure(label: text.figureTimePerCard,
                                            value: answers > 0 ? format.duration(seconds: total / answers) : text.noValue))
        }
        return content(.ready, figure: StatsBoxFigure(value: written(total), sentence: sentence), data: data,
                       bins: axis.bins, figures: figures)
    }



    // S7.5
    func activityBox() -> Outcome {
        guard set.value(.activityDays) != nil, let streaks else { return .unavailable }
        guard let firstCounted = streaks.counts.keys.min() else { return empty(.activity, .noReviewsYet) }
        // From H9's oldest review when the header has it, as the loading grid was sized (S10.3).
        let oldest: Int = if case let .day(day) = header.oldestDay { min(day, firstCounted) } else { firstCounted }
        // The period's window (Antoine, 2026-11-23): 1M to 1Y show their days ending today, All
        // every day. The headline stays the streak's and this year's.
        let span = StatsActivitySpan(firstDay: period.activityFirstDay(oldest: oldest), days: days)
        let years = span.years.map { year in
            let first = days.firstDay(ofYear: year) ?? Int.min
            let last = (days.firstDay(ofYear: year + 1) ?? 1) - 1
            let total = streaks.counts.reduce(0) { $0 + ($1.key >= first && $1.key <= last ? $1.value : 0) }
            // A year's total beside its label only where the window holds the whole year so far
            // (from 1 January); a year the window starts inside keeps its label alone.
            guard period.activityWindowDays.map({ 1 - $0 <= first }) ?? true else {
                return StatsActivity.Year(year: year, total: total, totalText: String(year))
            }
            return StatsActivity.Year(year: year, total: total, totalText: text.yearTotal(String(year), total, count(total)),
                                      totalFull: text.totalFull(total, count(total)), totalNumber: text.totalNumber(count(total)))
        }
        let thisYearStart = days.firstDay(ofYear: days.year(ofDay: 0)) ?? 0
        let thisYear = streaks.counts.keys.filter { $0 >= thisYearStart }.count
        var activity = StatsActivity(span: span, counts: streaks.counts, years: years, currentStreak: streaks.current,
                                     longestStreak: streaks.longest, daysStudiedThisYear: thisYear, days: days)
        activity.daysThisYear = max(1, 1 - thisYearStart)
        activity.windowed = period.activityWindowDays != nil
        return content(.ready, figure: Self.activityFigure(activity, text: text, count: count), data: StatsChartData(bins: [], series: []),
                       detail: .activity(activity))
    }

    /// Activity's headline and sentence (the owner's Option B, 2026-11-23): the current streak as
    /// "<n> days in a row" ("No streak today" at 0), then the longest streak, or "Your longest
    /// streak" when the current one is it, and the days studied of this year's days so far.
    static func activityFigure(_ activity: StatsActivity, text: StatsText, count: (Int) -> String) -> StatsBoxFigure {
        let studied = text.studiedThisYear(count(activity.daysStudiedThisYear), activity.daysThisYear, count(activity.daysThisYear))
        let current = activity.currentStreak, longest = activity.longestStreak
        guard current > 0 else {
            return StatsBoxFigure(value: text.noStreak, sentence: text.longest(text.days(longest, count(longest)), studied))
        }
        let sentence = current >= longest ? text.yourLongest(studied) : text.longest(text.days(longest, count(longest)), studied)
        return StatsBoxFigure(value: text.inARow(current, count(current)), sentence: sentence)
    }

    // S7.6
    func cardsOverTime() -> Outcome {
        guard case let .cardStatesOverTime(value)? = set.value(.cardStatesOverTime) else { return .unavailable }
        // The suspended cards left out are not said on the box (Antoine, 2026-11-23); the chart's
        // page says it in its About text (S9, U302).
        let footnote: String? = nil
        if noRows { return empty(.cardsOverTime, .noReviewsYet, footnote: footnote) }
        let axis = pastAxis
        let levels = Self.levels(value, at: axis.bins.map(\.last))
        let before = Self.levels(value, at: [axis.firstDay - 1])[0]
        let now = levels.last ?? value.start
        let data = kit.cardsData(axis: axis, levels: levels)
        let matureChange = now.mature - before.mature
        let sentence = matureChange < 0
            ? text.matureDown(count(now.mature), count(-matureChange))
            : text.matureUp(count(now.mature), count(matureChange))
        func figure(_ label: String, _ now: Int, _ then: Int) -> StatsSmallFigure {
            StatsSmallFigure(label: label, value: text.withChange(count(now), signed(now - then)))
        }
        return content(.ready, figure: StatsBoxFigure(value: cards(now.total), sentence: sentence), data: data,
                       bins: axis.bins,
                       figures: [
                           figure(text.figureNew, now.new, before.new),
                           figure(text.figureLearning, now.learning + now.relearning, before.learning + before.relearning),
                           figure(text.figureYoung, now.young, before.young),
                           figure(text.figureMature, now.mature, before.mature),
                       ],
                       footnote: footnote)
    }

    /// H2.4: the counts at the end of each day (ascending), `start` plus the deltas up to it, a
    /// delta after today counting on day 0.
    static func levels(_ value: StatsCardStatesOverTime, at ends: [Int]) -> [StatsStateCounts] {
        var out: [StatsStateCounts] = []
        var running = value.start
        var next = 0
        for end in ends {
            while next < value.deltas.keys.count, min(value.deltas.keys[next], 0) <= end {
                running = running + value.deltas.values[next]
                next += 1
            }
            out.append(running)
        }
        return out
    }


    // S7.7
    func cardCounts() -> Outcome {
        guard case let .cardCounts(value)? = set.value(.cardCounts) else { return .unavailable }
        let counts = value.excludingInactive
        let rows: [(StatsCountLabel, String, StatsSeriesColor, Int)] = [
            (.mature, text.mature, .mature, counts.mature), (.young, text.young, .young, counts.young),
            (.relearning, text.relearning, .relearning, counts.relearning), (.learning, text.learning, .learning, counts.learning),
            (.new, text.new, .new, counts.new), (.suspended, text.suspended, .suspended, counts.suspended),
            (.buried, text.buried, .buried, counts.buried),
        ]
        let total = rows.reduce(0) { $0 + $1.3 }
        let items = Dictionary(value.labelsSeparate.map { ($0.label, $0.itemText) }, uniquingKeysWith: { first, _ in first })
        let table = StatsCardCountsTable(
            rows: rows.map { row in
                let share = total > 0 ? Double(row.3) / Double(total) : 0
                return StatsCardCountsTable.Row(label: row.0, name: row.1, color: row.2, count: row.3, countText: count(row.3),
                                                share: share, shareText: percent(share), itemText: items[row.0])
            },
            total: count(total), notes: count(value.notes))
        let data = StatsChartData(
            bins: [.bucket(text.total)],
            series: rows.map { StatsSeries(id: "\($0.0)", name: $0.1, color: $0.2, mark: .stackedBar, values: [Double($0.3)]) })
        return content(.ready,
                       figure: StatsBoxFigure(value: text.cardsInNotes(total, count(total), value.notes, count(value.notes)), sentence: ""),
                       data: data,
                       figures: [StatsSmallFigure(label: text.figureMature, value: text.aOfB(count(counts.mature), count(total)))],
                       detail: .cardCounts(table))
    }

    // S7.8
    func addedAndStarted() -> Outcome {
        guard case let .added(added)? = set.value(.added), case let .started(started)? = set.value(.started) else {
            return .unavailable
        }
        let axis = pastAxis
        let addedSums = axis.sums(added) { Double($0) }
        let startedSums = axis.sums(started.days) { Double($0) }
        var data = kit.addedData(axis: axis, added: addedSums, started: startedSums)
        data.browsable = IndexSet(addedSums.indices.filter { addedSums[$0] > 0 })
        let addedTotal = addedSums.reduce(0, +), startedTotal = startedSums.reduce(0, +)
        return content(.ready,
                       figure: StatsBoxFigure(value: count(addedTotal), sentence: text.startedSentence(count(startedTotal))),
                       data: data, bins: axis.bins,
                       figures: [
                           StatsSmallFigure(label: text.figureAddedADay, value: decimal(addedTotal / Double(axis.days))),
                           StatsSmallFigure(label: text.figureStartedADay, value: decimal(startedTotal / Double(axis.days))),
                       ])
    }


    // S7.9
    func reviewsDue(remaining: Int?) -> Outcome {
        guard case let .futureDue(value)? = set.value(.futureDue) else { return .unavailable }
        guard !value.due.isEmpty else { return empty(.reviewsDue, .noCardsDue) }
        let axis = StatsAxes.ahead(period, days: days, due: value.due)
        let totals = axis.aheadSums(value.due)
        let young = axis.aheadSums(value.dueYoung)
        let overdueIndex = axis.bins.firstIndex { $0.period == .overdue }
        var mature: [Double?] = [], youngValues: [Double?] = [], overdue: [Double?] = []
        for index in axis.bins.indices {
            if index == overdueIndex {
                mature.append(nil); youngValues.append(nil); overdue.append(totals[index])
            } else {
                mature.append(totals[index] - young[index]); youngValues.append(young[index]); overdue.append(nil)
            }
        }
        var running = 0.0
        let byThen = totals.map { running += $0; return Optional(running) }
        var data = kit.reviewsDueData(axis: axis, mature: mature, young: youngValues, overdue: overdueIndex == nil ? nil : overdue,
                                  byThen: byThen)
        data.browsable = IndexSet(totals.indices.filter { totals[$0] > 0 })
        let overdueTotal = zip(value.due.keys, value.due.values).filter { $0.0 < 0 }.reduce(0) { $0 + $1.1 }
        let next30 = zip(value.due.keys, value.due.values).filter { (0...29).contains($0.0) }.reduce(0) { $0 + $1.1 }
        let tomorrow = count(value.dueTomorrow)
        let sentence = remaining.map { text.remainingTomorrow(count($0), tomorrow) } ?? text.onlyTomorrow(tomorrow)
        return content(.ready, figure: StatsBoxFigure(value: count(totals.reduce(0, +)), sentence: sentence), data: data,
                       bins: axis.bins,
                       figures: [
                           StatsSmallFigure(label: text.figureOverdue, value: count(overdueTotal)),
                           StatsSmallFigure(label: text.figureDueTomorrow, value: tomorrow),
                           StatsSmallFigure(label: text.figureNext30Days, value: count(next30)),
                           StatsSmallFigure(label: text.figureDailyLoad, value: count(value.dailyLoad)),
                       ])
    }

    func reviewsDue() -> Outcome { reviewsDue(remaining: decks.remaining(set.scope)) }


    // S7.10
    func answers() -> Outcome {
        guard case let .buttons(value)? = set.value(.buttons) else { return .unavailable }
        guard let presses = window(value.ranges) else { return .unavailable }
        let groups: [(String, [Int])] = [(text.learning, presses.learning), (text.young, presses.young), (text.mature, presses.mature)]
        let totals = (0..<4).map { button in groups.reduce(0) { $0 + ($1.1.indices.contains(button) ? $1.1[button] : 0) } }
        let total = totals.reduce(0, +)
        guard total > 0 else { return empty(.answers, noReviews) }
        let names = [text.again, text.hard, text.good, text.easy]
        let colors: [StatsSeriesColor] = [.again, .hard, .good, .easy]
        let data = StatsChartData(
            bins: groups.map { StatsPeriod.bucket($0.0) },
            series: (0..<4).map { button in
                StatsSeries(id: ["again", "hard", "good", "easy"][button], name: names[button], color: colors[button],
                            mark: .stackedBar,
                            values: groups.map { Double($0.1.indices.contains(button) ? $0.1[button] : 0) })
            })
        let largest = totals.indices.max { totals[$0] < totals[$1] } ?? 2
        let table = StatsAnswersTable(rows: groups.map { group in
            let sum = group.1.reduce(0, +)
            return StatsAnswersTable.Row(name: group.0, counts: group.1, countTexts: group.1.map(count),
                                         shareTexts: group.1.map { sum > 0 ? percent(Double($0) / Double(sum)) : text.noValue })
        })
        return content(.ready,
                       figure: StatsBoxFigure(value: count(total),
                                              sentence: text.shareOfButton(percent(Double(totals[largest]) / Double(total)), names[largest])),
                       data: data,
                       figures: groups.map { group in
                           let sum = group.1.reduce(0, +)
                           return StatsSmallFigure(label: text.againShare(group.0),
                                                   value: sum > 0 ? percent(Double(group.1.first ?? 0) / Double(sum)) : text.noValue)
                       },
                       detail: .answers(table))
    }

    // S7.11
    func timeOfDay() -> Outcome {
        guard case let .hours(value)? = set.value(.hours) else { return .unavailable }
        guard let hours = window(value.hours), let retention = window(value.retentionByHour) else { return .unavailable }
        let totals = (0..<24).map { hours.indices.contains($0) ? hours[$0].total : 0 }
        let total = totals.reduce(0, +)
        guard total > 0 else { return empty(.timeOfDay, noReviews) }
        let shares: [Double?] = (0..<24).map { hour in
            guard retention.indices.contains(hour) else { return nil }
            let row = retention[hour]
            return row.passed + row.failed >= 20 ? Double(row.passed) / Double(row.passed + row.failed) : nil
        }
        let (lower, upper) = kit.timeOfDayData(totals: totals.map(Double.init), shares: shares)
        let busiest = totals.indices.max { totals[$0] < totals[$1] } ?? 0
        let best = shares.indices.filter { shares[$0] != nil }.max { (shares[$0] ?? 0) < (shares[$1] ?? 0) }
        let beforeNoon = totals.prefix(12).reduce(0, +)
        return content(.ready, figure: StatsBoxFigure(value: format.name(.hour(busiest)), sentence: text.busiestHour),
                       data: lower,
                       figures: [
                           StatsSmallFigure(label: text.figureBestHour, value: best.map { format.name(.hour($0)) } ?? text.noValue),
                           StatsSmallFigure(label: text.figureBeforeNoon, value: percent(Double(beforeNoon) / Double(total))),
                       ],
                       detail: .timeOfDay(retention: upper))
    }


    // S7.12 to S7.14
    func memoryFootnote(_ leftOut: Int) -> String? {
        leftOut > 0 ? text.withoutMemoryState(leftOut, count(leftOut)) : nil
    }

    /// Bins of `width` percent from the 1% keys, keys of 100 and above in the last bin.
    func percentBins(_ map: StatsSparseMap<Int>, width: Int) -> (bins: [StatsPeriod], values: [Double]) {
        let count = 100 / width
        var values = Array(repeating: 0.0, count: count)
        for (key, value) in zip(map.keys, map.values) where key >= 0 {
            values[min(key / width, count - 1)] += Double(value)
        }
        let bins = (0..<count).map { n in
            StatsPeriod.bucket(text.range(wholePercent(Double(n * width) / 100), wholePercent(Double((n + 1) * width) / 100)))
        }
        return (bins, values)
    }


    func retrievability() -> Outcome {
        guard case let .retrievability(value)? = set.value(.retrievability) else { return .unavailable }
        let footnote = memoryFootnote(value.leftOut)
        guard !value.byPercent.isEmpty else { return empty(.retrievability, .noMemoryState, footnote: footnote) }
        let (bins, values) = percentBins(value.byPercent, width: 5)
        let desired: Double? = {
            guard case let .retentionByDay(retention)? = extraValue(.retentionByDay) else { return nil }
            return desiredRetention(retention, over: nil)
        }()
        let below = desired.map { desired in
            zip(value.byPercent.keys, value.byPercent.values).filter { Double($0.0) < desired * 100 }.reduce(0) { $0 + $1.1 }
        }
        let withState = value.byPercent.values.reduce(0, +)
        func rounded(_ value: Float) -> String { value.isFinite ? count(Double(value).rounded()) : text.noValue }
        let average = value.average.isFinite ? wholePercent(Double(value.average) / 100) : text.noValue
        return content(.ready,
                       figure: StatsBoxFigure(value: average,
                                              sentence: text.totalKnowledge(rounded(value.sumByCard), rounded(value.sumByNote))),
                       data: kit.memoryData(bins: bins, values: values, color: .remembered),
                       figures: [
                           StatsSmallFigure(label: text.figureBelowDesired, value: below.map(count)),
                           StatsSmallFigure(label: text.figureWithMemoryState, value: count(withState)),
                       ],
                       footnote: footnote, detail: .retrievability(desiredRetention: desired))
    }

    /// S7.13's doubling bins on the rounded stability key, in days.
    static let stabilityEdges = [0, 1, 3, 7, 14, 28, 90, 180, 365, 730]

    func stability() -> Outcome {
        guard case let .stability(value)? = set.value(.stability), case let .intervals(intervals)? = set.value(.intervals) else {
            return .unavailable
        }
        let footnote = memoryFootnote(value.leftOut)
        guard !value.byDays.isEmpty else { return empty(.stability, .noMemoryState, footnote: footnote) }
        var values = Array(repeating: 0.0, count: Self.stabilityEdges.count)
        for (key, cards) in zip(value.byDays.keys, value.byDays.values) {
            let bin = (Self.stabilityEdges.lastIndex { $0 <= max(key, 0) }) ?? 0
            values[bin] += Double(cards)
        }
        let median = StatsQuantile.interpolated(value.byDays, 0.5)
        let medianInterval = StatsQuantile.interpolated(intervals, 0.5).map { durationDays($0.rounded()) }
        let overYear = zip(value.byDays.keys, value.byDays.values).filter { $0.0 >= 365 }.reduce(0) { $0 + $1.1 }
        return content(.ready,
                       figure: StatsBoxFigure(value: median.map(durationDays) ?? text.noValue, sentence: text.medianStability),
                       data: kit.memoryData(bins: text.stabilityBins.map(StatsPeriod.bucket), values: values, color: .measure),
                       figures: [
                           StatsSmallFigure(label: text.figureMedianInterval, value: medianInterval ?? text.noValue),
                           StatsSmallFigure(label: text.figureOverOneYear, value: count(overYear)),
                       ],
                       footnote: footnote)
    }

    func difficulty() -> Outcome {
        guard case let .difficulty(value)? = set.value(.difficulty) else { return .unavailable }
        let footnote = memoryFootnote(value.leftOut)
        guard !value.byPercent.isEmpty else { return empty(.difficulty, .noMemoryState, footnote: footnote) }
        let (bins, values) = percentBins(value.byPercent, width: 10)
        let median = value.median.isFinite ? wholePercent(Double(value.median) / 100) : text.noValue
        return content(.ready, figure: StatsBoxFigure(value: median, sentence: text.medianDifficulty),
                       data: kit.memoryData(bins: bins, values: values, color: .measure),
                       figures: [StatsSmallFigure(label: text.figureTopTenth, value: count(values.last ?? 0))],
                       footnote: footnote)
    }

    // S7.15
    func byDeck() -> Outcome {
        // Its rows are named from the deck tree: none before it is read.
        guard case let .byDeck(value)? = set.value(.byDeck), decks.isKnown else { return .unavailable }
        if case let .deck(id) = set.scope, let deck = decks.deck(id), deck.children.isEmpty {
            return content(.empty(text.noSubdecks), data: skeleton(.byDeck))
        }
        var rows: [(StatsDeckRow, StatsDeckValues, String)] = []
        for row in value.rows {
            guard let values = window(row.windows) else { return .unavailable }
            // A deck the tree has not caught up with keeps its row, so the shares add up.
            rows.append((row, values, decks.deck(row.deckID)?.name ?? text.noValue))
        }
        let reviews = rows.reduce(0) { $0 + $1.1.reviews }
        guard reviews > 0 else { return empty(.byDeck, noReviews) }
        let time = rows.reduce(0) { $0 + $1.1.timeMilliseconds }
        let table = StatsByDeckTable(rows: rows.map { row, values, name in
            let rate = retention(passed: values.passed, failed: values.failed)
            let share = time > 0 ? Double(values.timeMilliseconds) / Double(time) : 0
            return StatsByDeckTable.Row(deckID: row.deckID, name: name, ownCards: row.ownCards, cards: row.cards,
                                        reviews: values.reviews, retention: rate, timeShare: share,
                                        cardsText: count(row.cards), reviewsText: count(values.reviews),
                                        retentionText: rate.map(percent) ?? text.noValue, timeShareText: percent(share))
        }, note: nil)
        let largest = table.rows.max { $0.timeShare < $1.timeShare }
        let data = StatsChartData(bins: table.rows.map { StatsPeriod.bucket($0.name) },
                                  series: [StatsSeries(id: "time", name: text.reviewsSeries, color: .measure, mark: .bar,
                                                       values: table.rows.map { $0.timeShare })],
                                  unit: .fraction)
        return content(.ready,
                       figure: largest.map { StatsBoxFigure(value: percent($0.timeShare), sentence: text.timeIn($0.name)) },
                       data: data, detail: .byDeck(table))
    }

    // MARK: Empty charts (S10.3)

    func skeleton(_ id: StatsBoxID) -> StatsChartData { kit.skeleton(id, selection: selection) }

    func skeletonFigures(_ id: StatsBoxID) -> [StatsSmallFigure] { kit.skeletonFigures(id, selection: selection) }
}

/// The streaks of S7.5 from `activity_days` (H11).
struct StatsStreaks {
    /// Day (folded at 0) → reviews, only studied days.
    let counts: [Int: Int]
    let current: Int
    let longest: Int

    init(_ map: StatsSparseMap<Int>) {
        var counts: [Int: Int] = [:]
        for (key, value) in zip(map.keys, map.values) where value > 0 { counts[min(key, 0), default: 0] += value }
        self.counts = counts
        // Today not yet studied does not break the streak: it then ends yesterday.
        var day = counts[0] != nil ? 0 : -1
        var current = 0
        while counts[day] != nil {
            current += 1
            day -= 1
        }
        self.current = current
        var longest = 0, run = 0, previous: Int?
        for day in counts.keys.sorted() {
            run = previous == day - 1 ? run + 1 : 1
            longest = max(longest, run)
            previous = day
        }
        self.longest = longest
    }
}

/// `stats/graph-presentation` P4.5's interpolated quantile over a count map.
enum StatsQuantile {
    static func interpolated(_ map: StatsSparseMap<Int>, _ p: Double) -> Double? {
        let entries = zip(map.keys, map.values).filter { $0.1 > 0 }
        let n = entries.reduce(0) { $0 + $1.1 }
        guard n > 0, let first = entries.first, let last = entries.last else { return nil }
        if p <= 0 || n < 2 { return Double(first.0) }
        if p >= 1 { return Double(last.0) }
        func value(_ j: Int) -> Double {
            var cumulative = 0
            for entry in entries {
                cumulative += entry.1
                if cumulative > j { return Double(entry.0) }
            }
            return Double(last.0)
        }
        let h = Double(n - 1) * p
        let i = Int(h.rounded(.down))
        let low = value(i)
        return low + (value(i + 1) - low) * (h - Double(i))
    }
}

/// The series of each box, with or without values: a box not ready keeps its legend (S10.3).
struct StatsSeriesKit {
    let text: StatsText

    func knowledgeData(axis: StatsTimeAxis?, known: [Double?], studied: [Double?], gapTexts: [String?]? = nil) -> StatsChartData {
        let gap = zip(known, studied).map { pair -> Double? in
            guard let k = pair.0, let s = pair.1 else { return nil }
            return max(0, s - k.rounded())
        }
        return StatsChartData(
            bins: axis?.periods ?? [],
            series: [
                StatsSeries(id: "known", name: text.known, color: .remembered, mark: .line(), values: known),
                StatsSeries(id: "studied", name: text.studied, color: .reference, mark: .line(dashed: true), values: studied),
            ],
            yScale: .fitted,
            calloutLines: axis == nil ? [] : [StatsCalloutLine(id: "likelyForgotten", name: text.likelyForgotten, values: gap,
                                                               texts: gapTexts, color: .forgotten)],
            // The gap between what was studied and what is known, as a band (Antoine, 2026-11-23).
            bands: [StatsBand(id: "likelyForgotten", name: text.likelyForgotten, color: .forgotten, lower: "known",
                              upper: "studied", spoken: text.likelyForgottenBand)]
        )
    }

    func retentionData(axis: StatsTimeAxis?, values: [Double?], open: IndexSet, desired: Double?, several: Bool,
                       remembered: [Double], forgotten: [Double]) -> StatsChartData {
        StatsChartData(
            bins: axis?.periods ?? [],
            series: [StatsSeries(id: "retention", name: text.retention, color: .remembered, mark: .line(points: true),
                                 values: values, openPoints: open)],
            rules: desired.map { [StatsRule(id: "desired", name: several ? text.averageDesiredRetention : text.desiredRetention, value: $0)] } ?? [],
            unit: .fraction,
            yScale: .fraction(lower: 0.7),
            calloutLines: axis == nil ? [] : [
                StatsCalloutLine(id: "remembered", name: text.remembered, values: remembered.map { $0 }, unit: .count),
                StatsCalloutLine(id: "forgotten", name: text.forgotten, values: forgotten.map { $0 }, unit: .count),
            ]
        )
    }

    func forgottenData(axis: StatsTimeAxis?, values: [Double]) -> StatsChartData {
        StatsChartData(bins: axis?.periods ?? [],
                       series: [StatsSeries(id: "forgotten", name: text.forgotten, color: .forgotten, mark: .bar,
                                            values: values.map { $0 })])
    }

    func reviewsTypeSeries(_ values: [[Double]], filtered: [Double]?) -> [StatsSeries] {
        let kinds: [(String, String, StatsSeriesColor)] = [
            ("mature", text.mature, .mature), ("young", text.young, .young),
            ("relearning", text.relearning, .relearning), ("learning", text.learning, .learning),
        ]
        var series = zip(kinds, values.isEmpty ? Array(repeating: [], count: 4) : values).map { kind, values in
            StatsSeries(id: kind.0, name: kind.1, color: kind.2, mark: .stackedBar, values: values.map { $0 })
        }
        if let filtered {
            series.append(StatsSeries(id: "filtered", name: text.filtered, color: .filtered, mark: .stackedBar,
                                      values: filtered.map { $0 }))
        }
        return series
    }

    func reviewsAnswerSeries(_ values: [[Double]]) -> [StatsSeries] {
        let kinds: [(String, String, StatsSeriesColor)] = [
            ("again", text.again, .again), ("hard", text.hard, .hard), ("good", text.good, .good), ("easy", text.easy, .easy),
        ]
        return zip(kinds, values.isEmpty ? Array(repeating: [], count: 4) : values).map { kind, values in
            StatsSeries(id: kind.0, name: kind.1, color: kind.2, mark: .stackedBar, values: values.map { $0 })
        }
    }

    func cardsData(axis: StatsTimeAxis?, levels: [StatsStateCounts]) -> StatsChartData {
        let kinds: [(String, String, StatsSeriesColor, (StatsStateCounts) -> Int)] = [
            ("mature", text.mature, .mature, \.mature), ("young", text.young, .young, \.young),
            ("relearning", text.relearning, .relearning, \.relearning), ("learning", text.learning, .learning, \.learning),
            ("new", text.new, .new, \.new),
        ]
        return StatsChartData(
            bins: axis?.periods ?? [],
            series: kinds.map { kind in
                StatsSeries(id: kind.0, name: kind.1, color: kind.2, mark: .stackedArea, values: levels.map { Double(kind.3($0)) })
            },
            calloutLines: axis == nil ? [] : [StatsCalloutLine(id: "total", name: text.total, values: levels.map { Double($0.total) })]
        )
    }

    func addedData(axis: StatsTimeAxis?, added: [Double], started: [Double]) -> StatsChartData {
        StatsChartData(bins: axis?.periods ?? [], series: [
            StatsSeries(id: "added", name: text.added, color: .new, mark: .bar, values: added.map { $0 }),
            StatsSeries(id: "started", name: text.started, color: .learning, mark: .line(), values: started.map { $0 }),
        ])
    }

    func reviewsDueData(axis: StatsTimeAxis?, mature: [Double?], young: [Double?], overdue: [Double?]?,
                        byThen: [Double?]) -> StatsChartData {
        var series = [
            StatsSeries(id: "mature", name: text.mature, color: .mature, mark: .stackedBar, values: mature),
            StatsSeries(id: "young", name: text.young, color: .young, mark: .stackedBar, values: young),
        ]
        if let overdue {
            series.append(StatsSeries(id: "overdue", name: text.overdue, color: .buried, mark: .stackedBar, values: overdue))
        }
        return StatsChartData(bins: axis?.periods ?? [], series: series,
                              calloutLines: axis == nil ? [] : [StatsCalloutLine(id: "totalByThen", name: text.totalByThen, values: byThen)])
    }

    func timeOfDayData(totals: [Double], shares: [Double?]) -> (StatsChartData, StatsChartData) {
        let bins = totals.isEmpty ? [] : (0..<24).map(StatsPeriod.hour)
        return (
            StatsChartData(bins: bins, series: [StatsSeries(id: "reviews", name: text.reviewsSeries, color: .measure, mark: .bar,
                                                            values: totals.map { $0 })]),
            StatsChartData(bins: bins, series: [StatsSeries(id: "remembered", name: text.remembered, color: .remembered,
                                                            mark: .line(points: true), values: shares)],
                           unit: .fraction, yScale: .fraction(lower: 0.7))
        )
    }

    func memoryData(bins: [StatsPeriod], values: [Double], color: StatsSeriesColor) -> StatsChartData {
        var data = StatsChartData(bins: bins, series: [StatsSeries(id: "cards", name: text.cardsSeries, color: color, mark: .bar,
                                                                   values: values.map { $0 })])
        data.browsable = IndexSet(values.indices.filter { values[$0] > 0 })
        return data
    }

    /// A box's chart with its series and no values (S10.3, S8.5).
    func skeleton(_ id: StatsBoxID, selection: StatsPageSelection) -> StatsChartData {
        switch id {
        case .knowledgeOverTime: return knowledgeData(axis: nil, known: [], studied: [])
        case .retention:
            return retentionData(axis: nil, values: [], open: [], desired: 0, several: false, remembered: [], forgotten: [])
        case .forgotten: return forgottenData(axis: nil, values: [])
        case .reviewsOverTime:
            let series = selection.reviewsMode == .byAnswer ? reviewsAnswerSeries([]) : reviewsTypeSeries([], filtered: nil)
            return StatsChartData(bins: [], series: series, unit: selection.reviewsMeasure == .time ? .duration : .count)
        case .activity, .cardCounts, .answers, .byDeck: return StatsChartData(bins: [], series: [])
        case .cardsOverTime: return cardsData(axis: nil, levels: [])
        case .addedAndStarted: return addedData(axis: nil, added: [], started: [])
        case .reviewsDue: return reviewsDueData(axis: nil, mature: [], young: [], overdue: nil, byThen: [])
        case .timeOfDay: return timeOfDayData(totals: [], shares: []).0
        case .retrievability: return memoryData(bins: [], values: [], color: .remembered)
        case .stability, .difficulty: return memoryData(bins: [], values: [], color: .measure)
        }
    }

    /// A box's small figures' labels with no values: they show in every phase (N25.4).
    func skeletonFigures(_ id: StatsBoxID, selection: StatsPageSelection) -> [StatsSmallFigure] {
        let labels: [String] = switch id {
        case .knowledgeOverTime: [text.figureStudied, text.figureLikelyForgotten, text.figureChangeInPeriod]
        case .retention: [text.figureRemembered, text.figureForgotten, text.figureYoung, text.figureMature]
        case .forgotten: [text.figureYoung, text.figureMature]
        case .reviewsOverTime:
            [text.figureDaysStudied, text.figureAverageADay, text.figureAverageOnDaysStudied]
                + (selection.reviewsMeasure == .time ? [text.figureTimePerCard] : [])
        // Nothing under Activity's grid (Option B): its headline carries the streaks.
        case .activity: []
        case .cardsOverTime: [text.figureNew, text.figureLearning, text.figureYoung, text.figureMature]
        case .cardCounts: [text.figureMature]
        case .addedAndStarted: [text.figureAddedADay, text.figureStartedADay]
        case .reviewsDue: [text.figureOverdue, text.figureDueTomorrow, text.figureNext30Days, text.figureDailyLoad]
        case .answers: [text.againShare(text.learning), text.againShare(text.young), text.againShare(text.mature)]
        case .timeOfDay: [text.figureBestHour, text.figureBeforeNoon]
        case .retrievability: [text.figureBelowDesired, text.figureWithMemoryState]
        case .stability: [text.figureMedianInterval, text.figureOverOneYear]
        case .difficulty: [text.figureTopTenth]
        case .byDeck: []
        }
        return labels.map { StatsSmallFigure(label: $0, value: nil) }
    }
}

// MARK: - The strip (S6)

extension StatsPresenter {
    /// One strip figure from this set; nil when the set lacks its part.
    func strip(_ id: StatsStripID) -> StatsStripFigure? {
        switch id {
        case .studiedToday:
            guard case let .studiedToday(value)? = set.value(.studiedToday) else { return nil }
            guard value.cards > 0 else { return StatsStripFigure(value: text.noneYet, unit: nil, detail: nil, loading: false, spoken: text.noneYet) }
            let time = format.duration(seconds: Double(value.timeMilliseconds) / 1_000)
            return StatsStripFigure(value: count(value.cards), unit: text.cardsUnit(value.cards), detail: time, loading: false,
                                    spoken: text.spoken(cards(value.cards), time))
        case .retentionToday:
            guard case let .retention(value)? = set.value(.retention) else { return nil }
            let today = value.today
            let rate = retention(passed: today.youngPassed + today.maturePassed, failed: today.youngFailed + today.matureFailed)
            let shown = rate.map(percent) ?? text.noValue
            return StatsStripFigure(value: shown, unit: nil, detail: nil, loading: false, spoken: shown)
        case .remainingToday:
            guard let remaining = decks.remaining(set.scope) else { return nil }
            var tomorrow: String?
            if case let .futureDue(value)? = set.value(.futureDue) { tomorrow = text.tomorrow(count(value.dueTomorrow)) }
            let shown = remaining > 0 ? count(remaining) : text.none
            return StatsStripFigure(value: shown, unit: nil, detail: tomorrow, loading: false,
                                    spoken: tomorrow.map { text.spoken(shown, $0) } ?? shown)
        case .streak:
            guard set.value(.activityDays) != nil, let current = streaks?.current else { return nil }
            return StatsStripFigure(value: count(current), unit: text.daysUnit(current), detail: nil, loading: false,
                                    spoken: text.days(current, count(current)))
        case .mature:
            guard case let .cardCounts(value)? = set.value(.cardCounts) else { return nil }
            let counts = value.excludingInactive
            let total = counts.new + counts.learning + counts.relearning + counts.young + counts.mature + counts.suspended + counts.buried
            let detail = total > 0 ? text.ofTotal(count(total)) : nil
            return StatsStripFigure(value: count(counts.mature), unit: nil, detail: detail, loading: false,
                                    spoken: detail.map { text.spoken(count(counts.mature), $0) } ?? count(counts.mature))
        }
    }

    /// Card Counts' total for the scope (S3.1's subtitle), nil without the part.
    var cardTotal: Int? {
        guard case let .cardCounts(value)? = set.value(.cardCounts) else { return nil }
        let c = value.excludingInactive
        return c.new + c.learning + c.relearning + c.young + c.mature + c.suspended + c.buried
    }
}
