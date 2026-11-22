#if MEGURI_TEST_HOOKS
@_spi(MeguriTestHooks) import AppModel
import Browse
import CoreBridge
import Home
import SwiftUI

extension TestHooks {
    /// Browse's section shell (U270; N21.1, `browser/workspace` V6): the
    /// shell over a made-up collection held in memory. No collection is
    /// opened; nothing is written.
    ///
    /// - `browse/sidebar/typical`: Browse selected, the query Deck: Languages
    ///   and Card State: Suspended, so two rows show selected; Flags and
    ///   Tags folded, as they start.
    /// - `browse/sidebar/filtered`: the same with "ver" in the sidebar's
    ///   field (`browser/sidebar` S6).
    /// - `browse/sidebar/today`: Today selected: the decks, with the Browse
    ///   row under Today.
    enum BrowseRoutes {
        static func register(in registry: inout RouteRegistry) {
            registry.registerShell("browse/sidebar/typical") { _ in launch(.typical) }
            registry.registerShell("browse/sidebar/filtered") { _ in launch(.filtered) }
            registry.registerShell("browse/sidebar/today") { _ in launch(.today) }
        }

        enum State { case typical, filtered, today }

        @MainActor static func launch(_ state: State) -> ShellLaunch {
            BrowseRollout.shown = true
            let model = BrowseModel(backend: SampleBrowseBackend())
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
                AnyView(BrowseSidebar(model: model, decks: HomeSidebarPreview.nodes, deckExpanded: { id in
                    Binding(get: { !expansion.collapsed.contains(id) }, set: { expanded in
                        if expanded { expansion.collapsed.remove(id) } else { expansion.collapsed.insert(id) }
                    })
                }, navigate: { navigation.select(.section($0)) }) { header })
            }
            return ShellLaunch(features: features, probe: false) { navigation in
                navigation.select(.section(state == .today ? .home : .browse))
                if state == .filtered { model.filter = "voc" }
                Task { @MainActor in
                    await model.prepareForTestHooks(query: FilterQuery(tokens: [
                        .value([SampleBrowseBackend.deck(1)], negated: false), .value([.state(.suspended)], negated: false),
                    ]))
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
        func runText(_ query: FilterQuery) async throws -> String { "" }
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
                default: nil
                }
            }
        }
        func setTagsCollapsed(_ names: [String], collapsed: Bool) async throws {}
    }
}
#endif
