import AppModel
import Foundation

/// N20.6 and N20.7's text for Delete Deck. Numbers use the locale's grouping.
enum DeckDeletionText {
    static func title(_ question: DeckDeletionModel.Confirmation) -> String { title(name: question.name) }
    /// The window's locale groups the counts (the launch locale can differ from the process's).
    static func message(_ question: DeckDeletionModel.Confirmation, locale: Locale = .current) -> String {
        message(subdecks: question.subdecks, cards: question.cards, locale: locale)
    }
    static func failureTitle(_ failure: DeckDeletionModel.Failure) -> String { failureTitle(name: failure.name) }
    static func failureMessage(_ failure: DeckDeletionModel.Failure) -> String { failureMessage(failure.reason) }

    static func title(name: String, locale: Locale = .current) -> String {
        String(localized: "Delete “\(name)”?", bundle: .module, locale: locale)
    }

    /// One String Catalog entry varies by both counts (N20.7's six rows).
    static func message(subdecks: UInt64, cards: UInt64, locale: Locale = .current) -> String {
        let subdecks = Int(clamping: subdecks), cards = Int(clamping: cards)
        return String(localized: "Its \(subdecks) subdecks and \(cards) cards will also be deleted.",
            bundle: .module, locale: locale)
    }

    static func failureTitle(name: String, locale: Locale = .current) -> String {
        String(localized: "Couldn’t Delete “\(name)”", bundle: .module, locale: locale)
    }

    static func failureMessage(_ reason: DeckDeletionModel.Failure.Reason, locale: Locale = .current) -> String {
        switch reason {
        case .backupFailed:
            String(localized: "Meguri couldn’t make a backup first, so nothing was deleted.", bundle: .module, locale: locale)
        case .other:
            String(localized: "Nothing was deleted.", bundle: .module, locale: locale)
        }
    }

    static var deleteDeck: String { String(localized: "Delete Deck", bundle: .module) }
    /// The Mac alert's destructive button.
    static var delete: String { String(localized: "Delete", bundle: .module) }
    static var cancel: String { String(localized: "Cancel", bundle: .module) }
    static var ok: String { String(localized: "OK", bundle: .module) }
}
