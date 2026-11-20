#if os(macOS)
import AppKit
import Observation

/// Makes the menu bar's Undo and Redo follow the focused text input (R174):
/// SwiftUI reads commands again only when observed state changes, so this
/// counts changes of the key window's first responder and of undo history.
@Observable @MainActor
final class MacUndoResponder {
    static let shared = MacUndoResponder()
    private(set) var revision: UInt64 = 0
    @ObservationIgnored private weak var lastResponder: NSResponder?
    @ObservationIgnored private var observers: [any NSObjectProtocol] = []

    private init() {
        let center = NotificationCenter.default
        for name in [Notification.Name.NSUndoManagerDidCloseUndoGroup, .NSUndoManagerDidUndoChange,
                     .NSUndoManagerDidRedoChange, NSWindow.didBecomeKeyNotification] {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { MacUndoResponder.shared.revision &+= 1 }
            })
        }
        // A window updates after every event; only a new first responder counts.
        observers.append(center.addObserver(forName: NSWindow.didUpdateNotification, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated {
                guard let window = NSApp.keyWindow else { return }
                let responder = MacUndoResponder.shared
                if responder.lastResponder !== window.firstResponder {
                    responder.lastResponder = window.firstResponder
                    responder.revision &+= 1
                }
            }
        })
    }

    /// The key window's first responder and its undo manager, read now.
    var firstResponder: NSResponder? {
        _ = revision
        return NSApp.keyWindow?.firstResponder
    }
}
#endif
