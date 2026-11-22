import Foundation
import Instrumentation
internal import MeguriCoreBindings

// Browse's query model and reads (U264a): the filter tokens query
// (`search/filter-tokens` T1 to T7) with its run text (T4), T7's reading
// and T8's commit, D5's incomplete text (T8), the suggestions (T12), the
// filter counts (T12.4) and the counts of a selection (`browser/workspace`
// V7, V8.1) as jobs, and the column sets (V2.2 to V2.4). The query and
// suggestion calls read only the core's name index; they run on the
// interactive lane, once per keystroke. Offsets into the field's text are
// UTF-16 offsets (`NSRange`, `String.UTF16View`).

/// A Card State value (T3).
public enum FilterState: Sendable, Hashable, CaseIterable {
    case new, learning, review, suspended, buried
    /// `tag:leech`.
    case leech
}

/// A Due value (T3).
public enum FilterDue: Sendable, Hashable, CaseIterable {
    case today, overdue, next7Days
}

/// The day values of Added, Edited and Studied (T3).
public enum FilterPeriod: Sendable, Hashable, CaseIterable {
    case today, last7Days, last30Days
}

/// A Field token's test (T3).
public enum FilterFieldTest: Sendable, Hashable {
    case contains(String)
    case empty
    case hasContent
}

/// One value of T3's table.
public enum FilterValue: Sendable, Hashable {
    /// A deck with its subdecks: its stored name (levels joined by U+001F)
    /// and every deck below it. Make one with `CoreBrowseQuery.deckValue`.
    case deck(id: Int64, native: String, tree: [Int64])
    case currentDeck
    /// A tag by its full name; it finds its descendants.
    case tag(String)
    case untagged
    case notetype(id: Int64, name: String)
    /// Every card type of that name in any note type.
    case cardType(String)
    case state(FilterState)
    /// Flags 1 to 7.
    case flag(UInt8)
    case noFlag
    case anyFlag
    case due(FilterDue)
    case added(FilterPeriod)
    case edited(FilterPeriod)
    case studied(FilterPeriod)
    case field(String, FilterFieldTest)
    /// Made only by Find Duplicates (V10).
    case duplicates(noteIDs: [Int64])
    /// Made only by a heatmap day (V15).
    case studiedOn(cardIDs: [Int64])

    /// A deck's name as shown, levels joined by `::`; nil for other values.
    public var deckName: String? {
        guard case .deck(_, let native, _) = self else { return nil }
        return native.replacingOccurrences(of: "\u{1F}", with: "::")
    }
}

/// One token of the field (T1).
public enum FilterToken: Sendable, Hashable {
    /// One or more values of one kind (OR), or its negation (is not).
    case value([FilterValue], negated: Bool)
    /// Anki search text no kind can show (T6), shown as written.
    case advanced(String)
}

/// What the search field holds (T1): tokens in their order, then the text.
public struct FilterQuery: Sendable, Hashable {
    public var tokens: [FilterToken]
    public var text: String
    public init(tokens: [FilterToken] = [], text: String = "") {
        self.tokens = tokens
        self.text = text
    }
    public var isEmpty: Bool { tokens.isEmpty && text.isEmpty }
}

// MARK: Conversions of the query model

extension FilterState {
    init(_ raw: MeguriCoreBindings.FilterState) {
        switch raw {
        case .new: self = .new
        case .learning: self = .learning
        case .review: self = .review
        case .suspended: self = .suspended
        case .buried: self = .buried
        case .leech: self = .leech
        }
    }
    var raw: MeguriCoreBindings.FilterState {
        switch self {
        case .new: .new
        case .learning: .learning
        case .review: .review
        case .suspended: .suspended
        case .buried: .buried
        case .leech: .leech
        }
    }
}

extension FilterDue {
    init(_ raw: MeguriCoreBindings.FilterDue) {
        switch raw {
        case .today: self = .today
        case .overdue: self = .overdue
        case .next7Days: self = .next7Days
        }
    }
    var raw: MeguriCoreBindings.FilterDue {
        switch self {
        case .today: .today
        case .overdue: .overdue
        case .next7Days: .next7Days
        }
    }
}

