@testable import CardRendering
import CoreBridge
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite struct BridgeAllowlistTests {
        @Test func strictTypesWorldsIdentitiesAndIdleAdmission() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved(), platform: .mac)
            defer { pair.close() }
            let page = pair.idle
            let payload = CardPayload(cardID: 7, question: "q", answer: "a", questionAudio: [.tts(language: "x", content: "silent"), .sound(name: "one.mp3")])
            let old = CardUpdate(card: 7, generation: 1, side: .question)
            page.begin(payload, update: old)
            var actions: [CardBridgeAction] = []
            var signals: [String] = []
            page.onAction = { _, action in actions.append(action) }
            page.onSignal = { _, signal, _ in signals.append(signal) }
            func send(_ values: [String: Any], world: String? = CardRuntime.interactionWorldName) {
                configuration.bridge.receive(name: "meguriCard", world: world, body: values, page: page)
            }
            let base: [String: Any] = ["card": 7, "generation": 1, "side": "question"]
            func body(_ signal: String, _ extra: [String: Any] = [:]) -> [String: Any] {
                base.merging(["signal": signal], uniquingKeysWith: { _, b in b }).merging(extra, uniquingKeysWith: { _, b in b })
            }
            send(body("command", ["command": "ans"])) // retained, never old-card action
            send(body("focus", ["editable": true, "typeans": true]))
            #expect(actions.isEmpty && signals.isEmpty)
            send(body("settled"))
            #expect(page.settled)
            let newer = CardUpdate(card: 7, generation: 2, side: .question)
            page.begin(payload, update: newer)
            send(body("settled"))
            #expect(!page.settled)
            for invalid in [body("nativeOpen", ["path": "/secret"]), body("settled", ["generation": true]),
                            body("settled", ["card": 7.5]), body("settled", ["generation": Double.infinity]),
                            body("focus", ["editable": "true", "typeans": true]), body("typedText", ["text": String(repeating: "x", count: 65_537)])] {
                send(invalid)
            }
            send(body("command", ["command": "ans", "generation": 2]), world: WKContentWorld.page.name)
            #expect(actions.isEmpty && !page.settled)
            #expect(CardBridge.command("play:q:0001", payload: payload) == .play(.question, 1))
            for invalid in ["play:q:-1", "play:q:1.0", "play:q:٢", "play:q:2", "play:x:0", "play:q:999999999999999999999999999", "ease3"] {
                #expect(CardBridge.command(invalid, payload: payload) == nil)
            }
            #expect(!pair.diagnostics.entries.isEmpty)
            send(body("error", ["generation": 2, "kind": "exception", "phase": "script", "index": 1,
                                "line": 3, "column": 4, "source": "file:///private/synthetic.txt", "message": "TypeError: synthetic-private-text"]))
            let error = try #require(pair.diagnostics.entries.last?.script)
            #expect(error.errorClass == "TypeError" && error.line == 3 && error.column == 4)
            #expect(error.source == "external-resource")
            for index in 0..<300 { pair.diagnostics.record("missingMedia", update: newer, name: "logical\(index).png") }
            #expect(pair.diagnostics.entries.count == 256)
        }

        @Test func exactIOSLegacyProtocols() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved(), platform: .iPhone)
            defer { pair.close() }
            let page = pair.current
            page.begin(CardPayload(cardID: 1, question: "q", answer: "a", answerAudio: [.sound(name: "a.mp3")]),
                       update: CardUpdate(card: 1, generation: 1, side: .question))
            var actions: [CardBridgeAction] = []
            page.onAction = { _, action in actions.append(action) }
            // Drive the same post-commit admission path without JS timing.
            page.visibleToken = 1
            let window = SpikeWindow(pair.container)
            defer { window.close() }
            try await pair.load()
            try await page.present(token: 1)
            for zone in CardBridge.zones {
                configuration.bridge.receive(name: "cb", world: WKContentWorld.page.name,
                                             body: "{\"scheme\":\"ankitap\",\"msg\":\"\(zone)\"}", page: page)
            }
            #expect(actions == Array(repeating: .reveal, count: 9))
            actions.removeAll()
            let payload = try #require(page.payload)
            page.begin(payload, update: CardUpdate(card: 1, generation: 2, side: .answer))
            for zone in ["topLeft", "midLeft", "bottomLeft", "topRight", "midRight", "bottomRight", "topCenter", "midCenter", "bottomCenter"] {
                configuration.bridge.receive(name: "cb", world: WKContentWorld.page.name,
                                             body: "{\"scheme\":\"ankitap\",\"msg\":\"\(zone)\"}", page: page)
            }
            #expect(actions == [.again, .again, .again, .good, .good, .good])
            for value: Any in ["{\"scheme\":\"other\",\"msg\":\"midCenter\"}", "{\"scheme\":\"ankitap\",\"msg\":\"middleCenter\"}", ["scheme": "ankitap", "msg": "topLeft"]] {
                configuration.bridge.receive(name: "cb", world: WKContentWorld.page.name, body: value, page: page)
            }
            configuration.bridge.receive(name: "meguri", world: WKContentWorld.page.name, body: "play:a:000", page: page)
            #expect(actions.last == .play(.answer, 0) && actions.count == 7)
        }
    }
}
