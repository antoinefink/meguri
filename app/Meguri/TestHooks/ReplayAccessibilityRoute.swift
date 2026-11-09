#if DEBUG && MEGURI_TEST_HOOKS && os(macOS)
import CardRendering
import CoreBridge
import Foundation
import QuartzCore
import SwiftUI
import WebKit

extension TestHooks {
    enum ReplayAccessibilityRoute {
        static func register(in registry: inout RouteRegistry) {
            registry.register("card/replay-accessibility") { _ in .screen { ReplayAccessibilityView() } }
        }
    }

    final class ReplayTransportObserver: NSObject, WKScriptMessageHandlerWithReply {
        let bridge: CardBridge
        var forwardingCommand: String?
        var received: [String] = []

        init(bridge: CardBridge) { self.bridge = bridge; super.init() }

        func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage,
                                   replyHandler: @escaping @MainActor @Sendable (Any?, String?) -> Void) {
            let body = message.body as? [String: Any]
            let command = body?["signal"] as? String == "command" ? body?["command"] as? String : nil
            forwardingCommand = command
            if let command { received.append(command) }
            defer { forwardingCommand = nil }
            bridge.userContentController(userContentController, didReceive: message, replyHandler: replyHandler)
        }
    }

    @Observable final class ReplayAccessibilityModel {
        var pair: CardPagePair?
        private var operationReady = false
        private var shown: CardUpdate?
        var ready: Bool { operationReady && shown != nil && shown == pair?.current.update }
        var failure = ""
        var checkpoint = 0
        var commands: [String] = []
        var actions: [String] = []
        var unmatchedActions = 0
        private var observer: ReplayTransportObserver?
        private var root: URL?
        private var work: Task<Void, Never>?
        private var card: Int64 = 0

        var report: String {
            let value: [String: Any] = [
                "ready": ready, "failure": failure, "checkpoint": checkpoint,
                "card": pair?.current.update?.card ?? 0,
                "side": pair?.current.update?.sideName ?? "none",
                "catalogLabel": CardRuntime.replayLabel,
                "received": observer?.received ?? [], "commands": commands,
                "actions": actions,
                "unmatchedActions": unmatchedActions
            ]
            return (try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]))
                .flatMap { String(data: $0, encoding: .utf8) } ?? "unavailable"
        }

        func start() {
            guard work == nil else { return }
            perform { [self] in
                let root = FileManager.default.temporaryDirectory.appending(path: "replay-accessibility-\(UUID())", directoryHint: .isDirectory)
                self.root = root
                try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
                let resolver = try await MediaPathResolver.open(root: root, trackedNames: [])
                try Task.checkCancellation()
                let configuration = CardWebConfiguration(store: .nonPersistent())
                let observer = ReplayTransportObserver(bridge: configuration.bridge)
                self.observer = observer
                let controller = configuration.configuration.userContentController
                let world = WKContentWorld.world(name: CardRuntime.interactionWorldName)
                controller.removeScriptMessageHandler(forName: CardRuntime.signalHandlerName, contentWorld: world)
                controller.addScriptMessageHandler(observer, contentWorld: world, name: CardRuntime.signalHandlerName)
                let pair = CardPagePair(configuration: configuration, resolver: resolver, appearance: .light)
                self.pair = pair
                pair.onSignal = { [weak model = self, weak pair] signal, update in
                    if signal == "shown", pair?.current.update == update { model?.shown = update }
                }
                pair.onAction = { [weak model = self, weak observer] action in
                    guard let model else { return }
                    guard case let .play(side, index) = action, let command = observer?.forwardingCommand else {
                        model.unmatchedActions += 1
                        return
                    }
                    model.commands.append(command)
                    model.actions.append("play:\(side == .question ? "q" : "a"):\(index)")
                }
                try await pair.load()
                try await self.question()
            }
        }

        // Synthetic D4 display output, as in the existing package fixture. The
        // real CardRuntime installs/localizes it from explicit ownership IDs.
        // No collection, account, media playback or authored style is changed.
        private func control(id: String, command: String) -> String {
            """
            <a class="replay-button soundLink" id="\(id)" data-meguri-replay="true" aria-label="Play audio" href=# onclick="pycmd('\(command)'); return false;" draggable="false"><svg class="playImage" aria-hidden="true" focusable="false" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>
            """
        }

        private func question() async throws {
            guard let pair else { throw CardPage.Failure.notPrepared }
            card += 1
            let front = ["play:q:00", "play:q:1", "play:q:00"]
            let back = front + front + ["play:a:000"]
            let questionIDs = front.indices.map { "review.card.replay.q\($0 + 1)" }
            let answerIDs = back.indices.map { "review.card.replay.a\($0)" }
            // q0 is authored: keep its matching classes, marker, ID, name and
            // style. It is deliberately excluded from generated ownership IDs.
            let authored = """
            <a class="replay-button soundLink" id="review.card.replay.q0" data-meguri-replay="true" aria-label="Author &amp; 猫" href="#own" style="color:purple">own</a>
            """
            let questionHTML = authored + zip(questionIDs, front).map { control(id: $0.0, command: $0.1) }.joined()
            let answerHTML = zip(answerIDs, back).map { control(id: $0.0, command: $0.1) }.joined()
            let payload = CardPayload(cardID: card, question: questionHTML, answer: answerHTML,
                questionReplayIDs: questionIDs, answerReplayIDs: answerIDs,
                questionAudio: [.sound(name: "synthetic-first.wav"), .sound(name: "synthetic-second.wav")],
                answerAudio: [.sound(name: "synthetic-answer.wav")], audioPolicy: .init(autoplay: false))
            let update = try await pair.prepare(payload)
            CATransaction.begin()
            _ = try pair.present(update)
            CATransaction.commit()
        }

        func next() { perform { try await self.question() } }
        func answer() { perform { _ = try await self.pair?.showAnswer() } }

        /// A round trip through the same isolated transport follows prior native
        /// presses. Await its reply before publishing cumulative command counts;
        /// no delay or counter increment is used to simulate an activation.
        func takeCheckpoint() {
            perform {
                guard let page = self.pair?.current, let update = page.update else { throw CardPage.Failure.notPrepared }
                _ = try await page.webView.callAsyncJavaScript("""
                    await window.webkit.messageHandlers[handler].postMessage({
                        signal: 'listeners', card, generation, side, tap: false, key: false
                    });
                    """, arguments: ["handler": CardRuntime.signalHandlerName, "card": update.card,
                                      "generation": update.generation, "side": update.sideName],
                    in: nil, contentWorld: .world(name: CardRuntime.interactionWorldName))
                self.checkpoint += 1
            }
        }

        private func perform(_ operation: @escaping @MainActor () async throws -> Void) {
            operationReady = false
            work = Task {
                do { try await operation(); operationReady = true }
                catch { if !Task.isCancelled { failure = "fixture operation failed" } }
            }
        }

        func stop() {
            work?.cancel(); work = nil
            pair?.close(); pair = nil; observer = nil
            if let root { try? FileManager.default.removeItem(at: root) }
            root = nil
        }
    }

    struct ReplayAccessibilityView: View {
        @State private var model = ReplayAccessibilityModel()

        var body: some View {
            VStack {
                HStack {
                    Button("Question") { model.next() }
                        .accessibilityIdentifier("testhooks.replay.question")
                    Button("Answer") { model.answer() }
                        .accessibilityIdentifier("testhooks.replay.answer")
                    Button("Checkpoint") { model.takeCheckpoint() }
                        .accessibilityIdentifier("testhooks.replay.checkpoint")
                }
                .disabled(!model.ready)
                Text("Synthetic replay fixture")
                    .accessibilityIdentifier("testhooks.replay.report")
                    .accessibilityValue(model.report)
                if let pair = model.pair { CardWebView(pair: pair) }
            }
            .onAppear { model.start() }
            .onDisappear { model.stop() }
        }
    }
}
#endif
