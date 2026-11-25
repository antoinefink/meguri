#if os(macOS)
import AppKit
import AppModel
import SwiftUI

/// Closing the review window cancels Edit first, then continues that close.
/// Add owns its separate window. Collection shutdown owns the quit guards.
struct AddNoteWindowGuard: NSViewRepresentable {
    /// Edit Note's panel during a review: ⌘W and the close button act as its Cancel (E7).
    let editNote: EditNoteFlow?
    func makeCoordinator() -> Coordinator { Coordinator(editNote: editNote) }
    func makeNSView(context: Context) -> ObserverView {
        let view = ObserverView()
        view.changedWindow = { [weak coordinator = context.coordinator] in coordinator?.install(on: $0) }
        return view
    }
    func updateNSView(_ view: ObserverView, context: Context) {
        context.coordinator.editNote = editNote
        context.coordinator.install(on: view.window)
    }
    static func dismantleNSView(_ view: ObserverView, coordinator: Coordinator) { coordinator.restore() }

    final class ObserverView: NSView {
        var changedWindow: ((NSWindow?) -> Void)?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); changedWindow?(window) }
    }
    final class Coordinator: NSObject, NSWindowDelegate {
        weak var editNote: EditNoteFlow?
        private var awaitingEditClose = false
        private var continuingClose = false
        private weak var window: NSWindow?
        // NSObject's nonisolated selector introspection only reads this weak
        // reference. Installation and all window callbacks stay on MainActor.
        nonisolated(unsafe) private weak var previous: (any NSWindowDelegate)?
        init(editNote: EditNoteFlow?) { self.editNote = editNote }
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
            if continuingClose { return previous?.windowShouldClose?(sender) ?? true }
            guard !awaitingEditClose else { return false }
            if let editNote, editNote.isPresented {
                awaitingEditClose = true
                Task {
                    let cancelled = await editNote.requestCancel()
                    awaitingEditClose = false
                    guard cancelled, window === sender else { return }
                    continuingClose = true
                    defer { continuingClose = false }
                    sender.performClose(nil)
                }
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
