import AppModel
import CardRendering
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// Hosts the existing pair and its native behavior presenter. The pair is
/// moved between layout hosts; no layout host creates or closes its pages.
struct ReviewCardHost {
    let model: ReviewCoordinator
    let pair: CardPagePair
    let close: () -> Void
    /// N20.8: Edit Note, Bury Card and Suspend Card, for their keys (E, -, @)
    /// and the Mac card's context menu.
    var actions = ReviewMoreActions()
    /// How much of the card's bottom the answer area covers (N16.9).
    var obscuredBottom: CGFloat = 0
    /// How much of the card's top the bars cover (N16.12).
    var obscuredTop: CGFloat = 0

    private func key(_ input: String, command: Bool, shift: Bool, repeated: Bool = false) -> Bool {
        guard model.phase == .question || model.phase == .answer else { return false }
        let focused: Bool
        #if os(macOS)
        focused = !command && !shift && input.lowercased() == "r"
            ? ReviewInputOwnership.replayFocused(in: pair.current.webView)
            : ReviewInputOwnership.cardFocused(in: pair.current.webView)
        #else
        focused = ReviewInputOwnership.cardFocused(in: pair.current.webView)
        #endif
        guard let pages = model.surface as? ReviewPages, pages.presenter.isAvailable,
              ReviewInputOwnership.admits(cardFocused: focused,
                  // An open Edit Note form owns the keys like a sheet (N11.1, E2).
                  editing: model.surface.editing, paused: model.paused || model.editingNote, repeated: repeated) else { return false }
        if command {
            if input.lowercased() == "z" { model.restore(redo: shift); return true }
            return false
        }
        switch input {
        case " ", "\r", "\n":
            if model.phase == .question { model.reveal() }
            else if model.phase == .answer { model.rate(.good) }
            return true
        case "r", "R":
            guard !shift else { return false }
            pair.replay()
            return true
        case "1": model.rate(.again); return true
        case "2": model.rate(.hard); return true
        case "3": model.rate(.good); return true
        case "4": model.rate(.easy); return true
        case "\u{1b}": close(); return true
        // review/commands: E, - and @ on either side, as the More menu (N20.8).
        case "e", "E": if actions.canEditNote { actions.editNote() }; return true
        case "-": if actions.canActOnCard { actions.act(.bury) }; return true
        case "@": if actions.canActOnCard { actions.act(.suspend) }; return true
        default: return false
        }
    }
}

