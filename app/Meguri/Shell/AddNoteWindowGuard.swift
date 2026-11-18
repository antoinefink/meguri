#if os(macOS)
import AppKit
import AppModel
import SwiftUI

/// The existing window's Close command and close button first close Add via
/// its guard. All other window delegate behavior remains with SwiftUI.
struct AddNoteWindowGuard: NSViewRepresentable {
    let navigation: AppNavigation
    func makeCoordinator() -> Coordinator { Coordinator(navigation: navigation) }
    func makeNSView(context: Context) -> ObserverView {
        let view = ObserverView()
        view.changedWindow = { [weak coordinator = context.coordinator] in coordinator?.install(on: $0) }
        return view
    }
    func updateNSView(_ view: ObserverView, context: Context) { context.coordinator.install(on: view.window) }
    static func dismantleNSView(_ view: ObserverView, coordinator: Coordinator) { coordinator.restore() }

    final class ObserverView: NSView {
        var changedWindow: ((NSWindow?) -> Void)?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); changedWindow?(window) }
    }
    final class Coordinator: NSObject, NSWindowDelegate {
        let navigation: AppNavigation
        private weak var window: NSWindow?
        // NSObject's nonisolated selector introspection only reads this weak
        // reference. Installation and all window callbacks stay on MainActor.
        nonisolated(unsafe) private weak var previous: (any NSWindowDelegate)?
        init(navigation: AppNavigation) { self.navigation = navigation }
        func install(on window: NSWindow?) {
            guard let window, window.delegate !== self else { return }
            restore()
            self.window = window; previous = window.delegate; window.delegate = self
        }
        func restore() {
            if window?.delegate === self { window?.delegate = previous }
            window = nil; previous = nil
        }
        func windowShouldClose(_ sender: NSWindow) -> Bool {
            if case .add = navigation.sheet {
                navigation.dismissSheet()
                return false
            }
            return previous?.windowShouldClose?(sender) ?? true
        }
        nonisolated override func responds(to selector: Selector!) -> Bool {
            super.responds(to: selector) || previous?.responds(to: selector) == true
        }
        nonisolated override func forwardingTarget(for selector: Selector!) -> Any? {
            if previous?.responds(to: selector) == true { return previous }
            return super.forwardingTarget(for: selector)
        }
    }
}
#endif
