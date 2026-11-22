import AppModel
import CoreBridge
import Foundation

/// The search field's strings (`search/filter-tokens` T15): kind and value
/// labels, the tokens' text and the suggestions' labels.
extension BrowseStrings {
    /// A kind's label (T15), the field's name for a Field kind.
    static func kind(_ kind: FilterKind, locale: Locale = .current) -> String {
        switch kind {
        case .deck: text("browse.kind.deck", locale: locale)
        case .tag: text("browse.kind.tag", locale: locale)
        case .notetype: text("browse.kind.notetype", locale: locale)
        case .cardType: text("browse.kind.cardType", locale: locale)
        case .state: text("browse.section.cardState", locale: locale)
        case .flag: text("browse.kind.flag", locale: locale)
        case .due: text("browse.kind.due", locale: locale)
        case .added: text("browse.kind.added", locale: locale)
        case .edited: text("browse.kind.edited", locale: locale)
        case .studied: text("browse.kind.studied", locale: locale)
        case .field(let name): name
        case .duplicates: text("browse.kind.duplicates", locale: locale)
        case .studiedOn: text("browse.kind.studiedOn", locale: locale)
        }
    }

    /// A suggestion section's label (T15): kind labels, plural for the
    /// name sections.
    static func section(_ section: FilterSection, locale: Locale = .current) -> String {
        switch section {
        case .savedSearches: text("browse.section.savedSearches", locale: locale)
        case .cardState: text("browse.section.cardState", locale: locale)
        case .flag: text("browse.kind.flag", locale: locale)
        case .due: text("browse.kind.due", locale: locale)
        case .added: text("browse.kind.added", locale: locale)
        case .edited: text("browse.kind.edited", locale: locale)
        case .studied: text("browse.kind.studied", locale: locale)
        case .decks: text("browse.section.decks", locale: locale)
        case .tags: text("browse.section.tags", locale: locale)
        case .notetypes: text("browse.section.notetypes", locale: locale)
        case .cardTypes: text("browse.section.cardTypes", locale: locale)
        case .fields: text("browse.section.fields", locale: locale)
        case .filterBy: text("browse.section.filterBy", locale: locale)
        }
    }

    /// A Filter By row: the kind its value list holds (T12.1).
    static func filterBy(_ section: FilterSection, locale: Locale = .current) -> String {
        switch section {
        case .decks: text("browse.kind.deck", locale: locale)
        case .tags: text("browse.kind.tag", locale: locale)
        case .notetypes: text("browse.kind.notetype", locale: locale)
        case .cardTypes: text("browse.kind.cardType", locale: locale)
        case .fields: text("browse.kind.field", locale: locale)
        default: Self.section(section, locale: locale)
        }
    }

    static func period(_ period: FilterPeriod, locale: Locale = .current) -> String {
        switch period {
        case .today: text("browse.value.today", locale: locale)
        case .last7Days: text("browse.value.last7Days", locale: locale)
        case .last30Days: text("browse.value.last30Days", locale: locale)
        }
    }

    /// A value's label (T3, T15); flags by their shown names (`flagNames`,
    /// F4). A deck shows its last name component.
    static func value(_ value: FilterValue, flagNames: [String], locale: Locale = .current) -> String {
        switch value {
        case .deck:
            let name = value.deckName ?? ""
            return name.components(separatedBy: "::").last ?? name
        case .currentDeck: return text("browse.value.currentDeck", locale: locale)
        case .tag(let name): return name
        case .untagged: return text("browse.value.untagged", locale: locale)
        case .notetype(_, let name): return name
        case .cardType(let name): return name
        case .state(.leech): return text("browse.value.leech", locale: locale)
        case .state(let state): return Self.state(state, locale: locale)
        case .flag(let flag): return flagNames.indices.contains(Int(flag) - 1) ? flagNames[Int(flag) - 1] : flagLabel(flag, locale: locale)
        case .noFlag: return text("browse.flag.none", locale: locale)
        case .anyFlag: return text("browse.value.anyFlag", locale: locale)
        case .due(.today): return text("browse.value.today", locale: locale)
        case .due(.overdue): return text("browse.value.overdue", locale: locale)
        case .due(.next7Days): return text("browse.value.next7Days", locale: locale)
        case .added(let period), .edited(let period), .studied(let period): return Self.period(period, locale: locale)
        case .field(_, .contains(let word)):
            return String(localized: "browse.token.contains", defaultValue: "\(word)", bundle: bundle, locale: locale)
        case .field(_, .empty): return text("browse.token.isEmpty", locale: locale)
        case .field(_, .hasContent): return text("browse.token.hasContent", locale: locale)
        case .duplicates(let ids): return ids.count.formatted(.number.locale(locale))
        case .studiedOn(let ids): return ids.count.formatted(.number.locale(locale))
        }
    }

