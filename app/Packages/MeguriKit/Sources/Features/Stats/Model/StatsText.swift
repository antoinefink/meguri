import Foundation

/// Every text the page model writes (`stats/statistics-page` S8.7, S16, S7's figures), from
/// the String Catalog. Numbers arrive already written by the system's formatters (`StatsChartFormat`);
/// a count that picks a plural form is passed beside its text.
struct StatsText {
    let locale: Locale

    private func text(_ value: String.LocalizationValue) -> String {
        String(localized: value, bundle: .module, locale: locale)
    }

    // MARK: Subtitle (S10.4)

    func cardsInDecks(_ cards: Int, _ cardsText: String, _ decks: Int, _ decksText: String) -> String {
        text("stats.subtitle.cardsInDecks \(cards) \(cardsText) \(decks) \(decksText)")
    }
    var updating: String { text("stats.subtitle.updating") }
    var updatedJustNow: String { text("stats.subtitle.updatedJustNow") }
    func updated(_ time: String) -> String { text("stats.subtitle.updated \(time)") }
    func withStatus(_ subtitle: String, _ status: String) -> String { text("stats.subtitle.withStatus \(subtitle) \(status)") }

    // MARK: Units and shared values

    func cards(_ n: Int, _ nText: String) -> String { text("stats.value.cards \(n) \(nText)") }
    func cardsUnit(_ n: Int) -> String { text("stats.unit.cards \(n)") }
    func days(_ n: Int, _ nText: String) -> String { text("stats.value.days \(n) \(nText)") }
    func daysUnit(_ n: Int) -> String { text("stats.unit.days \(n)") }
    func reviews(_ n: Int, _ nText: String) -> String { text("stats.value.reviews \(n) \(nText)") }
    func ofTotal(_ total: String) -> String { text("stats.value.ofTotal \(total)") }
    func withChange(_ value: String, _ change: String) -> String { text("stats.value.withChange \(value) \(change)") }
    func withShare(_ value: String, _ share: String) -> String { text("stats.value.withShare \(value) \(share)") }
    func spoken(_ first: String, _ second: String) -> String { text("stats.value.spoken \(first) \(second)") }
    var none: String { text("stats.value.none") }
    var noneYet: String { text("stats.value.noneYet") }
    var noValue: String { text("stats.value.noValue") }
    func tomorrow(_ n: String) -> String { text("stats.value.tomorrow \(n)") }
    func range(_ low: String, _ high: String) -> String { text("stats.value.range \(low) \(high)") }

    // MARK: Series and states

    var known: String { text("stats.series.known") }
    var studied: String { text("stats.series.studied") }
    var retention: String { text("stats.series.retention") }
    var desiredRetention: String { text("stats.series.desiredRetention") }
    var averageDesiredRetention: String { text("stats.series.averageDesiredRetention") }
    var remembered: String { text("stats.series.remembered") }
    var forgotten: String { text("stats.series.forgotten") }
    var new: String { text("stats.series.new") }
    var learning: String { text("stats.series.learning") }
    var relearning: String { text("stats.series.relearning") }
    var young: String { text("stats.series.young") }
    var mature: String { text("stats.series.mature") }
    var suspended: String { text("stats.series.suspended") }
    var buried: String { text("stats.series.buried") }
    var filtered: String { text("stats.series.filtered") }
    var again: String { text("stats.series.again") }
    var hard: String { text("stats.series.hard") }
    var good: String { text("stats.series.good") }
    var easy: String { text("stats.series.easy") }
    var added: String { text("stats.series.added") }
    var started: String { text("stats.series.started") }
    var overdue: String { text("stats.series.overdue") }
    var totalByThen: String { text("stats.series.totalByThen") }
    var reviewsSeries: String { text("stats.series.reviews") }
    var total: String { text("stats.series.total") }
    var cardsSeries: String { text("stats.series.cards") }
    var likelyForgotten: String { text("stats.series.likelyForgotten") }
    var fillsIn: String { text("stats.empty.fillsIn") }
    var noSubdecks: String { text("stats.empty.noSubdecks") }

    // MARK: Knowledge Over Time (S7.1)

    func knowledgeSentence(_ share: String, _ studied: Int, _ studiedText: String) -> String {
        text("stats.knowledge.sentence \(share) \(studied) \(studiedText)")
    }
    func up(_ sentence: String, _ n: String, _ date: String) -> String { text("stats.change.up \(sentence) \(n) \(date)") }
    func down(_ sentence: String, _ n: String, _ date: String) -> String { text("stats.change.down \(sentence) \(n) \(date)") }
    var figureStudied: String { text("stats.figure.studied") }
    var figureLikelyForgotten: String { text("stats.figure.likelyForgotten") }
    var figureChangeInPeriod: String { text("stats.figure.changeInPeriod") }
    func suspendedNotIncluded(_ n: Int, _ nText: String) -> String { text("stats.footnote.suspendedNotIncluded \(n) \(nText)") }
    func couldNotEstimate(_ n: Int, _ nText: String) -> String { text("stats.footnote.couldNotEstimate \(n) \(nText)") }
    func suspendedNotShown(_ n: Int, _ nText: String) -> String { text("stats.footnote.suspendedNotShown \(n) \(nText)") }
    func withoutMemoryState(_ n: Int, _ nText: String) -> String { text("stats.footnote.withoutMemoryState \(n) \(nText)") }
    func footnotes(_ first: String, _ second: String) -> String { text("stats.footnote.both \(first) \(second)") }

