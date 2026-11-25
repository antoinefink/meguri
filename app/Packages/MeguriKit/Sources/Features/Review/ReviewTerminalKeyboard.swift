import AppModel
import SwiftUI
#if os(macOS)
import AppKit

/// Terminal input has no card page to own the keys. Observe only its own
/// active window, leaving text input, sheets, menus and other windows alone.
struct ReviewTerminalKeyboard: NSViewRepresentable {
    let enabled: Bool
    let continueFocused: Bool
    let done: () -> Void
    final class Host: NSView {
        var attached: (() -> Void)?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); attached?() }
    }
    final class Coordinator {
        var enabled = false
        var continueFocused = false
        var done: () -> Void = {}
        private var presentedAt: Double
        private var gate: ReviewTerminalKeyGate
        var monitor: Any?
        var menuTrackingDepth = 0
        var observers: [NSObjectProtocol] = []
        init(presentedAt: Double = ProcessInfo.processInfo.systemUptime) {
            self.presentedAt = presentedAt
            gate = ReviewTerminalKeyGate(presentedAt: presentedAt)
        }
        func reset() {
            presentedAt = ProcessInfo.processInfo.systemUptime
            gate = ReviewTerminalKeyGate(presentedAt: presentedAt)
        }
        /// Admission (window, modifiers, text and menus) is checked by the
        /// monitor. Only the Continue button's explicit focus yields Space;
        /// held grades still cannot activate it or finish the summary.
        func route(_ event: NSEvent) -> NSEvent? {
            if continueFocused, event.charactersIgnoringModifiers == " ", !event.isARepeat,
               event.timestamp > presentedAt { return event }
            if gate.take(timestamp: event.timestamp, repeated: event.isARepeat, enabled: enabled) { done() }
            return nil
        }
        isolated deinit {
            if let monitor { NSEvent.removeMonitor(monitor) }
            for observer in observers { NotificationCenter.default.removeObserver(observer) }
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> Host {
        let view = Host()
        let owner = context.coordinator
        for (name, tracking) in [(NSMenu.didBeginTrackingNotification, true), (NSMenu.didEndTrackingNotification, false)] {
            owner.observers.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak owner] _ in
                MainActor.assumeIsolated {
                    guard let owner else { return }
                    owner.menuTrackingDepth = tracking ? owner.menuTrackingDepth + 1 : max(0, owner.menuTrackingDepth - 1)
                }
            })
        }
        view.attached = { [weak view, weak owner] in
            guard let owner else { return }
            if let monitor = owner.monitor { NSEvent.removeMonitor(monitor); owner.monitor = nil }
            guard view?.window != nil else { return }
            owner.reset()
            owner.monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak view, weak owner] event in
                guard let owner, let window = view?.window, event.window === window,
                      NSApp.isActive, window.isKeyWindow, window.attachedSheet == nil, NSApp.modalWindow == nil,
                      event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty,
                      [" ", "\r", "\n", "\u{3}"].contains(event.charactersIgnoringModifiers ?? ""),
                      !(window.firstResponder is NSTextView), !(window.firstResponder is NSTextField),
                      owner.enabled, owner.menuTrackingDepth == 0 else { return event }
                return owner.route(event)
            }
        }
        return view
    }
    func updateNSView(_ view: Host, context: Context) {
        context.coordinator.enabled = enabled
        context.coordinator.continueFocused = continueFocused
        context.coordinator.done = done
    }
    static func dismantleNSView(_ view: Host, coordinator: Coordinator) {
        if let monitor = coordinator.monitor { NSEvent.removeMonitor(monitor); coordinator.monitor = nil }
        for observer in coordinator.observers { NotificationCenter.default.removeObserver(observer) }
        coordinator.observers.removeAll()
    }
}
#endif
