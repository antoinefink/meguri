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
            guard let search = (field.cell as? NSSearchFieldCell)?.searchButtonCell else { return }
            // The exported button can be a virtual AX child rather than the
            // public cell. Match its native localized name, never the cancel
            // button or another search field. No roles or labels are changed.
            var visited: Set<ObjectIdentifier> = []
            identifyButton(in: field, search: search, visited: &visited)
        }

        private static func identifyButton(in element: any NSAccessibilityProtocol, search: NSButtonCell,
                                           visited: inout Set<ObjectIdentifier>) {
            guard visited.insert(ObjectIdentifier(element as AnyObject)).inserted else { return }
            if element.accessibilityRole() == .button,
               (element as AnyObject === search || (search.accessibilityLabel() != nil
                    && element.accessibilityLabel() == search.accessibilityLabel())) {
                element.setAccessibilityIdentifier("settings.root.searchButton")
            }
            for child in element.accessibilityChildren() ?? [] {
                if let child = child as? any NSAccessibilityProtocol {
                    identifyButton(in: child, search: search, visited: &visited)
                }
            }
        }
    }
}
#endif