extension FilterPeriod {
    init(_ raw: MeguriCoreBindings.FilterPeriod) {
        switch raw {
        case .today: self = .today
        case .last7Days: self = .last7Days
        case .last30Days: self = .last30Days
        }
    }
    var raw: MeguriCoreBindings.FilterPeriod {
        switch self {
        case .today: .today
        case .last7Days: .last7Days
        case .last30Days: .last30Days
        }
    }
}

extension FilterFieldTest {
    var raw: MeguriCoreBindings.FilterFieldTest {
        switch self {
        case .contains(let text): .contains(text: text)
        case .empty: .empty
        case .hasContent: .hasContent
        }
    }
}

extension FilterValue {
    init(_ raw: MeguriCoreBindings.FilterValue) {
        switch raw {
        case .deck(let id, let native, let tree): self = .deck(id: id, native: native, tree: tree)
        case .currentDeck: self = .currentDeck
        case .tag(let name): self = .tag(name)
        case .untagged: self = .untagged
        case .notetype(let id, let name): self = .notetype(id: id, name: name)
        case .cardType(let name): self = .cardType(name)
        case .state(let state): self = .state(FilterState(state))
        case .flag(let flag): self = .flag(flag)
        case .noFlag: self = .noFlag
        case .anyFlag: self = .anyFlag
        case .due(let due): self = .due(FilterDue(due))
        case .added(let period): self = .added(FilterPeriod(period))
        case .edited(let period): self = .edited(FilterPeriod(period))
        case .studied(let period): self = .studied(FilterPeriod(period))
        case .field(let field, let test):
            let test: FilterFieldTest = switch test {
            case .contains(let text): .contains(text)
            case .empty: .empty
            case .hasContent: .hasContent
            }
            self = .field(field, test)
        case .duplicates(let ids): self = .duplicates(noteIDs: ids)
        case .studiedOn(let ids): self = .studiedOn(cardIDs: ids)
        }
    }
    var raw: MeguriCoreBindings.FilterValue {
        switch self {
        case .deck(let id, let native, let tree): .deck(id: id, native: native, tree: tree)
        case .currentDeck: .currentDeck
        case .tag(let name): .tag(name: name)
        case .untagged: .untagged
        case .notetype(let id, let name): .notetype(id: id, name: name)
        case .cardType(let name): .cardType(name: name)
        case .state(let state): .state(state: state.raw)
        case .flag(let flag): .flag(flag: flag)
        case .noFlag: .noFlag
        case .anyFlag: .anyFlag
        case .due(let due): .due(due: due.raw)
        case .added(let period): .added(period: period.raw)
        case .edited(let period): .edited(period: period.raw)
        case .studied(let period): .studied(period: period.raw)
        case .field(let field, let test): .field(field: field, test: test.raw)
        case .duplicates(let ids): .duplicates(noteIds: ids)
        case .studiedOn(let ids): .studiedOn(cardIds: ids)
        }
    }
}

extension FilterQuery {
    init(_ raw: MeguriCoreBindings.FilterQuery) {
        self.init(tokens: raw.tokens.map { token in
            switch token {
            case .value(let values, let negated): .value(values.map(FilterValue.init), negated: negated)
            case .advanced(let text): .advanced(text)
            }
        }, text: raw.text)
    }
    var raw: MeguriCoreBindings.FilterQuery {
        .init(tokens: tokens.map { token in
            switch token {
            case .value(let values, let negated): .value(values: values.map(\.raw), negated: negated)
            case .advanced(let text): .advanced(text: text)
            }
        }, text: text)
    }
}

// MARK: Suggestions

/// A section of the suggestions (T12.1, T12.3).
public enum FilterSection: Sendable, Hashable, CaseIterable {
    case savedSearches, cardState, flag, due, added, edited, studied, decks, tags, notetypes, cardTypes, fields
    /// T12.1's first row group: the kinds, each opening its value list.
    case filterBy

    init(_ raw: MeguriCoreBindings.FilterSection) {
        switch raw {
        case .savedSearches: self = .savedSearches
        case .cardState: self = .cardState
        case .flag: self = .flag
        case .due: self = .due
        case .added: self = .added
        case .edited: self = .edited
        case .studied: self = .studied
        case .decks: self = .decks
        case .tags: self = .tags
        case .notetypes: self = .notetypes
        case .cardTypes: self = .cardTypes
        case .fields: self = .fields
        case .filterBy: self = .filterBy
        }
    }
    var raw: MeguriCoreBindings.FilterSection {
        switch self {
        case .savedSearches: .savedSearches
        case .cardState: .cardState
        case .flag: .flag
        case .due: .due
        case .added: .added
        case .edited: .edited
        case .studied: .studied
        case .decks: .decks
        case .tags: .tags
        case .notetypes: .notetypes
        case .cardTypes: .cardTypes
        case .fields: .fields
        case .filterBy: .filterBy
        }
    }
}

