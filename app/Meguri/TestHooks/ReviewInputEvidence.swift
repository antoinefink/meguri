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
        let collection: ReviewCollection?
        let enabled: Bool
        @State private var observer: NSObjectProtocol?
        func body(content: Content) -> some View {
            content.onAppear {
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
                        let audio = collection?.review?.pair?.nativeReviewAudioReadiness
                        let sides = audio?.replaySides.map { $0 == .question ? "q" : "a" }.joined(separator: ",") ?? ""
                        let fields = ["requests": String(audio?.replayRequests ?? 0), "sides": sides]
                        let data = try? JSONSerialization.data(withJSONObject: fields, options: [.sortedKeys])
                        guard let data, let text = String(data: data, encoding: .utf8) else { return }
                        DistributedNotificationCenter.default().postNotificationName(
                            .init(bundle + ".testhooks.reviewInputReply"), object: text, userInfo: nil, deliverImmediately: true)
                    }
                }
            }.onDisappear {
                if let observer { DistributedNotificationCenter.default().removeObserver(observer); self.observer = nil }
            }
        }
    }
}
#endif
