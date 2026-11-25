import AppModel
import SwiftUI
#if os(macOS)
import AppKit

/// Terminal input has no card page to own the keys. Observe only its own
/// active window, leaving text input, sheets, menus and other windows alone.
struct ReviewTerminalKeyboard: NSViewRepresentable {
    let enabled: Bool
    let done: () -> Void
    final class Host: NSView {
        var attached: (() -> Void)?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); attached?() }
    }
    final class Coordinator {
        var enabled = false
        var done: () -> Void = {}
        var gate = ReviewTerminalKeyGate(presentedAt: ProcessInfo.processInfo.systemUptime)
        var monitor: Any?
        var menuTrackingDepth = 0
        var observers: [NSObjectProtocol] = []
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
            owner.gate = ReviewTerminalKeyGate(presentedAt: ProcessInfo.processInfo.systemUptime)
            owner.monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak view, weak owner] event in
                guard let owner, let window = view?.window, event.window === window,
                      NSApp.isActive, window.isKeyWindow, window.attachedSheet == nil, NSApp.modalWindow == nil,
                      event.modifierFlags.intersection([.command, .option, .control, .shift]).isEmpty,
                      [" ", "\r", "\n", "\u{3}"].contains(event.charactersIgnoringModifiers ?? ""),
                      !(window.firstResponder is NSTextView), !(window.firstResponder is NSTextField),
                      owner.enabled, owner.menuTrackingDepth == 0 else { return event }
                if owner.gate.take(timestamp: event.timestamp, repeated: event.isARepeat, enabled: owner.enabled) { owner.done() }
                // Consume repeats too: a focused native button must not run them.
                return nil
            }
        }
        return view
    }
    func updateNSView(_ view: Host, context: Context) {
        context.coordinator.enabled = enabled
        context.coordinator.done = done
    }
    static func dismantleNSView(_ view: Host, coordinator: Coordinator) {
        if let monitor = coordinator.monitor { NSEvent.removeMonitor(monitor); coordinator.monitor = nil }
        for observer in coordinator.observers { NotificationCenter.default.removeObserver(observer) }
        coordinator.observers.removeAll()
    }
}
#endif