/// A built-in saved search (T13).
public enum FilterBuiltIn: Sendable, Hashable, CaseIterable {
    case dueToday, addedInLast7Days, leeches

    init(_ raw: MeguriCoreBindings.FilterBuiltIn) {
        switch raw {
        case .dueToday: self = .dueToday
        case .addedInLast7Days: self = .addedInLast7Days
        case .leeches: self = .leeches
        }
    }
}

/// The labels fixed values and built-ins match by (T12.2, T15). Pass the
/// app's localized strings; `english` holds T15's.
public struct FilterLabels: Sendable, Hashable {
    public var currentDeck, untagged, new, learning, review, suspended, buried, leech: String
    public var noFlag, anyFlag, today, overdue, next7Days, last7Days, last30Days: String
    public var dueToday, addedInLast7Days, leeches: String
    public init(currentDeck: String, untagged: String, new: String, learning: String, review: String, suspended: String,
                buried: String, leech: String, noFlag: String, anyFlag: String, today: String, overdue: String,
                next7Days: String, last7Days: String, last30Days: String, dueToday: String, addedInLast7Days: String,
                leeches: String) {
        self.currentDeck = currentDeck
        self.untagged = untagged
        self.new = new
        self.learning = learning
        self.review = review
        self.suspended = suspended
        self.buried = buried
        self.leech = leech
        self.noFlag = noFlag
        self.anyFlag = anyFlag
        self.today = today
        self.overdue = overdue
        self.next7Days = next7Days
        self.last7Days = last7Days
        self.last30Days = last30Days
        self.dueToday = dueToday
        self.addedInLast7Days = addedInLast7Days
        self.leeches = leeches
    }
    public static let english = FilterLabels(
        currentDeck: "Current Deck", untagged: "Untagged", new: "New", learning: "Learning", review: "Review",
        suspended: "Suspended", buried: "Buried", leech: "Leech", noFlag: "No Flag", anyFlag: "Any Flag", today: "Today",
        overdue: "Overdue", next7Days: "Next 7 Days", last7Days: "Last 7 Days", last30Days: "Last 30 Days",
        dueToday: "Due Today", addedInLast7Days: "Added in Last 7 Days", leeches: "Leeches")
    var raw: MeguriCoreBindings.FilterLabels {
        .init(currentDeck: currentDeck, untagged: untagged, new: new, learning: learning, review: review,
              suspended: suspended, buried: buried, leech: leech, noFlag: noFlag, anyFlag: anyFlag, today: today,
              overdue: overdue, next7Days: next7Days, last7Days: last7Days, last30Days: last30Days, dueToday: dueToday,
              addedInLast7Days: addedInLast7Days, leeches: leeches)
    }
}

/// D6.1's kinds of a syntax candidate.
public enum SyntaxCandidateKind: Sendable, Hashable {
    case asTyped, keyword, deck, tag, notetype, template, field, flag, state, property, `operator`, valueHint, days,
         button, preset, savedSearch
}

/// A property's unit (D6.3.1 step 3).
public enum SyntaxUnit: Sendable, Hashable {
    case days, daysFromToday, count, easeFactor, position, daysAgo, zeroToOne, number, text
}

extension SyntaxUnit {
    init(_ raw: MeguriCoreBindings.SyntaxUnit) {
        switch raw {
        case .days: self = .days
        case .daysFromToday: self = .daysFromToday
        case .count: self = .count
        case .easeFactor: self = .easeFactor
        case .position: self = .position
        case .daysAgo: self = .daysAgo
        case .zeroToOne: self = .zeroToOne
        case .number: self = .number
        case .text: self = .text
        }
    }
}

/// D6.1's `detail`.
public enum SyntaxDetail: Sendable, Hashable {
    case filtered
    case flagName(String)
    case unit(SyntaxUnit)
    case days(UInt32)
    case button(UInt8)
}

