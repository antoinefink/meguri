import CoreBridge
import Foundation

/// What a token filters on (`search/filter-tokens` T1, T3): every value of
/// one kind shares one token (T2). A field is one kind per field name.
public nonisolated enum FilterKind: Hashable, Sendable {
    case deck, tag, notetype, cardType, state, flag, due, added, edited, studied
    case field(String)
    case duplicates, studiedOn
}

extension FilterValue {
    /// T3's kind of this value.
    public nonisolated var kind: FilterKind {
        switch self {
        case .deck, .currentDeck: .deck
        case .tag, .untagged: .tag
        case .notetype: .notetype
        case .cardType: .cardType
        case .state: .state
        case .flag, .noFlag, .anyFlag: .flag
        case .due: .due
        case .added: .added
        case .edited: .edited
        case .studied: .studied
        case .field(let name, _): .field(name)
        case .duplicates: .duplicates
        case .studiedOn: .studiedOn
        }
    }

    /// Whether two values are the same thing: a deck or a note type by its
    /// id (its name or subdecks may have been read at another time), any
    /// other value as it is.
    public nonisolated func isSameValue(_ other: FilterValue) -> Bool {
        switch (self, other) {
        case let (.deck(a, _, _), .deck(b, _, _)): a == b
        case let (.notetype(a, _), .notetype(b, _)): a == b
        default: self == other
        }
    }
}

extension FilterToken {
    /// The kind of a value token; nil for an advanced token.
    public nonisolated var kind: FilterKind? {
        guard case .value(let values, _) = self else { return nil }
        return values.first?.kind
    }
}

/// T9: what a click on a sidebar row of value `v` does to the query. Each
/// input changes only the token of `v`'s kind; every other token and the
/// text stay (T2: a click never adds a second token of a kind).
extension FilterQuery {
    private nonisolated func tokenIndex(_ kind: FilterKind) -> Int? {
        tokens.firstIndex { $0.kind == kind }
    }

    /// The query with `kind`'s token replaced by `token` where it stood, or
    /// `token` after the others when there was none; nil removes it.
    private nonisolated func setting(_ kind: FilterKind, to token: FilterToken?) -> FilterQuery {
        var query = self
        if let index = tokenIndex(kind) {
            if let token { query.tokens[index] = token } else { query.tokens.remove(at: index) }
        } else if let token {
            query.tokens.append(token)
        }
        return query
    }

    /// A click (or Return, or a tap): K's token becomes is {v}.
    public nonisolated func clicking(_ value: FilterValue) -> FilterQuery {
        setting(value.kind, to: .value([value], negated: false))
    }

    /// ⌘-click: v is added to K's token when absent and removed when present
    /// (a token left with no value is removed); with no K token, is {v} is
    /// made. The token's is / is not stays.
    public nonisolated func commandClicking(_ value: FilterValue) -> FilterQuery {
        guard let index = tokenIndex(value.kind), case .value(var values, let negated) = tokens[index] else {
            return setting(value.kind, to: .value([value], negated: false))
        }
        if let present = values.firstIndex(where: { $0.isSameValue(value) }) {
            values.remove(at: present)
        } else {
            values.append(value)
        }
        return setting(value.kind, to: values.isEmpty ? nil : .value(values, negated: negated))
    }

    /// ⌥-click: K's token becomes is not {v}. No Flag becomes is {Any Flag}
    /// and Any Flag is {No Flag} (T10's normal form); Untagged, which has no
    /// opposite value, acts as a click.
    public nonisolated func optionClicking(_ value: FilterValue) -> FilterQuery {
        switch value {
        case .noFlag: setting(.flag, to: .value([.anyFlag], negated: false))
        case .anyFlag: setting(.flag, to: .value([.noFlag], negated: false))
        case .untagged: clicking(value)
        default: setting(value.kind, to: .value([value], negated: true))
        }
    }

    /// T9: a row shows selected exactly when its value is in its kind's
    /// token and that token is **is**.
    public nonisolated func selects(_ value: FilterValue) -> Bool {
        tokens.contains { token in
            guard case .value(let values, false) = token, token.kind == value.kind else { return false }
            return values.contains { $0.isSameValue(value) }
        }
    }
}

/// The built-in saved searches' queries (T13), which never need a reading.
extension FilterBuiltIn {
    public nonisolated var value: FilterValue {
        switch self {
        case .dueToday: .due(.today)
        case .addedInLast7Days: .added(.last7Days)
        case .leeches: .state(.leech)
        }
    }

    public nonisolated var query: FilterQuery {
        FilterQuery(tokens: [.value([value], negated: false)])
    }
}

/// The Browse sidebar's sections after Today and Browse, in order
/// (`browser/workspace` V6).
public nonisolated enum BrowseSidebarSection: String, CaseIterable, Codable, Hashable, Sendable {
    case savedSearches, cardState, flags, decks, notetypes, tags

    /// Flags and Tags start folded (V6).
    public static let foldedByDefault: Set<BrowseSidebarSection> = [.flags, .tags]
}

/// A row of the sidebar while Browse is selected: the two section rows at
/// the top, which navigate, then the filter rows of V6, which are views of
/// the query (T9). It is the list's selection tag.
public nonisolated enum BrowseSidebarTag: Hashable, Sendable {
    case section(AppSection)
    case builtIn(FilterBuiltIn)
    /// A user saved search, by its name (`browser/config-keys` K7.1).
    case saved(String)
    case state(FilterState)
    /// No Flag (0), then flags 1 to 7.
    case flag(UInt8)
    case deck(Int64)
    case notetype(Int64)
    /// A tag by its full name.
    case tag(String)
    /// "Show All <N>…" under the Tags section (V6).
    case showAllTags
}