#if os(macOS)
extension ReviewCardHost: NSViewControllerRepresentable {
    final class Host: NSViewController {
        var attached: (() -> Void)?
        override func viewDidAppear() { super.viewDidAppear(); attached?() }
    }
    final class Coordinator {
        var monitor: Any?
        var focusObserver: NSObjectProtocol?
        var menuTrackingDepth = 0
        var menuObservers: [NSObjectProtocol] = []
        isolated deinit {
            if let monitor { NSEvent.removeMonitor(monitor) }
            if let focusObserver { NotificationCenter.default.removeObserver(focusObserver) }
            for observer in menuObservers { NotificationCenter.default.removeObserver(observer) }
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSViewController(context: Context) -> Host {
        let host = Host()
        host.attached = { [weak host, weak model] in
            (model?.surface as? ReviewPages)?.presenter.window = host?.view.window
            (model?.surface as? ReviewPages)?.establishInitialFocus()
        }
        attach(to: host)
        context.coordinator.focusObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.didUpdateNotification, object: nil, queue: .main
        ) { [weak host, weak pair] note in
            guard let updatedWindow = note.object as? NSWindow else { return }
            let updatedWindowID = ObjectIdentifier(updatedWindow)
            MainActor.assumeIsolated {
                guard let host, let pair, let window = host.view.window,
                      ObjectIdentifier(window) == updatedWindowID else { return }
                (model.surface as? ReviewPages)?.establishInitialFocus()
                let owned = !model.paused && ReviewInputOwnership.cardFocused(in: pair.current.webView)
                if pair.ownsKeyboardFocus != owned { pair.ownsKeyboardFocus = owned }
            }
        }
        let owner = context.coordinator
        for (name, tracking) in [(NSMenu.didBeginTrackingNotification, true), (NSMenu.didEndTrackingNotification, false)] {
            owner.menuObservers.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak owner] _ in
                MainActor.assumeIsolated {
                    guard let owner else { return }
                    owner.menuTrackingDepth = tracking ? owner.menuTrackingDepth + 1 : max(0, owner.menuTrackingDepth - 1)
                }
            })
        }
        owner.monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak host, weak owner] event in
            guard let host, host.view.window != nil, event.window === host.view.window,
                  owner?.menuTrackingDepth == 0 else { return event }
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            guard flags.isDisjoint(with: [.option, .control]) else { return event }
            let replay = event.charactersIgnoringModifiers?.lowercased() == "r"
            if replay, !flags.subtracting(.capsLock).isEmpty { return event }
            guard replay || ReviewInputOwnership.cardFocused(in: pair.current.webView) else { return event }
            return key(event.charactersIgnoringModifiers ?? "", command: flags.contains(.command),
                       shift: flags.contains(.shift), repeated: event.isARepeat) ? nil : event
        }
        return host
    }
    func updateNSViewController(_ host: Host, context: Context) { attach(to: host) }
    static func dismantleNSViewController(_ host: Host, coordinator: Coordinator) {
        if let monitor = coordinator.monitor { NSEvent.removeMonitor(monitor); coordinator.monitor = nil }
        if let observer = coordinator.focusObserver { NotificationCenter.default.removeObserver(observer); coordinator.focusObserver = nil }
        for observer in coordinator.menuObservers { NotificationCenter.default.removeObserver(observer) }
        coordinator.menuObservers.removeAll()
    }
    private func attach(to host: Host) {
        // Expose WebKit through the same direct container boundary as the behavior host.
        pair.container.setAccessibilityIdentifier("review.session.card")
        if !host.isViewLoaded || host.view !== pair.container {
            pair.container.removeFromSuperview()
            host.view = pair.container
        }
        (model.surface as? ReviewPages)?.presenter.window = host.view.window
        (model.surface as? ReviewPages)?.establishInitialFocus()
        if pair.obscuredBottom != obscuredBottom { pair.obscuredBottom = obscuredBottom }
        if pair.obscuredTop != obscuredTop { pair.obscuredTop = obscuredTop }
        // N20.8: the card's context menu starts with Edit Note | Bury Card, Suspend Card.
        let actions = actions
        for page in pair.pages {
            page.webView.contextActions = { ReviewCardMenuItems.make(actions) }
        }
    }
}

/// The Mac card's context menu items, with the Card menu's key equivalents shown.
@MainActor enum ReviewCardMenuItems {
    static func make(_ actions: ReviewMoreActions) -> [NSMenuItem] {
        func item(_ title: String, _ symbol: String, _ key: String, _ modifiers: NSEvent.ModifierFlags,
                  enabled: Bool, _ run: @escaping () -> Void) -> NSMenuItem {
            let target = ClosureMenuTarget(run)
            let item = NSMenuItem(title: title, action: #selector(ClosureMenuTarget.runAction), keyEquivalent: "")
            item.target = target; item.representedObject = target
            item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: nil)
            item.keyEquivalent = key; item.keyEquivalentModifierMask = modifiers
            item.isEnabled = enabled
            return item
        }
        return [
            item(String(localized: "Edit Note", bundle: .module), "pencil", "e", .command, enabled: actions.canEditNote, actions.editNote),
            .separator(),
            item(String(localized: "Bury Card", bundle: .module), "moon", "j", .command, enabled: actions.canActOnCard) { actions.act(.bury) },
            item(String(localized: "Suspend Card", bundle: .module), "pause.circle", "j", [.command, .shift],
                 enabled: actions.canActOnCard) { actions.act(.suspend) },
        ]
    }
}