/// One D6 candidate while Anki syntax is typed at the caret (T12.2).
public struct SyntaxCandidate: Sendable, Hashable {
    public let kind: SyntaxCandidateKind
    public let label: String
    public let detail: SyntaxDetail?
    /// UTF-16 range of the text to replace, the replacement, and the caret
    /// after it (an offset into the new text); nil for `asTyped` and
    /// `valueHint`.
    public let insert: (range: Range<Int>, text: String, caretAfter: Int)?
    /// Accepting it commits the search (D6.6).
    public let completes: Bool

    public init(kind: SyntaxCandidateKind, label: String, detail: SyntaxDetail?,
                insert: (range: Range<Int>, text: String, caretAfter: Int)?, completes: Bool) {
        self.kind = kind
        self.label = label
        self.detail = detail
        self.insert = insert
        self.completes = completes
    }

    public static func == (a: SyntaxCandidate, b: SyntaxCandidate) -> Bool {
        a.kind == b.kind && a.label == b.label && a.detail == b.detail && a.completes == b.completes
            && a.insert?.range == b.insert?.range && a.insert?.text == b.insert?.text
            && a.insert?.caretAfter == b.insert?.caretAfter
    }
    public func hash(into hasher: inout Hasher) {
        hasher.combine(kind)
        hasher.combine(label)
        hasher.combine(insert?.text)
    }

    init(_ raw: MeguriCoreBindings.SyntaxCandidate) {
        kind = switch raw.kind {
        case .asTyped: .asTyped
        case .keyword: .keyword
        case .deck: .deck
        case .tag: .tag
        case .notetype: .notetype
        case .template: .template
        case .field: .field
        case .flag: .flag
        case .state: .state
        case .property: .property
        case .`operator`: .`operator`
        case .valueHint: .valueHint
        case .days: .days
        case .button: .button
        case .preset: .preset
        case .savedSearch: .savedSearch
        }
        label = raw.label
        detail = raw.detail.map { detail in
            switch detail {
            case .filtered: .filtered
            case .flagName(let name): .flagName(name)
            case .unit(let unit): .unit(SyntaxUnit(unit))
            case .days(let days): .days(days)
            case .button(let button): .button(button)
            }
        }
        insert = raw.insert.map { (Int($0.start)..<Int($0.end), $0.text, Int($0.caretAfter)) }
        completes = raw.completes
    }
}

/// One row of the suggestions (T12.1).
public enum FilterSuggestionRow: Sendable, Hashable {
    /// "Contains “w”": keeps the word as text and commits.
    case contains(String)
    case builtIn(FilterBuiltIn)
    /// A user saved search: accepting it replaces the query by T7's
    /// reading of its text.
    case savedSearch(name: String, text: String)
    /// Opens the kind's value list.
    case filterBy(FilterSection)
    case value(FilterValue)
    /// A field name with the note types that have it.
    case field(name: String, notetypes: [Int64])
    case cardType(FilterValue, notetypes: [Int64])
    /// Ends a section cut short: opens that section's value list.
    case showAll(FilterSection)
    /// Save Search…, last while the query is not empty.
    case saveSearch
    case syntax(SyntaxCandidate)

    init(_ raw: MeguriCoreBindings.FilterSuggestionRow) {
        switch raw {
        case .contains(let word): self = .contains(word)
        case .builtIn(let search): self = .builtIn(FilterBuiltIn(search))
        case .savedSearch(let name, let text): self = .savedSearch(name: name, text: text)
        case .filterBy(let section): self = .filterBy(FilterSection(section))
        case .value(let value): self = .value(FilterValue(value))
        case .field(let name, let notetypes): self = .field(name: name, notetypes: notetypes)
        case .cardType(let value, let notetypes): self = .cardType(FilterValue(value), notetypes: notetypes)
        case .showAll(let section): self = .showAll(FilterSection(section))
        case .saveSearch: self = .saveSearch
        case .syntax(let candidate): self = .syntax(SyntaxCandidate(candidate))
        }
    }
}

/// One suggestion, its section, and the key to pass back in `recent` once
/// the user accepts it (nil for rows not ranked by use).
public struct FilterSuggestion: Sendable, Hashable {
    public let section: FilterSection?
    public let row: FilterSuggestionRow
    public let key: String?
    public init(section: FilterSection?, row: FilterSuggestionRow, key: String?) {
        self.section = section
        self.row = row
        self.key = key
    }
}

// MARK: Counts, column sets and selection counts

