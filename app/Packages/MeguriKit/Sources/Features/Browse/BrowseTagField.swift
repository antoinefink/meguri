import SwiftUI
#if os(macOS)
import AppKit

/// V12's Tags: the system token field, a token per tag, completed from the
/// collection's tags (`tags/complete`'s prefix match on a tag's full name
/// and on each of its components, ignoring case). What it holds is handed
/// on when editing ends: the inspector saves it then.
struct BrowseTagField: NSViewRepresentable {
    let tags: [String]
    let names: [String]
    let commit: ([String]) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSTokenField {
        let field = NSTokenField()
        field.delegate = context.coordinator
        field.tokenStyle = .rounded
        // A tag holds no white space: a space ends a token, as Return does.
        field.tokenizingCharacterSet = CharacterSet.whitespacesAndNewlines
        field.completionDelay = 0
        field.isBordered = true
        field.isBezeled = true
        field.bezelStyle = .squareBezel
        field.lineBreakMode = .byWordWrapping
        field.cell?.wraps = true
        field.cell?.isScrollable = false
        field.objectValue = tags
        field.setContentHuggingPriority(.defaultLow, for: .horizontal)
        field.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        return field
    }

    func updateNSView(_ field: NSTokenField, context: Context) {
        context.coordinator.parent = self
        // A field being edited keeps what the person typed.
        guard field.currentEditor() == nil, (field.objectValue as? [String]) != tags else { return }
        field.objectValue = tags
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView field: NSTokenField, context: Context) -> CGSize? {
        guard let width = proposal.width, width.isFinite else { return nil }
        let height = field.cell?.cellSize(forBounds: NSRect(x: 0, y: 0, width: width, height: .greatestFiniteMagnitude)).height
        return CGSize(width: width, height: height ?? field.intrinsicContentSize.height)
    }

    final class Coordinator: NSObject, NSTokenFieldDelegate {
        var parent: BrowseTagField
        init(_ parent: BrowseTagField) { self.parent = parent }

        func tokenField(_ tokenField: NSTokenField, completionsForSubstring substring: String, indexOfToken tokenIndex: Int,
                        indexOfSelectedItem selectedIndex: UnsafeMutablePointer<Int>?) -> [Any]? {
            let typed = substring.lowercased()
            guard !typed.isEmpty else { return [] }
            let held = Set((tokenField.objectValue as? [String] ?? []).map { $0.lowercased() })
            let matches = parent.names.filter { name in
                let lower = name.lowercased()
                return !held.contains(lower) && (lower.hasPrefix(typed) || lower.components(separatedBy: "::").contains { $0.hasPrefix(typed) })
            }
            selectedIndex?.pointee = -1
            return Array(matches.prefix(50))
        }

        func controlTextDidEndEditing(_ notification: Notification) {
            guard let field = notification.object as? NSTokenField else { return }
            let tags = (field.objectValue as? [String] ?? []).flatMap { $0.split(whereSeparator: \.isWhitespace).map(String.init) }
            var seen = Set<String>(), unique: [String] = []
            for tag in tags where seen.insert(tag.lowercased()).inserted { unique.append(tag) }
            parent.commit(unique)
        }
    }
}
#endif
