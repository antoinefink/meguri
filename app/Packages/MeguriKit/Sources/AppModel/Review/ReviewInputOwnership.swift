import CardRendering
#if os(macOS)
import AppKit
import WebKit
#else
import UIKit
#endif

/// Native focus is authoritative even while the DOM's last report is idle.
public enum ReviewInputOwnership {
    public static func admits(cardFocused: Bool, editing: Bool, paused: Bool, repeated: Bool) -> Bool {
        cardFocused && !editing && !paused && !repeated
    }
    #if os(macOS)
    /// Nil waits only for this editor to return the keys. A different window
    /// or an inactive app relinquishes the one-use request.
    static func editedCardWindowReady(in view: CardPlatformView) -> Bool? {
        guard let window = view.window, NSApp?.isActive == true else { return false }
        if NSApp.keyWindow?.identifier?.rawValue == "review.editCurrent.window" { return nil }
        guard window.isKeyWindow,
              window.attachedSheet == nil, NSApp.modalWindow == nil else { return false }
        return true
    }

    /// Only repair the exact parked native page retained across this edit's
    /// swap. Other responders (including WebKit input descendants) keep focus.
    @discardableResult
    static func restoreEditedCardResponder(in pair: CardPagePair) -> Bool {
        guard let window = pair.current.webView.window,
              pair.idle.webView.window === window,
              window.firstResponder === pair.idle.webView else { return false }
        // CardNativeWebView accepts native focus only with this ownership.
        let owned = pair.ownsKeyboardFocus
        pair.ownsKeyboardFocus = true
        if window.makeFirstResponder(pair.current.webView) { return true }
        pair.ownsKeyboardFocus = owned
        return false
    }

    /// Replay belongs to the active review window, including its native
    /// controls. Text input, sheets and menu tracking retain their keys.
    public static func replayFocused(in container: CardPlatformView) -> Bool {
        guard let window = container.window, NSApp?.isActive == true, window.isKeyWindow,
              window.attachedSheet == nil, NSApp.modalWindow == nil,
              RunLoop.current.currentMode != .eventTracking,
              let responder = window.firstResponder else { return false }
        return replayResponder(responder, in: container)
    }

    static func replayResponder(_ responder: NSResponder, in container: CardPlatformView) -> Bool {
        guard let view = responder as? NSView else { return responder === container.window }
        if contains(view, in: container) { return true } // DOM editing is checked separately.
        var ancestor: NSView? = view
        while let current = ancestor {
            if current is NSText || current is NSTextField || current is NSComboBox || current is WKWebView { return false }
            ancestor = current.superview
        }
        return true
    }

    /// Menu commands use exactly the card input admission boundary, including
    /// native focus, behavior dialogs, DOM editing and repeated key events.
    public static func admitsNativeCommand(_ model: ReviewCoordinator) -> Bool {
        let repeated = NSApp.currentEvent.map { $0.type == .keyDown && $0.isARepeat } ?? false
        return nativeCommandFocusAvailable(model) && !repeated
    }

    /// Persistent availability for scene registration. The current event's
    /// repeat flag belongs to dispatch, and must never be stored in a scene's
    /// focused value after that event has ended.
    public static func nativeCommandFocusAvailable(_ model: ReviewCoordinator) -> Bool {
        guard let pair = model.pair, let pages = model.surface as? ReviewPages,
              pages.presenter.isAvailable else { return false }
        // An open Edit Note form owns the keys like a sheet (N11.1).
        return admits(cardFocused: cardFocused(in: pair.current.webView), editing: model.surface.editing,
            paused: model.paused || model.editingNote, repeated: false)
    }
    #else
    /// The iPad menu bar's commands (N20.4) use the card host's admission:
    /// the card has the focus, no DOM editing, not paused.
    public static func admitsNativeCommand(_ model: ReviewCoordinator) -> Bool {
        guard let pair = model.pair, let pages = model.surface as? ReviewPages,
              pages.presenter.isAvailable else { return false }
        return admits(cardFocused: cardFocused(in: pair.current.webView), editing: model.surface.editing,
            paused: model.paused, repeated: false)
    }
    #endif
    static func contains(_ view: CardPlatformView, in container: CardPlatformView) -> Bool {
        view === container || view.isDescendant(of: container)
    }
    public static func cardFocused(in container: CardPlatformView) -> Bool {
        #if os(macOS)
        guard let window = container.window, NSApp?.isActive == true, window.isKeyWindow,
              let responder = window.firstResponder as? NSView else { return false }
        return contains(responder, in: container)
        #else
        guard let window = container.window, window.isKeyWindow,
              window.windowScene?.activationState == .foregroundActive else { return false }
        // Full Keyboard Access can focus a native button without changing the
        // responder. That focus takes priority over a retained WebKit responder.
        if let item = UIFocusSystem.focusSystem(for: container)?.focusedItem {
            guard let focused = item as? UIView, contains(focused, in: container) else { return false }
        }
        func responder(in view: UIView) -> UIView? {
            if view.isFirstResponder { return view }
            for child in view.subviews { if let found = responder(in: child) { return found } }
            return nil
        }
        guard let focused = responder(in: window) else { return false }
        return contains(focused, in: container)
        #endif
    }

    /// Nil means attachment/activation is still pending. An outside native
    /// responder or keyboard-focused control wins the first-presentation race.
    static func initialNativeInputOwned(in container: CardPlatformView) -> Bool? {
        #if os(macOS)
        guard let window = container.window, NSApp?.isActive == true, window.isKeyWindow else { return nil }
        guard let responder = window.firstResponder else { return false }
        if responder === window || responder === window.contentView { return false }
        guard let view = responder as? NSView else { return true }
        return !contains(view, in: container)
        #else
        guard let window = container.window, window.isKeyWindow,
              window.windowScene?.activationState == .foregroundActive else { return nil }
        if let item = UIFocusSystem.focusSystem(for: container)?.focusedItem {
            guard let view = item as? UIView, contains(view, in: container) else { return true }
        }
        func outsideResponder(in view: UIView) -> Bool {
            if view.isFirstResponder { return view !== window && !contains(view, in: container) }
            return view.subviews.contains { outsideResponder(in: $0) }
        }
        return outsideResponder(in: window)
        #endif
    }

    static func focusCard(_ view: CardPlatformView) {
        #if os(macOS)
        _ = view.window?.makeFirstResponder(view)
        #else
        _ = view.becomeFirstResponder()
        #endif
    }
}

/// Resolve once per session, including when a native control already owns input.
struct ReviewInitialFocus {
    private(set) var resolved = false
    mutating func claim(presented: Bool, active: Bool, nativeInputOwned: Bool?) -> Bool {
        guard !resolved, presented, active, let nativeInputOwned else { return false }
        resolved = true
        return !nativeInputOwned
    }
}