/// One thing a sidebar or suggestion row counts (T12.3 step 4).
public enum FilterCountItem: Sendable, Hashable {
    case value(FilterValue)
    /// A user saved search by its stored text.
    case saved(String)
    /// A whole query (the deck page's Suspended Cards row).
    case query(FilterQuery)

    var raw: MeguriCoreBindings.FilterCountItem {
        switch self {
        case .value(let value): .value(value: value.raw)
        case .saved(let text): .saved(text: text)
        case .query(let query): .query(query: query.raw)
        }
    }
}

/// Which column set a change writes (V2.2).
public enum ColumnSetID: Sendable, Hashable {
    case standard
    /// A note type's own set.
    case notetype(Int64)

    init(_ raw: MeguriCoreBindings.ColumnSetId) {
        switch raw {
        case .standard: self = .standard
        case .notetype(let id): self = .notetype(id)
        }
    }
    var raw: MeguriCoreBindings.ColumnSetId {
        switch self {
        case .standard: .standard
        case .notetype(let id): .notetype(id: id)
        }
    }
}

/// The set a list shows (V2.2).
public struct ColumnSetShown: Sendable, Hashable {
    /// The set changes write to: the note type's when the query names one
    /// that exists, even before it has an own set.
    public let set: ColumnSetID
    /// Whether that note type has an own set.
    public let own: Bool
    /// Column keys in order, never empty; the state column is not listed.
    public let cards: [String]
    public let notes: [String]
    public let cardsSort: BrowseSort
    public let notesSort: BrowseSort
    public init(set: ColumnSetID, own: Bool, cards: [String], notes: [String], cardsSort: BrowseSort, notesSort: BrowseSort) {
        self.set = set
        self.own = own
        self.cards = cards
        self.notes = notes
        self.cardsSort = cardsSort
        self.notesSort = notesSort
    }

    public func columns(_ mode: BrowseMode) -> [String] { mode == .cards ? cards : notes }
    public func sort(_ mode: BrowseMode) -> BrowseSort { mode == .cards ? cardsSort : notesSort }
}

/// A user change to a column set (V2.3, V4).
public enum ColumnSetChange: Sendable, Hashable {
    case show(String)
    case hide(String)
    case reorder([String])
    /// A header click: the column in its first direction, or the current
    /// sort reversed.
    case sortBy(String)
    /// The touch Sort By menu.
    case setSort(BrowseSort)

    var raw: MeguriCoreBindings.ColumnSetChange {
        switch self {
        case .show(let key): .show(key: key)
        case .hide(let key): .hide(key: key)
        case .reorder(let keys): .reorder(keys: keys)
        case .sortBy(let key): .sortBy(key: key)
        case .setSort(let sort): .setSort(sort: sort.raw)
        }
    }
}

/// What the inspector's summary line and the commands' "shown when" read
/// (V7, V8.1), for the selection's target cards and notes.
public struct BrowseSelectionCounts: Sendable, Hashable {
    public let cards: UInt64
    public let notes: UInt64
    public let suspended: UInt64
    public let buried: UInt64
    /// Distinct home decks (a filtered card's original deck).
    public let decks: UInt64
    /// That deck when there is exactly one.
    public let deck: Int64?
    /// The target notes' note types, ascending.
    public let notetypes: [Int64]
    public init(cards: UInt64, notes: UInt64, suspended: UInt64, buried: UInt64, decks: UInt64, deck: Int64?, notetypes: [Int64]) {
        self.cards = cards
        self.notes = notes
        self.suspended = suspended
        self.buried = buried
        self.decks = decks
        self.deck = deck
        self.notetypes = notetypes
    }
}

/// What Find Duplicates found (V10).
public struct FindDuplicatesResult: Sendable, Hashable {
    /// Each group: two or more note ids with equal keys, ascending; the
    /// groups by their first note. Empty: "No duplicates found."
    public let groups: [[Int64]]
    /// Every grouped note, ascending: the Duplicates token's note ids
    /// (`FilterValue.duplicates`).
    public let noteIds: [Int64]
    /// The target notes read: those whose note type has the field.
    public let considered: UInt64
    public init(groups: [[Int64]], noteIds: [Int64], considered: UInt64) {
        self.groups = groups
        self.noteIds = noteIds
        self.considered = considered
    }
}