/// The target of a context menu item that runs a closure; the item retains it.
final class ClosureMenuTarget: NSObject {
    private let run: () -> Void
    init(_ run: @escaping () -> Void) { self.run = run; super.init() }
    @objc func runAction() { run() }
}
#else
extension ReviewCardHost: UIViewControllerRepresentable {
    final class Host: UIViewController {
        var handle: ((String, Bool, Bool) -> Bool)?
        var focusChanged: (() -> Void)?
        override func viewDidAppear(_ animated: Bool) {
            super.viewDidAppear(animated)
            focusChanged?()
        }
        override func didUpdateFocus(in context: UIFocusUpdateContext, with coordinator: UIFocusAnimationCoordinator) {
            super.didUpdateFocus(in: context, with: coordinator)
            focusChanged?()
        }
        override func viewWillDisappear(_ animated: Bool) {
            super.viewWillDisappear(animated)
            focusChanged?()
        }
        override var keyCommands: [UIKeyCommand]? {
            [" ", "\r", "1", "2", "3", "4", "r", "e", "-", "@", UIKeyCommand.inputEscape].map {
                let command = UIKeyCommand(input: $0, modifierFlags: [], action: #selector(pressed(_:)))
                command.wantsPriorityOverSystemBehavior = false
                if $0 == "r" { command.repeatBehavior = .nonRepeatable }
                return command
            } + [UIKeyCommand(input: "z", modifierFlags: .command, action: #selector(pressed(_:))),
                 UIKeyCommand(input: "z", modifierFlags: [.command, .shift], action: #selector(pressed(_:)))]
        }
        @objc private func pressed(_ command: UIKeyCommand) {
            _ = handle?(command.input ?? "", command.modifierFlags.contains(.command), command.modifierFlags.contains(.shift))
        }
        override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
            if action == #selector(pressed(_:)), let command = sender as? UIKeyCommand {
                // Native controls and editable DOM keep their own key commands.
                return canHandle?(command) == true
            }
            return super.canPerformAction(action, withSender: sender)
        }
        var canHandle: ((UIKeyCommand) -> Bool)?
        /// N16.9: SwiftUI's `safeAreaBar` registers the answer area with the
        /// page's scroll view, which draws the edge effect under its buttons;
        /// the soft style only, as the hard one draws a line at its top edge.
        /// N16.12: no edge effect at all where the card passes under the top
        /// bars: the card stays sharp up to the screen's edge.
        func softEdges(_ scrollViews: [UIScrollView]) {
            for scrollView in scrollViews {
                if scrollView.bottomEdgeEffect.style != .soft { scrollView.bottomEdgeEffect.style = .soft }
                if !scrollView.topEdgeEffect.isHidden { scrollView.topEdgeEffect.isHidden = true }
            }
        }
    }
    func makeUIViewController(context: Context) -> Host {
        let host = Host()
        host.handle = { key($0, command: $1, shift: $2) }
        host.canHandle = { _ in
            ReviewInputOwnership.admits(cardFocused: ReviewInputOwnership.cardFocused(in: pair.current.webView),
                editing: model.surface.editing, paused: model.paused || model.editingNote, repeated: false)
        }
        host.focusChanged = {
            (model.surface as? ReviewPages)?.establishInitialFocus()
            let owned = !model.paused && ReviewInputOwnership.cardFocused(in: pair.current.webView)
            if pair.ownsKeyboardFocus != owned { pair.ownsKeyboardFocus = owned }
        }
        attach(to: host)
        return host
    }
    func updateUIViewController(_ host: Host, context: Context) { attach(to: host) }
    private func attach(to host: Host) {
        if pair.container.superview !== host.view {
            pair.container.removeFromSuperview()
            pair.container.frame = host.view.bounds
            pair.container.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            host.view.addSubview(pair.container)
        }
        (model.surface as? ReviewPages)?.presenter.host = host
        (model.surface as? ReviewPages)?.establishInitialFocus()
        if pair.obscuredBottom != obscuredBottom { pair.obscuredBottom = obscuredBottom }
        if pair.obscuredTop != obscuredTop { pair.obscuredTop = obscuredTop }
        host.softEdges(pair.pages.map(\.webView.scrollView))
    }
}
#endif
