import AppModel
import CoreBridge
import Foundation
import SwiftUI

/// Browse's strings (`browser/workspace` V16, `search/filter-tokens` T15),
/// from its String Catalog.
public enum BrowseStrings {
    static var bundle: Bundle { .module }

    static func text(_ key: String.LocalizationValue, locale: Locale = .current) -> String {
        String(localized: key, bundle: .module, locale: locale)
    }

    static func section(_ section: BrowseSidebarSection, locale: Locale = .current) -> String {
        switch section {
        case .savedSearches: text("browse.section.savedSearches", locale: locale)
        case .cardState: text("browse.section.cardState", locale: locale)
        case .flags: text("browse.section.flags", locale: locale)
        case .decks: text("browse.section.decks", locale: locale)
        case .notetypes: text("browse.section.notetypes", locale: locale)
        case .tags: text("browse.section.tags", locale: locale)
        }
    }

    static func state(_ state: FilterState, locale: Locale = .current) -> String {
        switch state {
        case .new: text("browse.state.new", locale: locale)
        case .learning: text("browse.state.learning", locale: locale)
        case .review: text("browse.state.review", locale: locale)
        case .suspended: text("browse.state.suspended", locale: locale)
        case .buried: text("browse.state.buried", locale: locale)
        case .leech: text("browse.saved.leeches", locale: locale)
        }
    }

    static func builtIn(_ search: FilterBuiltIn, locale: Locale = .current) -> String {
        switch search {
        case .dueToday: text("browse.saved.dueToday", locale: locale)
        case .addedInLast7Days: text("browse.saved.addedInLast7Days", locale: locale)
        case .leeches: text("browse.saved.leeches", locale: locale)
        }
    }

    /// "Show All 46…" (V6).
    static func showAll(_ count: Int, locale: Locale = .current) -> String {
        let number = count.formatted(.number.locale(locale))
        return String(localized: "browse.tags.showAll", defaultValue: "\(number)", bundle: .module, locale: locale)
    }

    /// A row's count as VoiceOver reads it, with its plural.
    static func count(_ count: UInt64, mode: BrowseMode, locale: Locale = .current) -> String {
        let text = count.formatted(.number.locale(locale)), n = Int(clamping: count)
        return switch mode {
        case .cards: String(localized: "browse.count.cards", defaultValue: "\(n) \(text)", bundle: .module, locale: locale)
        case .notes: String(localized: "browse.count.notes", defaultValue: "\(n) \(text)", bundle: .module, locale: locale)
        }
    }
}