/// A started Browse read: its progress and cancel, and its value once it
/// ends.
public final class CoreBrowseReadJob<Value: Sendable>: Sendable {
    public let job: CoreJob
    private let resultCall: BridgeCall
    private let read: @Sendable () throws -> Value?
    init(job: CoreJob, resultCall: BridgeCall, read: @escaping @Sendable () throws -> Value?) {
        self.job = job
        self.resultCall = resultCall
        self.read = read
    }
    public func value() async throws(BridgeError) -> Value {
        try await job.value()
        let read = read
        return try await MaintenanceLane.shared.call(resultCall) {
            guard let value = try read() else { throw BridgeError.internal("Successful job has no result") }
            return value
        }
    }
}

// MARK: Calls

/// Browse's query and read calls.
public enum CoreBrowseQuery {
    /// T4: the Anki search text the query stands for, which `startSearch`
    /// runs and Save Search stores.
    public static func runText(_ query: FilterQuery) async throws(BridgeError) -> String {
        let raw = query.raw
        return try await InteractiveLane.shared.call(.filterRunText) { try filterRunText(query: raw) }
    }

    /// T7: the query Anki search text reads as. It never fails.
    public static func read(_ text: String) async throws(BridgeError) -> FilterQuery {
        try await InteractiveLane.shared.call(.filterReadText) { FilterQuery(try filterReadText(text: text)) }
    }

    /// T8's commit: syntax in the text becomes tokens after the query's own.
    public static func commit(_ query: FilterQuery) async throws(BridgeError) -> FilterQuery {
        let raw = query.raw
        return try await InteractiveLane.shared.call(.filterCommit) { FilterQuery(try filterCommit(query: raw)) }
    }

    /// T8: whether the text is incomplete at the UTF-16 offset `caret`
    /// (D5), so the live search keeps the last results.
    public static func isIncomplete(_ text: String, caret: Int) async throws(BridgeError) -> Bool {
        try await InteractiveLane.shared.call(.filterTextIncomplete) {
            try filterTextIncomplete(text: text, caret: UInt32(clamping: caret))
        }
    }

    /// The Deck value of a sidebar row (with its subdecks); nil when no
    /// deck has that id or it is not offered.
    public static func deckValue(_ deckID: Int64) async throws(BridgeError) -> FilterValue? {
        try await InteractiveLane.shared.call(.filterDeckValue) { try filterDeckValue(deckId: deckID).map(FilterValue.init) }
    }

    /// T12: the suggestions for `query` with the caret at the UTF-16 offset
    /// `caret` of its text; `scope` asks for one kind's value list;
    /// `recent` holds the keys of accepted rows, newest first.
    public static func suggestions(for query: FilterQuery, caret: Int, scope: FilterSection? = nil, recent: [String] = [],
                                   labels: FilterLabels) async throws(BridgeError) -> [FilterSuggestion] {
        let raw = query.raw, rawLabels = labels.raw
        return try await InteractiveLane.shared.call(.filterSuggestions) {
            try filterSuggestions(query: raw, caret: UInt32(clamping: caret), scope: scope?.raw, recent: recent,
                                  labels: rawLabels).map {
                FilterSuggestion(section: $0.section.map(FilterSection.init), row: FilterSuggestionRow($0.row), key: $0.key)
            }
        }
    }

    /// T12.4: for each item, the rows of `mode` it finds in the whole
    /// collection (nil when it cannot be counted), as a job at utility QoS.
    /// A newer call cancels this one.
    public static func startFilterCounts(_ mode: BrowseMode, items: [FilterCountItem]) async throws(BridgeError)
        -> CoreBrowseReadJob<[UInt64?]> {
        try await CoreEventStream.shared.start()
        let rawMode = mode.raw, rawItems = items.map(\.raw)
        return try await MaintenanceLane.shared.call(.startFilterCounts) {
            let start = try MeguriCoreBindings.startFilterCounts(mode: rawMode, items: rawItems)
            let result = start.result
            return CoreBrowseReadJob(job: try Jobs.track(start.handle), resultCall: .filterCountsJobResult) {
                try result.result()
            }
        }
    }

