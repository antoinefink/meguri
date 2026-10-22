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

    private func key(_ input: String, command: Bool, shift: Bool, repeated: Bool = false) -> Bool {
        guard let pages = model.surface as? ReviewPages, pages.presenter.isAvailable,
              ReviewInputOwnership.admits(cardFocused: ReviewInputOwnership.cardFocused(in: pair.current.webView),
                  editing: model.surface.editing, paused: model.paused, repeated: repeated) else { return false }
        if command {
            if input.lowercased() == "z" { model.restore(redo: shift); return true }
            return false
        }
        switch input {
        case " ", "\r", "\n":
            if model.phase == .question { model.reveal() }
            else if model.phase == .answer { model.rate(.good) }
            return true
        case "1": model.rate(.again); return true
        case "2": model.rate(.hard); return true
        case "3": model.rate(.good); return true
        case "4": model.rate(.easy); return true
        case "\u{1b}": close(); return true
        default: return false
        }
    }
}

#if os(macOS)
extension ReviewCardHost: NSViewRepresentable {
    final class Host: NSView {
        var attached: (() -> Void)?
        override func viewDidMoveToWindow() { super.viewDidMoveToWindow(); attached?() }
    }
    final class Coordinator {
        var monitor: Any?
        var focusObserver: NSObjectProtocol?
        isolated deinit {
            if let monitor { NSEvent.removeMonitor(monitor) }
            if let focusObserver { NotificationCenter.default.removeObserver(focusObserver) }
        }
    }
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeNSView(context: Context) -> Host {
        let host = Host()
        host.attached = { [weak host, weak model] in
            (model?.surface as? ReviewPages)?.presenter.window = host?.window
            (model?.surface as? ReviewPages)?.establishInitialFocus()
        }
        attach(to: host)
        context.coordinator.focusObserver = NotificationCenter.default.addObserver(
            forName: NSWindow.didUpdateNotification, object: nil, queue: .main
        ) { [weak host, weak pair] note in
            MainActor.assumeIsolated {
                guard let host, let pair, let window = note.object as? NSWindow, window === host.window else { return }
                (model.surface as? ReviewPages)?.establishInitialFocus()
                let owned = !model.paused && ReviewInputOwnership.cardFocused(in: pair.current.webView)
                if pair.ownsKeyboardFocus != owned { pair.ownsKeyboardFocus = owned }
            }
        }
        context.coordinator.monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak host] event in
            guard let host, host.window != nil, event.window === host.window,
                  ReviewInputOwnership.cardFocused(in: pair.current.webView) else { return event }
            let flags = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            guard flags.isDisjoint(with: [.option, .control]) else { return event }
            return key(event.charactersIgnoringModifiers ?? "", command: flags.contains(.command),
                       shift: flags.contains(.shift), repeated: event.isARepeat) ? nil : event
        }
        return host
    }
    func updateNSView(_ host: Host, context: Context) { attach(to: host) }
    static func dismantleNSView(_ host: Host, coordinator: Coordinator) {
        if let monitor = coordinator.monitor { NSEvent.removeMonitor(monitor); coordinator.monitor = nil }
        if let observer = coordinator.focusObserver { NotificationCenter.default.removeObserver(observer); coordinator.focusObserver = nil }
    }
    private func attach(to host: Host) {
        if pair.container.superview !== host {
            pair.container.removeFromSuperview()
            pair.container.frame = host.bounds
            pair.container.autoresizingMask = [.width, .height]
            host.addSubview(pair.container)
        }
        (model.surface as? ReviewPages)?.presenter.window = host.window
        (model.surface as? ReviewPages)?.establishInitialFocus()
    }
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
            [" ", "\r", "1", "2", "3", "4", UIKeyCommand.inputEscape].map {
                let command = UIKeyCommand(input: $0, modifierFlags: [], action: #selector(pressed(_:)))
                command.wantsPriorityOverSystemBehavior = false
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
    }
    func makeUIViewController(context: Context) -> Host {
        let host = Host()
        host.handle = { key($0, command: $1, shift: $2) }
        host.canHandle = { _ in
            ReviewInputOwnership.admits(cardFocused: ReviewInputOwnership.cardFocused(in: pair.current.webView),
                editing: model.surface.editing, paused: model.paused, repeated: false)
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
    }
}
#endif
