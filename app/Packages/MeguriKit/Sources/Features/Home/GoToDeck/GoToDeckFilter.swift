import CoreBridge
import Foundation
import Observation

/// Go to Deck's all-term, component substring search (R26/R27).
public nonisolated struct GoToDeckFilter: Sendable {
    private let terms: [String]

    public init(query: String) {
        terms = query.split(whereSeparator: { $0.isWhitespace }).map(String.init)
    }

    public func matches(fullName: String) -> Bool {
        let components = fullName.components(separatedBy: "::")
        return terms.allSatisfy { term in
            components.contains { component in
                component.range(of: term, options: .caseInsensitive) != nil
            }
        }
    }
}

/// The deck list's search (N14.2): Go to Deck's filter over the loaded tree,
/// and whether to offer creating the typed deck.
public nonisolated struct DeckSearch: Sendable {
    public struct Match: Identifiable, Hashable, Sendable {
        public let id: Int64
        public let name: String
        /// The ancestors' names, outermost first.
        public let ancestors: [String]
        public var fullName: String { (ancestors + [name]).joined(separator: "::") }
    }

    public let matches: [Match]
    /// No deck has the typed name, compared as Go to Deck does, without case.
    public let offersCreate: Bool

    public init(nodes: [CountedDeckNode], query: String) {
        let query = query.trimmingCharacters(in: .whitespacesAndNewlines)
        let filter = GoToDeckFilter(query: query)
        var found: [Match] = [], exists = false
        func visit(_ node: CountedDeckNode, ancestors: [String]) {
            let name = node.name.components(separatedBy: "::").last ?? node.name
            let match = Match(id: node.id, name: name, ancestors: ancestors)
            if match.fullName.compare(query, options: .caseInsensitive) == .orderedSame { exists = true }
            if filter.matches(fullName: match.fullName) { found.append(match) }
            for child in node.children { visit(child, ancestors: ancestors + [name]) }
        }
        for node in nodes { visit(node, ancestors: []) }
        matches = query.isEmpty ? [] : found
        offersCreate = !query.isEmpty && !exists
    }
}

/// The deck list's search field (N14.2), held by `AppNavigation` so a fold, an
/// unfold or a layout switch keeps the query.
@Observable @MainActor
public final class DeckListSearch {
    public var text = ""
    public var presented = false
    public init() {}
    /// The query to show results for; nil while the field is blank.
    public var query: String? {
        let query = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return query.isEmpty ? nil : query
    }
}
