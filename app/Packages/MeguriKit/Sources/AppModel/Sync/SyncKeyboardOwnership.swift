import Foundation
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Admission for the unmodified Y only. Native commands keep their own key
/// equivalents; no focus is claimed or moved by the sync shell.
public enum SyncKeyboardOwnership {
    public static func admits(_ collection: ReviewCollection) -> Bool {
        guard collection.navigation.sheet == nil, !collection.syncPresentation.statusOwnsInput else { return false }
        #if os(macOS)
        if let review = collection.review { return ReviewInputOwnership.admitsNativeCommand(review) }
        guard NSApp.isActive, let window = NSApp.keyWindow, window.attachedSheet == nil,
              let event = NSApp.currentEvent, event.type == .keyDown, !event.isARepeat,
              event.modifierFlags.intersection([.command, .control, .option, .shift]).isEmpty,
              let responder = window.firstResponder else { return false }
        return responder is NSWindow || (responder is NSView && !(responder is NSControl) && !(responder is NSTextView))
        #else
        // During review Rust disables Sync Now; card input retains its existing
        // native/DOM ownership and repeat suppression without another key path.
        guard collection.review == nil else { return false }
        let windows = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
            .filter { $0.activationState == .foregroundActive }.flatMap(\.windows)
        guard let window = windows.first(where: \.isKeyWindow), window.rootViewController?.presentedViewController == nil else { return false }
        func inputOwned(_ view: UIView) -> Bool {
            if view.isFirstResponder, view is any UITextInput || view is UIControl { return true }
            return view.subviews.contains(where: inputOwned)
        }
        return !inputOwned(window)
        #endif
    }
}