    /// V2.2: the column set a list shows when the query's Note Type token
    /// names `notetype` (one value, is), or the standard set for nil.
    public static func columnSet(notetype: Int64?) async throws(BridgeError) -> ColumnSetShown {
        try await QueryLane.shared.call(.browserColumnSet) {
            let shown = try browserColumnSet(notetype: notetype)
            return ColumnSetShown(set: ColumnSetID(shown.set), own: shown.own, cards: shown.cards, notes: shown.notes,
                                  cardsSort: BrowseSort(shown.cardsSort), notesSort: BrowseSort(shown.notesSort))
        }
    }

    /// A user change to `set` in `mode`: no undo step, the undo steps kept
    /// (V2.4). `written` is false when nothing changed.
    public static func changeColumnSet(_ set: ColumnSetID, mode: BrowseMode, change: ColumnSetChange) async throws(BridgeError)
        -> BoolMutation {
        let rawSet = set.raw, rawMode = mode.raw, rawChange = change.raw
        return try await InteractiveLane.shared.call(.changeBrowserColumnSet) {
            BoolMutation(try changeBrowserColumnSet(set: rawSet, mode: rawMode, change: rawChange))
        }
    }

    /// Use Standard Columns (V2.2).
    public static func useStandardColumns(notetype: Int64) async throws(BridgeError) -> BoolMutation {
        try await InteractiveLane.shared.call(.useStandardBrowserColumns) {
            BoolMutation(try useStandardBrowserColumns(notetype: notetype))
        }
    }

    /// V7, V8.1: the counts of a selection's target cards and notes, as a
    /// job. A newer call for a selection of the same session cancels this
    /// one. When the selection itself is refused, `value()` throws that
    /// `BridgeError.browse` failure.
    public static func startSelectionCounts(_ selection: BrowseSelection) async throws(BridgeError)
        -> CoreBrowseReadJob<BrowseSelectionCounts> {
        try await CoreEventStream.shared.start()
        let raw = selection.raw
        return try await MaintenanceLane.shared.call(.startBrowseSelectionCounts) {
            let start = try MeguriCoreBindings.startBrowseSelectionCounts(selection: raw)
            let result = start.result
            return CoreBrowseReadJob(job: try Jobs.track(start.handle), resultCall: .browseSelectionCountsJobResult) {
                try result.result().map {
                    BrowseSelectionCounts(cards: $0.cards, notes: $0.notes, suspended: $0.suspended, buried: $0.buried,
                                          decks: $0.decks, deck: $0.deck, notetypes: $0.notetypes)
                }
            }
        }
    }

    /// V10's Find: the notes whose values of the field named exactly
    /// `field` have equal keys, grouped, among every note (`found` nil: All
    /// Notes) or the notes of `found`'s rows (All Found: a selection of
    /// every row of the results), as a job with progress and Cancel. It
    /// never writes. It holds a search reader until it ends: cancel it when
    /// the sheet goes away. When the selection itself is refused, `value()`
    /// throws that `BridgeError.browse` failure.
    public static func startFindDuplicates(field: String, in found: BrowseSelection?) async throws(BridgeError)
        -> CoreBrowseReadJob<FindDuplicatesResult> {
        try await CoreEventStream.shared.start()
        let raw = found?.raw
        return try await MaintenanceLane.shared.call(.startFindDuplicates) {
            let start = try MeguriCoreBindings.startFindDuplicates(field: field, found: raw)
            let result = start.result
            return CoreBrowseReadJob(job: try Jobs.track(start.handle), resultCall: .findDuplicatesJobResult) {
                try result.result().map {
                    FindDuplicatesResult(groups: $0.groups, noteIds: $0.noteIds, considered: $0.considered)
                }
            }
        }
    }

    /// T14: what a first open of Browse reads, the default search and the
    /// mode. It writes nothing.
    public static func opening() async throws(BridgeError) -> BrowseOpening {
        try await QueryLane.shared.call(.browseOpening) {
            let opening = try MeguriCoreBindings.browseOpening()
            return BrowseOpening(defaultSearch: opening.defaultSearch, notesMode: opening.notesMode)
        }
    }
}

/// What Browse opens with (`search/filter-tokens` T14).
public struct BrowseOpening: Sendable, Hashable {
    /// `defaultSearchText` as stored when it is set (`browser/config-keys`
    /// K6.4); nil opens the whole collection.
    public let defaultSearch: String?
    /// `browserTableShowNotesMode` (K6.1).
    public let notesMode: Bool
    public init(defaultSearch: String?, notesMode: Bool) {
        self.defaultSearch = defaultSearch
        self.notesMode = notesMode
    }
}
