#if MEGURI_TEST_HOOKS
@_spi(MeguriTestHooks) import AppModel
import Browse
import CoreBridge
import Home
import SwiftUI

extension TestHooks {
    /// Browse's section shell (U270; N21.1, `browser/workspace` V6) and its
    /// results table (U270a; N21.3, V2.5, V5, V11): the shell over a made-up
    /// collection held in memory. No collection is opened; nothing is
    /// written.
    ///
    /// - `browse/sidebar/typical`: Browse selected, the query Deck: Languages
    ///   and Card State: Suspended, so two rows show selected; Flags and
    ///   Tags folded, as they start.
    /// - `browse/sidebar/filtered`: the same with "ver" in the sidebar's
    ///   field (`browser/sidebar` S6).
    /// - `browse/sidebar/today`: Today selected: the decks, with the Browse
    ///   row under Today.
    /// - `browse/table/typical`: Deck: Languages, Cards, one row selected.
    /// - `browse/table/notes`: the same in Notes.
    /// - `browse/table/no-results`: a query that finds nothing.
    /// - `browse/table/empty-collection`: a collection with no cards.
    /// - `browse/table/error`: a search that fails for storage reasons.
    /// - `browse/table/large`: 150,000 rows of the whole collection, for
    ///   the scrolling performance test (`BrowserSearchPerfTests`).
    /// - `browse/search/tokens`: the search field (U271, N21.2) holding a
    ///   Deck token, a Tag is-not token of two values, a Flag token, an
    ///   advanced token and the text "plimb".
    /// - `browse/search/save-search`: Save Search… asking for a name (T13).
    /// - `browse/search/contains`: a Field token's Contains… asking for its
    ///   text (T10).
    enum BrowseRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.registerShell("browse/sidebar/typical") { _ in launch(.typical) }
            registry.registerShell("browse/sidebar/filtered") { _ in launch(.filtered) }
            registry.registerShell("browse/sidebar/today") { _ in launch(.today) }
            registry.registerShell("browse/table/typical") { _ in launch(.table) }
            registry.registerShell("browse/table/notes") { _ in launch(.notes) }
            registry.registerShell("browse/table/no-results") { _ in launch(.noResults) }
            registry.registerShell("browse/table/empty-collection") { _ in launch(.emptyCollection) }
            registry.registerShell("browse/table/error") { _ in launch(.error) }
            registry.registerShell("browse/table/large") { _ in launch(.large) }
            registry.registerShell("browse/search/tokens") { _ in launch(.tokens) }
            registry.registerShell("browse/search/save-search") { _ in launch(.saveSearch) }
            registry.registerShell("browse/search/contains") { _ in launch(.contains) }
        }

        enum State { case typical, filtered, today, table, notes, noResults, emptyCollection, error, large, tokens, saveSearch, contains }

        @MainActor static func launch(_ state: State) -> ShellLaunch {
            let cards = switch state {
            case .emptyCollection: 0
            case .large: 150_000
            default: 1_204
            }
            let session = SampleSearchSession(cards: cards, fails: state == .error)
            let model = BrowseModel(backend: SampleBrowseBackend(session: session))
            BrowseScrollDriver.shared.model = model
            let expansion = SampleDeckExpansion()
            var features = ShellSample.features(.typical)
            features.todayRoot = true
            #if os(macOS)
            features.nativeMacWorkspace = true
            features.availableSections = [.home, .browse, .noteTypes]
            #else
            features.availableSections = [.home, .browse, .settings]
            #endif
            features.sidebarDecks = { _ in AnyView(HomeSidebarPreview.decks(selection: nil)) }
            let placeholder = features.sectionRoot
            features.sectionRoot = { section, navigation in
                section == .browse ? AnyView(BrowseView(model: model)) : placeholder(section, navigation)
            }
            features.browseSidebar = { navigation, header in
                AnyView(BrowseSidebar(model: model, decks: HomeSidebarPreview.nodes, deckExpanded: { node in
                    let id = node.id
                    return Binding(get: { !expansion.collapsed.contains(id) }, set: { expanded in
                        if expanded { expansion.collapsed.remove(id) } else { expansion.collapsed.insert(id) }
                    })
                }, navigate: { navigation.select(.section($0)) }) { header })
            }
            let query: FilterQuery = switch state {
            case .typical, .filtered, .today:
                FilterQuery(tokens: [.value([SampleBrowseBackend.deck(1)], negated: false), .value([.state(.suspended)], negated: false)])
            case .table, .notes, .error: FilterQuery(tokens: [.value([SampleBrowseBackend.deck(1)], negated: false)])
            case .noResults: FilterQuery(tokens: [.value([SampleBrowseBackend.deck(7)], negated: false), .value([.state(.learning)], negated: false)])
            case .emptyCollection, .large: FilterQuery()
            case .saveSearch: FilterQuery(tokens: [.value([SampleBrowseBackend.deck(1)], negated: false)])
            case .contains: FilterQuery(tokens: [.value([.field("Front", .contains("zindro"))], negated: false)])
            case .tokens:
                FilterQuery(tokens: [.value([SampleBrowseBackend.deck(1)], negated: false),
                                     .value([.tag("brask"), .tag("orrin")], negated: true),
                                     .value([.flag(1)], negated: false), .advanced("prop:ivl>30")], text: "plimb")
            }
            return ShellLaunch(features: features, probe: false) { navigation in
                navigation.select(.section(state == .today ? .home : .browse))
                if state == .filtered { model.filter = "voc" }
                if state == .notes { model.setMode(.notes) }
                Task { @MainActor in
                    await model.prepareForTestHooks(query: query)
                    if state == .saveSearch { model.startSavingSearch() }
                    if state == .contains { model.editContains(in: 0) }
                    guard state == .table || state == .notes else { return }
                    // One row chosen, as after a click.
                    for _ in 0..<200 where model.results.list == nil { try? await Task.sleep(for: .milliseconds(10)) }
                    model.results.select([2])
                }
            }
        }
    }

    /// The sample decks' disclosures (Science starts folded, as its node says).
    @Observable @MainActor final class SampleDeckExpansion {
        var collapsed: Set<Int64> = Set(HomeSidebarPreview.nodes.flatMap(collapsedIDs))
        nonisolated private static func collapsedIDs(_ node: CountedDeckNode) -> [Int64] {
            (node.studyCollapsed ? [node.id] : []) + node.children.flatMap(collapsedIDs)
        }
    }

    /// A made-up collection's Browse reads, in memory (sample content only):
    /// `HomeSidebarPreview`'s decks, four note types, a few tags, one saved
    /// search, and counts for every row.
    @MainActor final class SampleBrowseBackend: BrowseBackend {
        var beforeSearchSession: (@MainActor () async -> Void)?
        private let session: SampleSearchSession
        init(session: SampleSearchSession = SampleSearchSession(cards: 1_204, fails: false)) {
            self.session = session
        }

        /// The deck value of a `HomeSidebarPreview` deck.
        static func deck(_ id: Int64) -> FilterValue {
            func find(_ nodes: [CountedDeckNode]) -> CountedDeckNode? {
                for node in nodes { if node.id == id { return node }; if let found = find(node.children) { return found } }
                return nil
            }
            func ids(_ node: CountedDeckNode) -> [Int64] { [node.id] + node.children.flatMap { ids($0) } }
            let node = find(HomeSidebarPreview.nodes)
            return .deck(id: id, native: (node?.name ?? "").replacingOccurrences(of: "::", with: "\u{1F}"),
                         tree: node.map { ids($0) } ?? [id])
        }
        private static let deckCounts: [Int64: UInt64] = [1: 2_091, 2: 1_204, 3: 412, 4: 565, 5: 96, 6: 30, 7: 196, 8: 385, 9: 385]
        private static let notetypeCounts: [(String, UInt64)] = [
            ("Basic", 512), ("Basic (and reversed card)", 196), ("Cloze", 124), ("Vocabulary", 1_840),
        ]
        private static let tagCounts: [String: UInt64] = [
            "unit-four": 96, "orrin": 141, "orrin::tavel": 88, "quill": 12, "spindle": 844, "brask": 318,
        ]

        func opening() async throws -> BrowseOpening { BrowseOpening(defaultSearch: nil, notesMode: false) }
        func read(_ text: String) async throws -> FilterQuery { FilterQuery(text: text) }
        /// The query as the sample session reads it: the decks and states
        /// of its is tokens.
        func runText(_ query: FilterQuery) async throws -> String {
            var decks: [Int64] = [], states: [String] = []
            for case .value(let values, false) in query.tokens {
                for value in values {
                    if case .deck(_, _, let tree) = value { decks += tree }
                    if case .state(let state) = value { states.append("\(state)") }
                }
            }
            return "decks=\(decks.map(String.init).joined(separator: ",")) states=\(states.joined(separator: ","))"
        }
        func commit(_ query: FilterQuery) async throws -> FilterQuery { query }
        func isIncomplete(_ text: String, caret: Int) async throws -> Bool { text.hasSuffix(":") }
        /// T12.1 over the sample names: the saved searches and Filter By for
        /// an empty word; otherwise Contains, then the names and labels that
        /// hold the word; a scope lists its kind's values.
        func suggestions(for query: FilterQuery, caret: Int, scope: FilterSection?, recent: [String]) async throws
            -> [FilterSuggestion] {
            typealias Named = (section: FilterSection, value: FilterValue, label: String)
            var all: [Named] = []
            for state in FilterState.allCases where state != .leech {
                all.append((.cardState, .state(state), "\(state)"))
            }
            all.append((.flag, .noFlag, "No Flag"))
            for flag in 1...7 { all.append((.flag, .flag(UInt8(flag)), Self.flagLabels[flag - 1])) }
            all.append((.due, .due(.today), "Today"))
            all.append((.due, .due(.overdue), "Overdue"))
            all.append((.due, .due(.next7Days), "Next 7 Days"))
            for id in Self.deckCounts.keys.sorted() { all.append((.decks, Self.deck(id), Self.deck(id).deckName ?? "")) }
            for name in Self.tagCounts.keys.sorted() { all.append((.tags, .tag(name), name)) }
            for (offset, notetype) in Self.notetypeCounts.enumerated() {
                all.append((.notetypes, .notetype(id: 1_700_000_100_001 + Int64(offset), name: notetype.0), notetype.0))
            }
            func shown(_ value: FilterValue) -> Bool {
                !query.tokens.contains { if case .value(let values, false) = $0 { values.contains(value) } else { false } }
            }
            if let scope {
                return all.filter { $0.0 == scope && shown($0.1) }.map { FilterSuggestion(section: $0.0, row: .value($0.1), key: nil) }
            }
            let word = String(query.text.split(separator: " ", omittingEmptySubsequences: false).last ?? "")
            guard !word.isEmpty else {
                let saved = FilterBuiltIn.allCases.map { FilterSuggestion(section: .savedSearches, row: .builtIn($0), key: nil) }
                    + [FilterSuggestion(section: .savedSearches, row: .savedSearch(name: "Brask to Review", text: "tag:brask is:due"), key: nil)]
                let kinds: [FilterSection] = [.decks, .tags, .notetypes, .cardTypes, .cardState, .flag, .due, .added, .edited, .studied, .fields]
                return saved + kinds.map { FilterSuggestion(section: .filterBy, row: .filterBy($0), key: nil) }
                    + (query.isEmpty ? [] : [FilterSuggestion(section: nil, row: .saveSearch, key: nil)])
            }
            let matches = all.filter { $0.2.localizedCaseInsensitiveContains(word) && shown($0.1) }
            var rows = [FilterSuggestion(section: nil, row: .contains(word), key: nil)]
            for section in [FilterSection.cardState, .flag, .due, .decks, .tags, .notetypes] {
                let found = matches.filter { $0.0 == section }
                rows += found.prefix(5).map { FilterSuggestion(section: section, row: .value($0.1), key: nil) }
                if found.count > 5 { rows.append(FilterSuggestion(section: section, row: .showAll(section), key: nil)) }
            }
            if "Front".localizedCaseInsensitiveContains(word) {
                rows.append(FilterSuggestion(section: .fields, row: .field(name: "Front", notetypes: [1_700_000_100_001, 1_700_000_100_002]),
                                             key: nil))
            }
            return rows
        }
        func saveSearch(name: String, text: String) async throws {}
        func renameSavedSearch(_ old: String, to name: String) async throws {}
        func deleteSavedSearch(_ name: String) async throws {}
        private static let flagLabels = ["Red", "Orange", "Green", "Blue", "Pink", "Turquoise", "Purple"]
        func values(_ section: FilterSection) async throws -> [FilterSuggestionRow] {
            switch section {
            case .savedSearches:
                FilterBuiltIn.allCases.map { .builtIn($0) } + [.savedSearch(name: "Brask to Review", text: "tag:brask is:due")]
            case .decks: Self.deckCounts.keys.sorted().map { .value(Self.deck($0)) }
            default: []
            }
        }
        func notetypes() async throws -> [NotetypeName] {
            Self.notetypeCounts.enumerated().map { NotetypeName(id: 1_700_000_100_001 + Int64($0.offset), name: $0.element.0) }
        }
        func tagTree() async throws -> [TagNode] {
            func tag(_ name: String, _ children: [TagNode] = []) -> TagNode {
                TagNode(name: name.components(separatedBy: "::").last ?? name, fullName: name, collapsed: false, implicit: false,
                        children: children)
            }
            return [tag("brask"), tag("orrin", [tag("orrin::tavel")]), tag("quill"), tag("spindle"), tag("unit-four")]
        }
        func flagNames() async throws -> [String] { ["Red", "Orange", "Green", "Blue", "Pink", "Turquoise", "Purple"] }
        func counts(_ mode: BrowseMode, _ items: [FilterCountItem]) async throws -> [UInt64?] {
            items.map { item -> UInt64? in
                switch item {
                case .value(.due(.today)): 47
                case .value(.added(.last7Days)): 23
                case .value(.state(.leech)): 12
                case .saved: 64
                case .value(.state(let state)): [.new: 412, .learning: 18, .review: 2_203, .suspended: 31, .buried: 8][state]
                case .value(.noFlag): 2_610
                case .value(.flag(let flag)): [1: 24, 2: 9, 3: 15, 4: 6, 5: 0, 6: 3, 7: 5][flag]
                case .value(.deck(let id, _, _)): Self.deckCounts[id]
                case .value(.notetype(_, let name)): Self.notetypeCounts.first { $0.0 == name }?.1
                case .value(.tag(let name)): Self.tagCounts[name]
                case .query(let query) where query.isEmpty: UInt64(mode == .notes ? (session.cards + 1) / 2 : session.cards)
                default: nil
                }
            }
        }
        func setTagsCollapsed(_ names: [String], collapsed: Bool) async throws {}
        func openSearchSession() async throws -> any BrowseSearching {
            await beforeSearchSession?()
            return session
        }
        func columnSet(notetype: Int64?) async throws -> ColumnSetShown {
            ColumnSetShown(set: .standard, own: false, cards: ["noteFld", "template", "deck", "cardDue", "cardMod"],
                           notes: ["noteFld", "note", "template", "noteTags", "noteCrt"],
                           cardsSort: BrowseSort(key: "noteFld", direction: .ascending),
                           notesSort: BrowseSort(key: "noteFld", direction: .ascending))
        }
        func changeColumnSet(_ set: ColumnSetID, mode: BrowseMode, change: ColumnSetChange) async throws {}
        func useStandardColumns(notetype: Int64) async throws {}
        func setNotesMode(_ notes: Bool) async throws {}
    }
}
#endif
