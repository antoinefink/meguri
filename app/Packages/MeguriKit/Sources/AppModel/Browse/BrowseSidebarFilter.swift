import Foundation

/// The sidebar's filter box while Browse is selected (`browser/sidebar` S6,
/// `browser/workspace` V6): which rows show and which stand expanded.
public nonisolated struct BrowseSidebarFilter: Sendable, Hashable {
    public let text: String
    public init(_ text: String) { self.text = text }

    /// On while the text holds a character that is not White_Space.
    public var isOn: Bool { text.unicodeScalars.contains { !$0.properties.isWhitespace } }

    /// A label matches when it holds the whole text, case folded, leading
    /// and trailing spaces included (S6).
    public func matches(_ label: String) -> Bool {
        isOn && label.range(of: text, options: [.caseInsensitive]) != nil
    }

    /// The range of `label` that matches, for its highlight.
    public func match(in label: String) -> Range<String.Index>? {
        isOn ? label.range(of: text, options: [.caseInsensitive]) : nil
    }

    /// One shown node of a filtered tree: the children shown under it, and
    /// whether it stands expanded (an item with a matching descendant does,
    /// nothing else does).
    public nonisolated struct Node<Item> {
        public let item: Item
        public let children: [Node<Item>]
        public let matched: Bool
        public var expanded: Bool { !children.isEmpty }
    }

    /// S6 over a tree: an item shows when it matches or one of its
    /// descendants does; under a matching section root (`rootMatched`)
    /// every top-level item shows too, folded unless a descendant matches.
    public func tree<Item>(_ items: [Item], label: (Item) -> String, children: (Item) -> [Item],
                           rootMatched: Bool) -> [Node<Item>] {
        items.compactMap { item in
            let below = tree(children(item), label: label, children: children, rootMatched: false)
            let matched = matches(label(item))
            guard matched || !below.isEmpty || rootMatched else { return nil }
            return Node(item: item, children: below, matched: matched)
        }
    }

    /// S6 over a flat section: its matching rows, or every row when the
    /// section's own label matches.
    public func rows<Item>(_ items: [Item], label: (Item) -> String, rootMatched: Bool) -> [Item] {
        rootMatched ? items : items.filter { matches(label($0)) }
    }
}