    /// A deck value's path, joined with " › " (T7: its help tag and
    /// accessibility label); nil for other values.
    static func path(_ value: FilterValue) -> String? {
        value.deckName.map { $0.components(separatedBy: "::").joined(separator: " › ") }
    }

    /// A token's values joined with "or" (T15: "alpha or beta").
    static func values(_ values: [FilterValue], flagNames: [String], locale: Locale = .current) -> String {
        let or = " " + text("browse.token.or", locale: locale) + " "
        return values.map { value($0, flagNames: flagNames, locale: locale) }.joined(separator: or)
    }

    /// The kind label with "is not" after it for a negated token (T15).
    static func kindLabel(_ kind: FilterKind, negated: Bool, locale: Locale = .current) -> String {
        let label = Self.kind(kind, locale: locale)
        return negated ? label + " " + text("browse.token.isNot", locale: locale) : label
    }

    /// The whole token as VoiceOver reads it: the kind, is not, the values
    /// (a deck by its path); an advanced token's text.
    static func accessibility(_ token: FilterToken, flagNames: [String], locale: Locale = .current) -> String {
        switch token {
        case .advanced(let text): return text
        case .value(let values, let negated):
            guard let kind = token.kind else { return "" }
            let or = " " + text("browse.token.or", locale: locale) + " "
            let shown = values.map { path($0) ?? value($0, flagNames: flagNames, locale: locale) }.joined(separator: or)
            return kindLabel(kind, negated: negated, locale: locale) + " " + shown
        }
    }

    /// "Contains “w”" (T12.1's first row).
    static func containsWord(_ word: String, locale: Locale = .current) -> String {
        String(localized: "browse.search.containsWord", defaultValue: "\(word)", bundle: bundle, locale: locale)
    }

    /// "Show All <section>" (T12.3 step 3).
    static func showAllSection(_ section: FilterSection, locale: Locale = .current) -> String {
        let label = Self.section(section, locale: locale)
        return String(localized: "browse.search.showAll", defaultValue: "\(label)", bundle: bundle, locale: locale)
    }

    /// "Add Another tag…" (T10; the kind's label in lower case).
    static func addAnother(_ kind: FilterKind, locale: Locale = .current) -> String {
        let label = Self.kind(kind, locale: locale).lowercased(with: locale)
        return String(localized: "browse.token.addAnother", defaultValue: "\(label)", bundle: bundle, locale: locale)
    }

    /// A Field or Card Type suggestion's detail: "in <note type>" or "in <n>
    /// note types" (T12.1).
    static func notetypesDetail(_ ids: [Int64], names: [NotetypeName], locale: Locale = .current) -> String? {
        guard !ids.isEmpty else { return nil }
        if ids.count == 1, let name = names.first(where: { $0.id == ids[0] })?.name {
            return String(localized: "browse.search.inNotetype", defaultValue: "\(name)", bundle: bundle, locale: locale)
        }
        let count = ids.count
        return String(localized: "browse.search.inNotetypes", defaultValue: "\(count)", bundle: bundle, locale: locale)
    }

    /// The replace question of Save Search (T13).
    static func replaceSavedSearch(_ name: String, locale: Locale = .current) -> String {
        String(localized: "browse.saveSearch.replace.message", defaultValue: "\(name)", bundle: bundle, locale: locale)
    }
}
