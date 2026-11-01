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
            guard let cell = field.cell as? NSSearchFieldCell else { return }
            // Set metadata on the native cells directly; their opaque AX proxies
            // need not support an NSAccessibilityProtocol cast.
            cell.searchButtonCell?.setAccessibilityIdentifier("settings.root.searchButton")
            cell.cancelButtonCell?.setAccessibilityIdentifier("settings.root.searchClear")
        }
    }
}
#endif