    // MARK: Retention and Forgotten (S7.2, S7.3)

    func desiredSentence(_ value: String) -> String { text("stats.retention.desired \(value)") }
    func averageDesiredSentence(_ value: String) -> String { text("stats.retention.averageDesired \(value)") }
    var figureRemembered: String { text("stats.figure.remembered") }
    var figureForgotten: String { text("stats.figure.forgotten") }
    var figureYoung: String { text("stats.figure.young") }
    var figureMature: String { text("stats.figure.mature") }
    func perDay(_ n: String) -> String { text("stats.forgotten.perDay \(n)") }

    // MARK: Reviews Over Time and Activity (S7.4, S7.5)

    func daysInARow(_ n: Int, _ nText: String) -> String { text("stats.reviews.daysInARow \(n) \(nText)") }
    func daysStudied(_ n: Int, _ nText: String) -> String { text("stats.reviews.daysStudied \(n) \(nText)") }
    func aOfB(_ a: String, _ b: String) -> String { text("stats.value.aOfB \(a) \(b)") }
    var figureDaysStudied: String { text("stats.figure.daysStudied") }
    var figureAverageADay: String { text("stats.figure.averageADay") }
    var figureAverageOnDaysStudied: String { text("stats.figure.averageOnDaysStudied") }
    var figureTimePerCard: String { text("stats.figure.timePerCard") }
    var figureCurrentStreak: String { text("stats.figure.currentStreak") }
    var figureLongestStreak: String { text("stats.figure.longestStreak") }
    var figureDaysStudiedThisYear: String { text("stats.figure.daysStudiedThisYear") }
    var figureAllReviews: String { text("stats.figure.allReviews") }
    func yearTotal(_ year: String, _ n: Int, _ nText: String) -> String { text("stats.activity.yearTotal \(year) \(n) \(nText)") }

    // MARK: Cards (S7.6 to S7.8)

    func matureUp(_ mature: String, _ n: String) -> String { text("stats.cards.matureUp \(mature) \(n)") }
    func matureDown(_ mature: String, _ n: String) -> String { text("stats.cards.matureDown \(mature) \(n)") }
    var figureNew: String { text("stats.figure.new") }
    var figureLearning: String { text("stats.figure.learning") }
    func cardsInNotes(_ cards: Int, _ cardsText: String, _ notes: Int, _ notesText: String) -> String {
        text("stats.cardCounts.figure \(cards) \(cardsText) \(notes) \(notesText)")
    }
    var figureTotal: String { text("stats.figure.total") }
    var figureNotes: String { text("stats.figure.notes") }
    func startedSentence(_ n: String) -> String { text("stats.added.started \(n)") }
    var figureAddedADay: String { text("stats.figure.addedADay") }
    var figureStartedADay: String { text("stats.figure.startedADay") }

    // MARK: Coming Up and Answering (S7.9 to S7.11)

    func remainingTomorrow(_ remaining: String, _ tomorrow: String) -> String {
        text("stats.reviewsDue.sentence \(remaining) \(tomorrow)")
    }
    func onlyTomorrow(_ tomorrow: String) -> String { text("stats.reviewsDue.tomorrow \(tomorrow)") }
    var figureOverdue: String { text("stats.figure.overdue") }
    var figureDueTomorrow: String { text("stats.figure.dueTomorrow") }
    var figureNext30Days: String { text("stats.figure.next30Days") }
    var figureDailyLoad: String { text("stats.figure.dailyLoad") }
    func shareOfButton(_ share: String, _ button: String) -> String { text("stats.answers.sentence \(share) \(button)") }
    func againShare(_ group: String) -> String { text("stats.answers.againShare \(group)") }
    var busiestHour: String { text("stats.timeOfDay.busiest") }
    var figureBestHour: String { text("stats.figure.bestHour") }
    var figureBeforeNoon: String { text("stats.figure.beforeNoon") }

    // MARK: Memory and Decks (S7.12 to S7.15)

    func totalKnowledge(_ cards: String, _ notes: String) -> String { text("stats.retrievability.sentence \(cards) \(notes)") }
    var figureBelowDesired: String { text("stats.figure.belowDesired") }
    var figureWithMemoryState: String { text("stats.figure.withMemoryState") }
    var medianStability: String { text("stats.stability.median") }
    var figureMedianInterval: String { text("stats.figure.medianInterval") }
    var figureOverOneYear: String { text("stats.figure.overOneYear") }
    var medianDifficulty: String { text("stats.difficulty.median") }
    var figureTopTenth: String { text("stats.figure.topTenth") }
    func timeIn(_ deck: String) -> String { text("stats.byDeck.sentence \(deck)") }
    var filteredDeckNote: String { text("stats.page.filteredDeck") }

    /// Stability's doubling bins (S7.13), from under 1 day to over 2 years.
    var stabilityBins: [String] {
        [text("stats.stability.bin0"), text("stats.stability.bin1"), text("stats.stability.bin2"),
         text("stats.stability.bin3"), text("stats.stability.bin4"), text("stats.stability.bin5"),
         text("stats.stability.bin6"), text("stats.stability.bin7"), text("stats.stability.bin8"),
         text("stats.stability.bin9")]
    }
}
