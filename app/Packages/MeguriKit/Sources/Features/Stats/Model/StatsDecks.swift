import CoreBridge
import Foundation

/// The decks the page knows (the deck list's tree with its counts, `sched/counts` C5): the deck
/// pop-up's menu (S3.1), the subtitle's deck count, the deck names the page writes, By Deck's
/// rows and Remaining Today (S6).
public struct StatsDeckDirectory: Hashable, Sendable {
    public struct Deck: Hashable, Sendable {
        public let id: Int64
        /// The deck's own name (the last part of its full name).
        public let name: String
        public let fullName: String
        public let depth: Int
        public let filtered: Bool
        public let parent: Int64?
        public let children: [Int64]
        /// Its cards waiting today, its subdecks' included (C5).
        public let counts: ReviewCounts
    }

    /// One row of the deck pop-up's menu, after All Decks (S3.1).
    public struct MenuItem: Hashable, Sendable, Identifiable {
        public let id: Int64
        public let name: String
        /// The indent: 0 for a top-level deck.
        public let depth: Int
        public let filtered: Bool
    }

    private var decks: [Int64: Deck] = [:]
    /// Every deck in the deck list's order.
    private var order: [Int64] = []
    private var topLevel: [Int64] = []
    /// The tree has been read at least once (a deck missing from an empty directory is unknown).
    public let isKnown: Bool

    public static let unknown = StatsDeckDirectory()

    private init() { isKnown = false }

    public init(nodes: [CountedDeckNode]) {
        isKnown = true
        topLevel = nodes.map(\.id)
        func add(_ node: CountedDeckNode, parent: Deck?) {
            let full = parent.map { "\($0.fullName)::\(node.name)" } ?? node.name
            let deck = Deck(id: node.id, name: node.name, fullName: full, depth: (parent?.depth ?? -1) + 1,
                            filtered: node.filtered, parent: parent?.id, children: node.children.map(\.id),
                            counts: node.counts)
            decks[node.id] = deck
            order.append(node.id)
            for child in node.children { add(child, parent: deck) }
        }
        for node in nodes { add(node, parent: nil) }
    }

    public func deck(_ id: Int64) -> Deck? { decks[id] }

    /// The deck pop-up's rows (S3.1): the deck tree indented in the deck list's order, filtered
    /// decks last.
    public var menu: [MenuItem] {
        let items = order.compactMap { decks[$0] }.map { MenuItem(id: $0.id, name: $0.name, depth: $0.depth, filtered: $0.filtered) }
        return items.filter { !$0.filtered } + items.filter(\.filtered).map {
            MenuItem(id: $0.id, name: decks[$0.id]?.fullName ?? $0.name, depth: 0, filtered: true)
        }
    }

    /// The decks of a page scope: a deck with its subdecks, or every deck, filtered decks
    /// included (S3.1's subtitle).
    public func deckCount(_ scope: StatsPageScope) -> Int? {
        guard isKnown else { return nil }
        switch scope {
        case .allDecks: return decks.count
        case let .deck(id): return decks[id].map { subtree($0.id).count }
        case .search: return nil
        }
    }

    func subtree(_ id: Int64) -> [Int64] {
        [id] + (decks[id]?.children ?? []).flatMap(subtree)
    }

    /// Remaining Today's count (S6): the deck's cards waiting today, or Today's totals for All
    /// Decks (the top-level decks' counts, which hold their subdecks').
    public func remaining(_ scope: StatsPageScope) -> Int? {
        guard isKnown else { return nil }
        func waiting(_ counts: ReviewCounts) -> Int { Int(counts.new) + Int(counts.learning) + Int(counts.review) }
        switch scope {
        case .allDecks: return topLevel.compactMap { decks[$0] }.reduce(0) { $0 + waiting($1.counts) }
        case let .deck(id): return decks[id].map { waiting($0.counts) }
        case .search: return nil
        }
    }

    /// Whether the deck the page shows is gone (S12.7): known only once the tree was read.
    public func isMissing(_ scope: StatsPageScope) -> Bool {
        guard isKnown, case let .deck(id) = scope else { return false }
        return decks[id] == nil
    }
}
