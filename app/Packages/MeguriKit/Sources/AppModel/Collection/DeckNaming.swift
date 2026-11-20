import CoreBridge
import Foundation

/// One entry of the naming form's Parent Deck menu: a normal deck of the
/// deck tree, with its place in it.
public nonisolated struct DeckParentChoice: Identifiable, Hashable, Sendable {
    public let id: Int64
    /// The deck's own name, its last component.
    public let name: String
    /// Its full path, outermost first, itself last.
    public let path: [String]
    /// 0 for a top-level deck.
    public var depth: Int { path.count - 1 }
    /// The human full name the core takes (D1).
    public var fullName: String { path.joined(separator: "::") }
    /// The path as people read it; the "::" notation is not shown.
    public var spokenPath: String { path.joined(separator: " › ") }
}

/// What the naming form offers and saves (N20, D5, D6): a name and a parent
/// deck in place of a typed "::" path.
public nonisolated enum DeckNaming {
    /// The tree's decks in its order, each with its depth. Filtered decks
    /// cannot hold subdecks, and a deck cannot move inside itself, so the
    /// excluded deck and its descendants are left out. The tree itself already
    /// hides the empty Default deck (D12).
    public static func parentChoices(in nodes: [CountedDeckNode], excluding excluded: Int64? = nil) -> [DeckParentChoice] {
        var choices: [DeckParentChoice] = []
        func visit(_ node: CountedDeckNode, parents: [String]) {
            guard node.id != excluded, !node.filtered else { return }
            let path = DeckNaming.path(of: node, parents: parents)
            choices.append(DeckParentChoice(id: node.id, name: path.last ?? node.name, path: path))
            for child in node.children { visit(child, parents: path) }
        }
        for node in nodes { visit(node, parents: []) }
        return choices
    }

    /// The full name saved: the parent's full name, "::" and the typed name,
    /// or the typed name alone at the top level. A "::" typed in the name
    /// still nests (D1).
    public static func fullName(name: String, parent: DeckParentChoice?) -> String {
        guard let parent else { return name }
        return parent.fullName + "::" + name
    }

    /// The deck that holds `id` in the tree, nil for a top-level deck or one
    /// the tree does not show.
    public static func parent(of id: Int64, in nodes: [CountedDeckNode]) -> CountedDeckNode? {
        for node in nodes {
            if node.children.contains(where: { $0.id == id }) { return node }
            if let found = parent(of: id, in: node.children) { return found }
        }
        return nil
    }

    /// How many decks move with `id` when it is renamed or moved: all of its
    /// descendants.
    public static func descendantCount(of id: Int64, in nodes: [CountedDeckNode]) -> Int {
        func count(_ node: CountedDeckNode) -> Int { node.children.reduce(node.children.count) { $0 + count($1) } }
        func find(_ nodes: [CountedDeckNode]) -> CountedDeckNode? {
            for node in nodes {
                if node.id == id { return node }
                if let found = find(node.children) { return found }
            }
            return nil
        }
        return find(nodes).map(count) ?? 0
    }

    /// The line under the form. A rename of a deck with subdecks says that they
    /// follow; otherwise a parent is named in full; at the top level there is none.
    public enum Footer: Equatable, Sendable {
        case subdecksMove(Int)
        case inside(String)
    }
    public static func footer(renaming: Bool, descendants: Int, parent: DeckParentChoice?) -> Footer? {
        if renaming, descendants > 0 { return .subdecksMove(descendants) }
        return parent.map { .inside($0.spokenPath) }
    }

    /// A tree node's components. Fixture trees can carry a full "::" name.
    static func path(of node: CountedDeckNode, parents: [String]) -> [String] {
        node.name.contains("::") ? node.name.components(separatedBy: "::") : parents + [node.name]
    }
}
