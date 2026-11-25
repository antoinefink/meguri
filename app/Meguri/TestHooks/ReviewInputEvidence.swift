#if MEGURI_TEST_HOOKS && os(macOS)
import AppKit
import AppModel
@_spi(MeguriTestHooks) import CardRendering
import CoreBridge
import SwiftUI

extension TestHooks {
    /// Only the authored review-editor recipe installs this probe. It reports
    /// actual audio-controller queue and key-window facts, never field text or media names.
    /// Native queued repeat events exercise the production local monitors.
    struct ReviewInputEvidence: ViewModifier {
        @MainActor final class Source {
            weak var collection: ReviewCollection?
            weak var workspaceView: NSView?
        }
        /// A view in the actual main scene identifies its window even without
        /// a review card. Reading AppKit identity needs no accessibility trust.
        struct WorkspaceWindow: NSViewRepresentable {
            let source: Source
            func makeNSView(context: Context) -> NSView {
                let view = NSView()
                source.workspaceView = view
                return view
            }
            func updateNSView(_ view: NSView, context: Context) { source.workspaceView = view }
        }
        let collection: ReviewCollection?
        let enabled: Bool
        @State private var observer: NSObjectProtocol?
        @State private var source = Source()
        func body(content: Content) -> some View {
            content.background {
                if enabled { WorkspaceWindow(source: source) }
            }.onAppear {
                source.collection = collection
                guard enabled, observer == nil, let bundle = Bundle.main.bundleIdentifier else { return }
                observer = DistributedNotificationCenter.default().addObserver(
                    forName: .init(bundle + ".testhooks.reviewInput"), object: nil, queue: .main
                ) { notification in
                    let command = notification.object as? String ?? "snapshot"
                    MainActor.assumeIsolated {
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
                        let window = NSApp.keyWindow
                        let workspace = source.workspaceView?.window
                        let responder = window?.firstResponder
                        let reviewWindowActive = window != nil && window === pair?.container.window && NSApp.isActive
                        let responderInWindow = responder != nil
                            && (responder === window || (responder as? NSView)?.window === window)
                        let cardFocused = pair.map { ReviewInputOwnership.cardFocused(in: $0.current.webView) } ?? false
                        let focus: String
                        if cardFocused { focus = "card" }
                        else if responder is NSText || responder is NSTextField { focus = "field" }
                        else if reviewWindowActive && responderInWindow { focus = "native" }
                        else { focus = responder == nil ? "none" : "other" }
                        let fields = ["requests": String(audio?.replayRequests ?? 0), "sides": sides,
                                      "live": source.collection == nil ? "false" : "true", "focus": focus,
                                      "firstResponderClass": responder.map { NSStringFromClass($0.classForCoder) } ?? "none",
                                      "workspaceKey": workspace != nil && window === workspace ? "true" : "false",
                                      "collectionIdentity": source.collection.map { String(describing: ObjectIdentifier($0)) } ?? "none",
                                      "collectionGeneration": source.collection.map { String($0.generation) } ?? "none",
                                      "collectionReady": source.collection?.state == .ready ? "true" : "false",
                                      "reviewPhase": source.collection?.review.map { String(describing: $0.phase) } ?? "none",
                                      "reviewPresented": source.collection?.navigation.review == nil ? "false" : "true",
                                      "addWindowKey": NSApp.keyWindow?.identifier?.rawValue == "editor.add.window" ? "true" : "false"]
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
