#if os(iOS)
import Observation
import UIKit

/// Whether a text field or text view is editing in the app (iPad's menu bar):
/// while one is, Edit's Undo and Redo stay the system's, so the text keeps its
/// own undo (R174). UIKit's begin and end editing notifications drive it, which
/// SwiftUI then observes.
@Observable @MainActor
final class TextInputFocus {
    static let shared = TextInputFocus()
    private(set) var editing = false
    @ObservationIgnored private var observers: [any NSObjectProtocol] = []

    private init() {
        let center = NotificationCenter.default
        let changes: [(Notification.Name, Bool)] = [
            (UITextField.textDidBeginEditingNotification, true), (UITextField.textDidEndEditingNotification, false),
            (UITextView.textDidBeginEditingNotification, true), (UITextView.textDidEndEditingNotification, false),
        ]
        for (name, editing) in changes {
            observers.append(center.addObserver(forName: name, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated { TextInputFocus.shared.editing = editing }
            })
        }
    }
}
#endif
