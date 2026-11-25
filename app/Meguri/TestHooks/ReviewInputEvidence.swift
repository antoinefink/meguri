#if MEGURI_TEST_HOOKS && os(macOS)
import AppKit
import AppModel
@_spi(MeguriTestHooks) import CardRendering
import CoreBridge
import SwiftUI

extension TestHooks {
    /// Only the authored review-editor recipe installs this probe. It reports
    /// actual audio-controller queue facts, never field text or media names.
    /// Native queued repeat events exercise the production local monitors.
    struct ReviewInputEvidence: ViewModifier {
        @MainActor final class Source {
            weak var collection: ReviewCollection?
        }
        let collection: ReviewCollection?
        let enabled: Bool
        @State private var observer: NSObjectProtocol?
        @State private var source = Source()
        func body(content: Content) -> some View {
            content.onAppear {
                source.collection = collection
                guard enabled, observer == nil, let bundle = Bundle.main.bundleIdentifier else { return }
                observer = DistributedNotificationCenter.default().addObserver(
                    forName: .init(bundle + ".testhooks.reviewInput"), object: nil, queue: .main
                ) { notification in
                    let command = notification.object as? String ?? "snapshot"
                    MainActor.assumeIsolated {
                        if command == "focus-review-control", let window = NSApp.keyWindow {
                            // Focus the production AppKit button, never WebKit or
                            // a substitute responder, to prove window-level R.
                            @MainActor func button(in view: NSView) -> NSButton? {
                                if let button = view as? NSButton,
                                   button.accessibilityIdentifier() == "review.session.rate.good" { return button }
                                for child in view.subviews {
                                    if let found = button(in: child) { return found }
                                }
                                return nil
                            }
                            if let content = window.contentView, let control = button(in: content) {
                                window.makeFirstResponder(control)
                            }
                            return
                        }
                        if command != "snapshot", let window = NSApp.keyWindow {
                            let key: (String, UInt16)
                            switch command {
                            case "repeat-space": key = (" ", 49)
                            case "repeat-enter", "stale-enter": key = ("\r", 36)
                            case "repeat-r": key = ("r", 15)
                            default: return
                            }
                            if let event = NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
                                timestamp: command == "stale-enter" ? 0 : ProcessInfo.processInfo.systemUptime,
                                windowNumber: window.windowNumber, context: nil, characters: key.0,
                                charactersIgnoringModifiers: key.0, isARepeat: command != "stale-enter", keyCode: key.1) {
                                NSApp.postEvent(event, atStart: false)
                            }
                            return
                        }
                        let pair = source.collection?.review?.pair
                        let audio = pair?.nativeReviewAudioReadiness
                        let sides = audio?.replaySides.map { $0 == .question ? "q" : "a" }.joined(separator: ",") ?? ""
                        let responder = NSApp.keyWindow?.firstResponder
                        let cardFocused = pair.map { ReviewInputOwnership.cardFocused(in: $0.current.webView) } ?? false
                        let focus: String
                        if cardFocused { focus = "card" }
                        else if responder is NSText || responder is NSTextField { focus = "field" }
                        else if responder is NSControl { focus = "native" }
                        else { focus = responder == nil ? "none" : "other" }
                        let fields = ["requests": String(audio?.replayRequests ?? 0), "sides": sides,
                                      "live": source.collection == nil ? "false" : "true", "focus": focus]
                        let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys])
                        guard let data, let text = String(data: data, encoding: .utf8) else { return }
                        DistributedNotificationCenter.default().postNotificationName(
                            .init(bundle + ".testhooks.reviewInputReply"), object: text, userInfo: nil, deliverImmediately: true)
                    }
                }
            }.onChange(of: collection.map(ObjectIdentifier.init), initial: true) { _, _ in
                source.collection = collection
            }.onDisappear {
                if let observer { DistributedNotificationCenter.default().removeObserver(observer); self.observer = nil }
            }
        }
    }
}
#endif
