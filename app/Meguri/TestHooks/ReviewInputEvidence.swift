#if MEGURI_TEST_HOOKS && os(macOS)
import AppKit
import AppModel
@_spi(MeguriTestHooks) import CardRendering
import CoreBridge
import SwiftUI

extension TestHooks {
    /// Only the authored review-editor and empty-collection journeys install this probe. It reports
    /// actual audio-controller queue and key-window facts, never field text or media names.
    /// Native queued repeat events exercise the production local monitors.
    struct ReviewInputEvidence: ViewModifier {
        @MainActor final class Source {
            weak var collection: ReviewCollection?
            weak var workspaceView: NSView?
            var focusedContext: NativeCommandContext?
        }
        /// A view in the actual main scene identifies its window even without
        /// a review card. Reading AppKit identity needs no accessibility trust.
        struct WorkspaceWindow: NSViewRepresentable {
            let source: Source
            let focusedContext: NativeCommandContext?
            func makeNSView(context: Context) -> NSView {
                let view = NSView()
                source.workspaceView = view
                return view
            }
            func updateNSView(_ view: NSView, context: Context) {
                source.workspaceView = view
                source.focusedContext = focusedContext
            }
        }
        let collection: ReviewCollection?
        let enabled: Bool
        @State private var observer: NSObjectProtocol?
        @State private var source = Source()
        @FocusedValue(\.nativeCommandContext) private var focusedContext
        func body(content: Content) -> some View {
            content.background {
                if enabled { WorkspaceWindow(source: source, focusedContext: focusedContext) }
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
                        var fields: [String: String] = [:]
                        fields["requests"] = String(audio?.replayRequests ?? 0)
                        fields["sides"] = sides
                        fields["live"] = source.collection == nil ? "false" : "true"
                        fields["focus"] = focus
                        fields["firstResponderClass"] = responder.map { NSStringFromClass($0.classForCoder) } ?? "none"
                        fields["workspaceKey"] = workspace != nil && window === workspace ? "true" : "false"
                        // Exact identifiers, not NSApp.mainWindow (Add can be main).
                        let main = NSApp.windows.first { $0.identifier?.rawValue == "main" && $0.isVisible }
                        let add = NSApp.windows.first { $0.identifier?.rawValue == "editor.add.window" && $0.isVisible }
                        fields["workspaceIdentifier"] = workspace?.identifier?.rawValue ?? "none"
                        fields["workspaceMatchesMain"] = main != nil && workspace === main ? "true" : "false"
                        fields["addWindowVisible"] = add == nil ? "false" : "true"
                        fields["collectionIdentity"] = source.collection.map { String(describing: ObjectIdentifier($0)) } ?? "none"
                        fields["collectionGeneration"] = source.collection.map { String($0.generation) } ?? "none"
                        fields["collectionReady"] = source.collection?.state == .ready ? "true" : "false"
                        fields["addWindowKey"] = NSApp.keyWindow?.identifier?.rawValue == "editor.add.window" ? "true" : "false"
                        let context = source.focusedContext
                        let review = source.collection?.review
                        let flow = source.collection?.editNoteFlow
                        fields["focusedHost"] = context.map { String(describing: $0.host) } ?? "none"
                        fields["focusedCollectionMatches"] = String(context?.collection === source.collection)
                        fields["registeredReviewInput"] = String(context?.reviewAvailability?.input == true)
                        fields["registeredEditNote"] = String(context?.reviewAvailability?.editNote == true)
                        fields["liveReviewInput"] = String(context?.reviewInputAvailable == true)
                        fields["reviewPhase"] = review.map { String(describing: $0.phase) } ?? "none"
                        fields["reviewPaused"] = String(review?.paused == true)
                        fields["reviewEditingNote"] = String(review?.editingNote == true)
                        fields["reviewCanEditNote"] = String(review?.canEditNote == true)
                        fields["reviewCanActOnCard"] = String(review?.canActOnCard == true)
                        fields["reviewCanRefresh"] = String(review?.canRefresh == true)
                        fields["reviewCanFinish"] = String(review?.canFinish == true)
                        fields["reviewIsActive"] = String(source.collection?.reviewIsActive == true)
                        fields["editCanOpen"] = String(flow?.canOpen == true)
                        fields["editPresented"] = String(flow?.isPresented == true)
                        fields["editOpening"] = String(flow?.opening == true)
                        fields["editSaving"] = String(flow?.saving == true)
                        fields["editGuardingClose"] = String(flow?.guardingClose == true)
                        fields["sheetPresented"] = String(source.collection?.navigation.sheet != nil)
                        fields["keyWindowIdentifier"] = window?.identifier?.rawValue ?? "none"
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
