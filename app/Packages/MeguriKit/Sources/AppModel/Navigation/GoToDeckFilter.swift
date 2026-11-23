import Foundation

/// The deck search's filter (`decks/deck-list` H6.2): every term of the query,
/// split at U+0020, occurs in the lowered full human name, `::` included, so a
/// term may span components. Lowercasing is Unicode's full mapping, not case
/// folding; the query is NFC first.
public nonisolated struct GoToDeckFilter: Sendable {
    private let terms: [[Unicode.Scalar]]

    public init(query: String) {
        terms = query.precomposedStringWithCanonicalMapping
            .split(separator: " ", omittingEmptySubsequences: true)
            .map { Array(Self.lower(String($0)).unicodeScalars) }
    }

    public func matches(fullName: String) -> Bool {
        let name = Array(Self.lower(fullName).unicodeScalars)
        return terms.allSatisfy { Self.contains(name, $0) }
    }

    /// Independent of the person's locale; keeps Final_Sigma and U+0130's `i` U+0307.
    private static func lower(_ text: String) -> String {
        (text as NSString).lowercased(with: Locale(identifier: "en_US_POSIX"))
    }
    private static func contains(_ name: [Unicode.Scalar], _ term: [Unicode.Scalar]) -> Bool {
        guard !term.isEmpty else { return true }
        guard term.count <= name.count else { return false }
        return (0...(name.count - term.count)).contains { start in
            name[start..<(start + term.count)].elementsEqual(term)
        }
    }
}
