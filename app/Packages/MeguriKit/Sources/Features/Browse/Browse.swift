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

    // MARK: The table (U270a)

    /// A column's header: `browser/columns` C2's label in the mode; a field
    /// column is its field's name (V3).
    static func column(_ key: String, mode: BrowseMode, locale: Locale = .current) -> String {
        if key.hasPrefix("fld:") { return String(key.dropFirst(4)) }
        switch (key, mode) {
        case ("template", .notes): return text("browse.column.template.notes", locale: locale)
        case ("cardEase", .notes): return text("browse.column.cardEase.notes", locale: locale)
        case ("cardIvl", .notes): return text("browse.column.cardIvl.notes", locale: locale)
        default: break
        }
        switch key {
        case "noteFld": return text("browse.column.noteFld", locale: locale)
        case "question": return text("browse.column.question", locale: locale)
        case "answer": return text("browse.column.answer", locale: locale)
        case "template": return text("browse.column.template", locale: locale)
        case "deck": return text("browse.column.deck", locale: locale)
        case "cardDue": return text("browse.column.cardDue", locale: locale)
        case "cardEase": return text("browse.column.cardEase", locale: locale)
        case "cardIvl": return text("browse.column.cardIvl", locale: locale)
        case "cardReps": return text("browse.column.cardReps", locale: locale)
        case "cardLapses": return text("browse.column.cardLapses", locale: locale)
        case "noteCrt": return text("browse.column.noteCrt", locale: locale)
        case "cardMod": return text("browse.column.cardMod", locale: locale)
        case "noteMod": return text("browse.column.noteMod", locale: locale)
        case "note": return text("browse.column.note", locale: locale)
        case "noteTags": return text("browse.column.noteTags", locale: locale)
        case "originalPosition": return text("browse.column.originalPosition", locale: locale)
        case "stability": return text("browse.column.stability", locale: locale)
        case "difficulty": return text("browse.column.difficulty", locale: locale)
        case "retrievability": return text("browse.column.retrievability", locale: locale)
        default: return key
        }
    }

    /// The flags' colours (`tags/flags-and-marked` F1): the system's.
    static func flagColor(_ flag: UInt8) -> Color {
        switch flag {
        case 1: .red
        case 2: .orange
        case 3: .green
        case 4: .blue
        case 5: .pink
        case 6: .teal
        default: .purple
        }
    }

    /// V2.5: a flag's accessibility label, by its colour ("Red Flag").
    static func flagLabel(_ flag: UInt8, locale: Locale = .current) -> String {
        switch flag {
        case 1: text("browse.flag.red", locale: locale)
        case 2: text("browse.flag.orange", locale: locale)
        case 3: text("browse.flag.green", locale: locale)
        case 4: text("browse.flag.blue", locale: locale)
        case 5: text("browse.flag.pink", locale: locale)
        case 6: text("browse.flag.turquoise", locale: locale)
        default: text("browse.flag.purple", locale: locale)
        }
    }

    /// V2.2: "<note type> Columns".
    static func ownColumns(_ name: String, locale: Locale = .current) -> String {
        String(localized: "browse.columns.own", defaultValue: "\(name)", bundle: .module, locale: locale)
    }

    /// V11: "<t> cards", "<r> of <t> cards" (singular when t is 1), then
    /// " · <s> selected"; "Searching…" before while a search runs. The first
    /// number is bold. Before the total is known, the rows alone.
    static func status(_ status: BrowseStatus, locale: Locale = .current) -> AttributedString {
        let number = { (n: Int) in n.formatted(.number.locale(locale)) }
        let shown = number(status.shown)
        var line: String
        if status.wholeCollection || status.total == nil {
            line = count(UInt64(status.shown), mode: status.mode, locale: locale)
        } else {
            let total = Int(clamping: status.total ?? 0), all = number(total)
            line = switch status.mode {
            case .cards: String(localized: "browse.status.cardsOf", defaultValue: "\(total) \(shown) \(all)", bundle: .module, locale: locale)
            case .notes: String(localized: "browse.status.notesOf", defaultValue: "\(total) \(shown) \(all)", bundle: .module, locale: locale)
            }
        }
        let searching = status.searching ? text("browse.status.searching", locale: locale) + " " : ""
        if status.selected > 0 {
            line += " " + String(localized: "browse.status.selected", defaultValue: "\(number(status.selected))", bundle: .module,
                                 locale: locale)
        }
        var text = AttributedString(searching + line)
        if let range = text.range(of: shown) { text[range].inlinePresentationIntent = .stronglyEmphasized }
        return text
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
