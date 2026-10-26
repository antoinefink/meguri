import Foundation

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
