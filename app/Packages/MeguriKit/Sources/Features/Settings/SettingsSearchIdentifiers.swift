#if os(macOS)
import AppKit
import SwiftUI

/// SwiftUI's native sidebar search exposes no per-part identifier modifier.
/// Attach only identifiers through public AppKit/AX APIs in this sidebar.
struct SettingsSearchIdentifiers: NSViewRepresentable {
    func makeNSView(context: Context) -> Attachment { Attachment() }
    func updateNSView(_ view: Attachment, context: Context) { view.attach() }

    @MainActor final class Attachment: NSView {
        override func viewDidMoveToSuperview() { super.viewDidMoveToSuperview(); attach() }
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); attach() }
        override func layout() { super.layout(); attach() }

        func attach() {
            guard window != nil else { return }
            // Stop at this column of the nearest split view, never the window
            // or another column. Reattachment follows native mount/layout.
            var column: NSView = self
            while let parent = column.superview {
                if parent is NSSplitView {
                    for field in Self.searchFields(in: column) { Self.identify(field) }
                    return
                }
                column = parent
            }
        }

        private static func searchFields(in view: NSView) -> [NSSearchField] {
            if let field = view as? NSSearchField { return [field] }
            return view.subviews.flatMap { searchFields(in: $0) }
        }

        private static func identify(_ field: NSSearchField) {
            field.setAccessibilityIdentifier("settings.root.search")
            guard let cell = field.cell as? NSSearchFieldCell,
                  let label = cell.searchButtonCell?.accessibilityLabel(), !label.isEmpty else { return }
            var visited: Set<ObjectIdentifier> = []
            let matches = accessibilityButtons(in: field, visited: &visited).filter { $0.accessibilityLabel() == label }
            guard matches.count == 1, let search = matches.first else { return }
            // The cell supplies the native localized label; only the actual
            // AX descendant receives the identifier.
            search.setAccessibilityIdentifier("settings.root.searchButton")
        }

        private static func accessibilityButtons(in element: any NSAccessibilityProtocol,
                                                  visited: inout Set<ObjectIdentifier>) -> [any NSAccessibilityProtocol] {
            guard visited.insert(ObjectIdentifier(element as AnyObject)).inserted else { return [] }
            let own: [any NSAccessibilityProtocol] = element.accessibilityRole() == .button ? [element] : []
            return own + (element.accessibilityChildren() ?? []).flatMap { child -> [any NSAccessibilityProtocol] in
                guard let child = child as? any NSAccessibilityProtocol else { return [] }
                return accessibilityButtons(in: child, visited: &visited)
            }
        }
    }
}
#endif
