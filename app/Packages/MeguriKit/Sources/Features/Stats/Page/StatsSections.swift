import CoreBridge
import Foundation

/// The page's seven sections (`stats/statistics-page` S5), in page order; the raw value is S15's
/// `stats.section.<section>`.
public enum StatsSectionID: String, CaseIterable, Hashable, Sendable {
    case knowledge, reviews, cards, comingUp, answering, memory, decks

    /// Its boxes, in order (S5's table).
    public var boxes: [StatsBoxID] {
        switch self {
        case .knowledge: [.knowledgeOverTime, .retention, .forgotten]
        case .reviews: [.reviewsOverTime, .activity]
        case .cards: [.cardsOverTime, .cardCounts, .addedAndStarted]
        case .comingUp: [.reviewsDue]
        case .answering: [.answers, .timeOfDay]
        case .memory: [.retrievability, .stability, .difficulty]
        case .decks: [.byDeck]
        }
    }

    /// Its title (S5), for View › Go to Section and the Jump menu.
    public func title(locale: Locale = .autoupdatingCurrent) -> String {
        StatsPageText.title(self, locale: locale)
    }

    /// The box that spans its row (N25.3), if the section has one: Answering and Memory have none.
    public var lead: StatsBoxID? {
        switch self {
        case .answering, .memory: nil
        default: boxes.first
        }
    }
}

extension StatsStripID {
    /// The section a click on the figure scrolls to (S6).
    public var section: StatsSectionID {
        switch self {
        case .studiedToday, .streak: .reviews
        case .retentionToday: .knowledge
        case .remainingToday: .comingUp
        case .mature: .cards
        }
    }
}

extension StatsBoxID {
    /// The charts of the present, whose header says "Now" (S1).
    var isPresent: Bool {
        switch self {
        case .cardCounts, .retrievability, .stability, .difficulty: true
        default: false
        }
    }

    /// The footnote lines the box keeps in every phase, so it never grows when one arrives (S8.5).
    var footnoteLines: Int {
        switch self {
        case .knowledgeOverTime: 2
        case .cardsOverTime, .retrievability, .stability, .difficulty: 1
        default: 0
        }
    }
}

/// The page's own strings (S16) beside the model's (`StatsText`).
nonisolated enum StatsPageText {
    private static func text(_ value: String.LocalizationValue, locale: Locale) -> String {
        String(localized: value, bundle: .module, locale: locale)
    }

    static func title(_ section: StatsSectionID, locale: Locale) -> String {
        switch section {
        case .knowledge: text("stats.section.knowledge", locale: locale)
        case .reviews: text("stats.section.reviews", locale: locale)
        case .cards: text("stats.section.cards", locale: locale)
        case .comingUp: text("stats.section.comingUp", locale: locale)
        case .answering: text("stats.section.answering", locale: locale)
        case .memory: text("stats.section.memory", locale: locale)
        case .decks: text("stats.section.decks", locale: locale)
        }
    }

    static func question(_ section: StatsSectionID, locale: Locale) -> String {
        switch section {
        case .knowledge: text("stats.section.knowledge.question", locale: locale)
        case .reviews: text("stats.section.reviews.question", locale: locale)
        case .cards: text("stats.section.cards.question", locale: locale)
        case .comingUp: text("stats.section.comingUp.question", locale: locale)
        case .answering: text("stats.section.answering.question", locale: locale)
        case .memory: text("stats.section.memory.question", locale: locale)
        case .decks: text("stats.section.decks.question", locale: locale)
        }
    }

    static func title(_ box: StatsBoxID, locale: Locale) -> String {
        switch box {
        case .knowledgeOverTime: text("stats.box.knowledgeOverTime", locale: locale)
        case .retention: text("stats.box.retention", locale: locale)
        case .forgotten: text("stats.box.forgotten", locale: locale)
        case .reviewsOverTime: text("stats.box.reviewsOverTime", locale: locale)
        case .activity: text("stats.box.activity", locale: locale)
        case .cardsOverTime: text("stats.box.cardsOverTime", locale: locale)
        case .cardCounts: text("stats.box.cardCounts", locale: locale)
        case .addedAndStarted: text("stats.box.addedAndStarted", locale: locale)
        case .reviewsDue: text("stats.box.reviewsDue", locale: locale)
        case .answers: text("stats.box.answers", locale: locale)
        case .timeOfDay: text("stats.box.timeOfDay", locale: locale)
        case .retrievability: text("stats.box.retrievability", locale: locale)
        case .stability: text("stats.box.stability", locale: locale)
        case .difficulty: text("stats.box.difficulty", locale: locale)
        case .byDeck: text("stats.box.byDeck", locale: locale)
        }
    }

    static func label(_ figure: StatsStripID, locale: Locale) -> String {
        switch figure {
        case .studiedToday: text("stats.strip.studiedToday", locale: locale)
        case .retentionToday: text("stats.strip.retentionToday", locale: locale)
        case .remainingToday: text("stats.strip.remainingToday", locale: locale)
        case .streak: text("stats.strip.streak", locale: locale)
        case .mature: text("stats.strip.mature", locale: locale)
        }
    }

    /// The period control's short names and their accessibility labels (S3.2).
    static func name(_ period: StatsPagePeriod, locale: Locale) -> String {
        switch period {
        case .oneMonth: text("stats.period.1m", locale: locale)
        case .threeMonths: text("stats.period.3m", locale: locale)
        case .sixMonths: text("stats.period.6m", locale: locale)
        case .oneYear: text("stats.period.1y", locale: locale)
        case .all: text("stats.period.all", locale: locale)
        }
    }

    static func spoken(_ period: StatsPagePeriod, locale: Locale) -> String {
        switch period {
        case .oneMonth: text("stats.period.1m.spoken", locale: locale)
        case .threeMonths: text("stats.period.3m.spoken", locale: locale)
        case .sixMonths: text("stats.period.6m.spoken", locale: locale)
        case .oneYear: text("stats.period.1y.spoken", locale: locale)
        case .all: text("stats.period.all.spoken", locale: locale)
        }
    }

    /// The identifier part of a period, `stats.period.<part>`.
    static func identifier(_ period: StatsPagePeriod) -> String {
        switch period {
        case .oneMonth: "1m"
        case .threeMonths: "3m"
        case .sixMonths: "6m"
        case .oneYear: "1y"
        case .all: "all"
        }
    }
}
