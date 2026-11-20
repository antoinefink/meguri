import CardRendering
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Native focus is authoritative even while the DOM's last report is idle.
public enum ReviewInputOwnership {
    public static func admits(cardFocused: Bool, editing: Bool, paused: Bool, repeated: Bool) -> Bool {
        cardFocused && !editing && !paused && !repeated
    }
    #if os(macOS)
    /// Menu commands use exactly the card input admission boundary, including
    /// native focus, behavior dialogs, DOM editing and repeated key events.
    public static func admitsNativeCommand(_ model: ReviewCoordinator) -> Bool {
        guard let pair = model.pair, let pages = model.surface as? ReviewPages,
              pages.presenter.isAvailable else { return false }
        let repeated = NSApp.currentEvent.map { $0.type == .keyDown && $0.isARepeat } ?? false
        // An open Edit Note form owns the keys like a sheet (N11.1).
        return admits(cardFocused: cardFocused(in: pair.current.webView), editing: model.surface.editing,
            paused: model.paused || model.editingNote, repeated: repeated)
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
