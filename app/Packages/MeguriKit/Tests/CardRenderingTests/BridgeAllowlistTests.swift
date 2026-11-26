@testable import CardRendering
import CoreBridge
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    @Suite struct BridgeAllowlistTests {
        // Compile the production JSON on Mac too, even though only iOS
        // installs it. NSRegularExpression accepts syntax WebKit rejects.
        @Test func productionLoopbackRulesCompileInWebKit() async throws {
            let source = try CardWebConfiguration.loopbackRules
            let store = try #require(WKContentRuleListStore.default())
            let identifier = "meguri.test.loopback.\(UUID().uuidString)"
            let list: WKContentRuleList = try await withCheckedThrowingContinuation { continuation in
                store.compileContentRuleList(forIdentifier: identifier, encodedContentRuleList: source) { list, error in
                    if let error { continuation.resume(throwing: error) }
                    else if let list { continuation.resume(returning: list) }
                    else { continuation.resume(throwing: URLError(.cannotLoadFromNetwork)) }
                }
            }
            #expect(list.identifier == identifier)
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                store.removeContentRuleList(forIdentifier: identifier) { error in
                    if let error { continuation.resume(throwing: error) }
                    else { continuation.resume() }
                }
            }
        }

        @Test func loopbackRulesMatchHostsWithoutBlockingRemoteURLText() throws {
            let source = try CardWebConfiguration.loopbackRules
            let rules = try #require(JSONSerialization.jsonObject(with: Data(source.utf8)) as? [[String: Any]])
            let expressions = try rules.map { rule in
                let trigger = try #require(rule["trigger"] as? [String: Any])
                let filter = try #require(trigger["url-filter"] as? String)
                #expect(trigger["url-filter-is-case-sensitive"] as? Bool == false)
                return try NSRegularExpression(pattern: filter, options: .caseInsensitive)
            }
            // Fixed URL facts, independent of the production patterns. Numeric
            // IPv4 spellings canonicalize to 127/8 before rule matching; the
            // actual iOS fetch regression also exercises those raw spellings.
            let cases: [(url: String, blocked: Bool)] = [
                ("http://localhost/", true),
                ("https://LOCALHOST.:8765/", true),
                ("https://user:password@one.two.localhost.:443/path", true),
                ("http://one..localhost/", true),
                ("http://.localhost/", true),
                ("http://127.0.0.1:8765/", true),
                ("https://user:password@127.255.255.254/", true),
                ("http://127.1/", true),
                ("http://[::1]/", true),
                ("https://user:password@[0:0:0:0:0:0:0:1]:443/", true),
                ("http://[::ffff:127.0.0.1]/", true),
                ("https://[::ffff:7f00:1]:8765/", true),
                ("http://[::ffff:7fff:fffe]/", true),
                ("http://example.com/", false),
                ("https://localhost.example.com/", false),
                ("http://notlocalhost/", false),
                ("https://127.0.0.1.example.com/", false),
                ("http://localhost@example.com/", false),
                ("http://127.0.0.1:8765@example.com/", false),
                ("https://user:localhost@[2001:db8::1]/", false),
                ("http://[::1]@example.com/", false),
                ("https://example.com/path/localhost?url=http://127.0.0.1/", false),
                ("http://126.255.255.255/", false),
                ("http://128.0.0.1/", false),
                ("http://192.168.1.1:8765/", false),
                ("http://[::2]/", false),
                ("http://[::ffff:7e00:1]/", false),
                ("http://[::ffff:8000:1]/", false),
                ("ftp://localhost/", false),
            ]
            for testCase in cases {
                let range = NSRange(testCase.url.startIndex..<testCase.url.endIndex, in: testCase.url)
                let blocked = expressions.contains { $0.firstMatch(in: testCase.url, range: range) != nil }
                #expect(blocked == testCase.blocked, "\(testCase.url)")
            }
        }

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

        @Test func mathDiagnosticsAdmitOnlyTheTypesetPairWithExistingLimits() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved(), platform: .mac)
            defer { pair.close() }
            let page = pair.idle
            let update = CardUpdate(card: 7, generation: 1, side: .answer)
            page.begin(CardPayload(cardID: 7, ordinal: 2, templateDescriptor: "Math card", question: "q", answer: "a"), update: update)
            let base: [String: Any] = ["signal": "error", "card": 7, "generation": 1, "side": "answer",
                                       "kind": "math", "phase": "typeset", "message": "Missing argument"]
            func send(_ changes: [String: Any] = [:]) {
                configuration.bridge.receive(name: CardRuntime.signalHandlerName, world: CardRuntime.interactionWorldName,
                    body: base.merging(changes, uniquingKeysWith: { _, value in value }), page: page)
            }
            send()
            let accepted = try #require(pair.diagnostics.templateErrors.first)
            #expect(accepted.update == update && accepted.kind == "math" && accepted.phase == "typeset")
            #expect(accepted.templateOrdinal == 2 && accepted.templateDescriptor == "Math card")
            #expect(accepted.message == "Missing argument" && accepted.scriptIndex == nil)
            #expect(pair.diagnostics.entries.last?.kind == "script.math.typeset")
            for phase in ["script", "later", "update-hook", "shown-hook"] {
                var changes: [String: Any] = ["phase": phase]
                if phase == "script" { changes["index"] = 1 }
                send(changes)
            }
            for kind in ["exception", "rejection", "hook", "unknown"] { send(["kind": kind]) }
            let invalid: [[String: Any]] = [
                ["message": String(repeating: "x", count: 8_001)], ["source": String(repeating: "x", count: 4_097)],
                ["index": 1], ["line": true], ["column": -1], ["extra": "field"],
            ]
            for changes in invalid { send(changes) }
            #expect(pair.diagnostics.templateErrors == [accepted])
            #expect(pair.diagnostics.entries.filter { $0.kind == "bridgeRejected" }.reduce(0) { $0 + $1.repeats } == 14)
            send(["message": String(repeating: "x", count: 8_000), "source": String(repeating: "s", count: 4_096),
                  "line": 0, "column": 1, "index": NSNull()])
            #expect(pair.diagnostics.templateErrors.count == 2)
            #expect(pair.diagnostics.templateErrors.last?.message.count == 2_000)
        }

        @Test func bridgeReasonStoragePreservesLegacyPrivacyShape() {
            let diagnostics = CardDiagnostics()
            let update = CardUpdate(card: 7, generation: 2, side: .question)
            diagnostics.record("bridgeRejected", update: update)
            for reason in CardBridge.RejectionReason.allCases {
                diagnostics.record("bridgeRejected", update: update, bridgeRejectionReason: reason)
                diagnostics.record("bridgeRejected", update: update, bridgeRejectionReason: reason)
            }
            diagnostics.record("bridgeRejected", update: update)
            let old = CardDiagnostics.Entry(update: update, kind: "bridgeRejected", name: "", index: nil, script: nil, repeats: 2)
            #expect(old.bridgeRejectionReason == nil)
            #expect(diagnostics.entries.first == old)
            #expect(diagnostics.entries.count == CardBridge.RejectionReason.allCases.count + 1)
            #expect(diagnostics.entries.dropFirst().compactMap(\.bridgeRejectionReason) == CardBridge.RejectionReason.allCases)
            #expect(diagnostics.entries.allSatisfy {
                $0.kind == "bridgeRejected" && $0.update == update && $0.name.isEmpty && $0.index == nil && $0.script == nil && $0.repeats == 2
            })
        }

        @Test func receiveRejectionReasonsPreserveAdmission() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved(), platform: .mac)
            defer { pair.close() }
            let window = SpikeWindow(pair.container)
            defer { window.close() }
            try await pair.load()
            let page = pair.current
            let update = CardUpdate(card: 7, generation: 2, side: .question)
            page.begin(CardPayload(cardID: 7, question: "q", answer: "a"), update: update)
            var actions: [CardBridgeAction] = []
            var signals: [String] = []
            var confirmations = 0
            page.onAction = { _, action in actions.append(action) }
            page.onSignal = { _, signal, _ in signals.append(signal) }
            page.onConfirmation = { _, _ in confirmations += 1 }
            let base: [String: Any] = ["card": 7, "generation": 2, "side": "question", "signal": "settled"]
            func body(_ changes: [String: Any]) -> [String: Any] {
                base.merging(changes, uniquingKeysWith: { _, value in value })
            }
            let world = CardRuntime.interactionWorldName
            let signal = CardRuntime.signalHandlerName
            let confirmation = CardRuntime.confirmationHandlerName
            func paint(_ changes: [String: Any] = [:]) -> [String: Any] {
                let base: [String: Any] = ["card": 7, "generation": 2, "side": "question", "token": 2, "phase": "settled"]
                return base.merging(changes, uniquingKeysWith: { _, value in value })
            }
            var samples: [(String, String?, Any, CardBridge.RejectionReason)] = [
                (signal, WKContentWorld.page.name, "malformed", .world),
                (signal, world, "malformed", .body),
                (signal, world, body(["card": 7.5, "generation": 1]), .card),
                (signal, world, body(["generation": true]), .generation),
                (signal, world, body(["generation": 1, "side": "unknown"]), .side),
                (signal, world, body(["generation": 3]), .futureGeneration),
                (signal, world, body(["side": "answer"]), .side),
                (signal, world, body(["signal": true]), .signalType),
                (signal, world, body(["signal": "nativeOpen", "path": "/synthetic-private"]), .unknownSignal),
                (signal, world, body(["extra": true]), .signalFields),
                (signal, world, body(["signal": "error", "kind": "exception", "phase": "later",
                                      "message": "TypeError: synthetic-private-text", "source": "file:///private/synthetic.js",
                                      "line": 98765, "column": 87654, "extra": true]), .signalFields),
                (signal, world, body(["signal": "focus", "editable": "true", "typeans": true]), .signalValue),
                (signal, world, body(["signal": "typedText", "text": String(repeating: "x", count: 65_537)]), .signalValue),
                (confirmation, world, paint(["token": true]), .confirmationToken),
                (confirmation, world, paint(["extra": true]), .confirmationShape),
                (confirmation, world, paint(["token": 1]), .confirmationValue),
                (signal, world, body(["card": 8]), .card),
                (signal, world, body(["signal": "visible", "token": 2]), .signalValue),
                (confirmation, world, paint(["card": 8]), .card),
                (confirmation, world, paint(["side": "answer"]), .side),
                (confirmation, world, paint(["generation": 3]), .futureGeneration),
                (confirmation, world, paint(["phase": "visible"]), .confirmationValue),
                // Old malformed input is still rejected before age admission.
                (signal, WKContentWorld.page.name, body(["generation": 1]), .world),
                (confirmation, WKContentWorld.page.name, paint(["generation": 1]), .world),
                (confirmation, world, "malformed", .body),
                (signal, world, body(["generation": -1]), .generation),
                (signal, world, body(["generation": 9_007_199_254_740_992 as Int64]), .generation),
                (signal, world, body(["generation": 1, "side": "answer"]).merging(
                    Dictionary(uniqueKeysWithValues: (0..<13).map { ("extra\($0)", true as Any) }),
                    uniquingKeysWith: { _, value in value }), .bodySize),
                (signal, world, body(["generation": 1, "side": true]), .side),
                (signal, world, body(["generation": 1, "signal": "nativeOpen"]), .unknownSignal),
                (signal, world, body(["generation": 1, "signal": "openAttachmentPDF"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "openAttachmentPDF", "filename": "other.pdf"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "openAttachmentPDF", "filename": String(repeating: "x", count: 76)]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "openAttachmentPDF", "filename": CardAttachmentPDFTests.name, "url": "ignored"]), .signalFields),
                (signal, world, body(["generation": 1, "signal": true]), .signalType),
                (signal, world, body(["generation": 1, "extra": true]), .signalFields),
                (signal, world, body(["generation": 1, "signal": "visible", "token": -1]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "visible", "token": true]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "visible"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "shown", "extra": true]), .signalFields),
                (signal, world, body(["generation": 1, "signal": "updateFailed", "error": false]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "updateFailed", "error": String(repeating: "x", count: 8_001)]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "command"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "command", "reveal": false]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "command", "reveal": true, "command": "ans"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "command", "command": String(repeating: "x", count: 129)]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "error", "kind": "exception", "phase": "script", "message": "error"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "error", "kind": "exception", "phase": "later", "message": "error", "line": true]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "mediaError", "tag": "unknown", "url": "synthetic"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "mediaError", "tag": "img", "url": String(repeating: "x", count: 4_097)]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "selection", "selected": 1]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "focus", "editable": true]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "composition", "active": "true"]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "typedText", "text": String(repeating: "x", count: 65_537)]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "listeners", "tap": true, "key": 0]), .signalValue),
                (signal, world, body(["generation": 1, "signal": "mediaPlayback", "playing": NSNull()]), .signalValue),
                (confirmation, world, paint(["generation": 1, "token": true]), .confirmationToken),
                (confirmation, world, paint(["generation": 1, "token": -1]), .confirmationToken),
                (confirmation, world, paint(["generation": 1, "token": 1.5]), .confirmationToken),
                (confirmation, world, paint(["generation": 1, "token": Double.infinity]), .confirmationToken),
                (confirmation, world, paint(["generation": 1, "token": 9_007_199_254_740_992 as Int64]), .confirmationToken),
                (confirmation, world, paint(["generation": 1, "extra": true]), .confirmationShape),
                (confirmation, world, ["card": 7, "token": 1, "phase": "settled"] as [String: Any], .confirmationShape),
                (confirmation, world, paint(["generation": 1, "phase": "unknown"]), .confirmationValue),
                (confirmation, world, paint(["generation": 1, "phase": true]), .confirmationShape),
                (confirmation, world, paint(["card": true, "generation": 1]), .card),
                (confirmation, world, paint(["generation": true]), .generation),
                (confirmation, world, paint(["generation": -1]), .generation),
                (confirmation, world, paint(["generation": 1.5]), .generation),
                (confirmation, world, paint(["generation": 1, "side": "unknown"]), .side),
                (confirmation, world, paint(["generation": 1, "side": true]), .side),
                // Confirmation shape cannot enter through the signal channel.
                (signal, world, ["card": 7, "token": 2, "phase": "settled"] as [String: Any], .generation),
            ]
            let malformedCommands = [
                "play:q:-1", "play:q:1.5", "play:x:0", "play:question:0",
                "play:q:٢", "play:q:１", "play:q:one", "play:q:",
                "play:q:999999999999999999999999999", "play:q:+0", "play:q: 0",
                "play:q:0:extra", "PLAY:q:0", "", "nativeOpen", "ease3",
            ]
            for generation in [1, 2] {
                for command in malformedCommands {
                    samples.append((signal, world, body(["generation": generation, "signal": "command", "command": command]), .signalValue))
                }
            }
            for (name, world, value, reason) in samples {
                let before = pair.diagnostics.entries.first { $0.kind == "bridgeRejected" && $0.update == update && $0.bridgeRejectionReason == reason }?.repeats ?? 0
                configuration.bridge.receive(name: name, world: world, body: value, page: page)
                let record = try #require(pair.diagnostics.entries.first {
                    $0.kind == "bridgeRejected" && $0.update == update && $0.bridgeRejectionReason == reason
                })
                #expect(record.repeats == before + 1)
                #expect(record.name.isEmpty && record.index == nil && record.script == nil)
                #expect(actions.isEmpty && signals.isEmpty && confirmations == 0 && !page.settled)
                #expect(pair.diagnostics.templateErrors.isEmpty)
            }
            let rejections = pair.diagnostics.entries
            configuration.bridge.receive(name: signal, world: world, body: base, page: page)
            configuration.bridge.receive(name: confirmation, world: world,
                                         body: paint(), page: page)
            #expect(page.settled && signals == ["settled"] && confirmations == 1 && actions.isEmpty)
            #expect(pair.diagnostics.entries == rejections)
        }

        @Test(arguments: [false, true], [UInt64(2), UInt64(80)])
        func delayedWellFormedMessagesLeaveCurrentUpdateUntouched(sameCard: Bool, visibleToken: UInt64) async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved(), platform: .mac)
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let page = pair.current
            let old = CardUpdate(card: sameCard ? 7 : 6, generation: 39, side: .answer)
            page.begin(CardPayload(cardID: old.card, question: "old", answer: "old",
                                   questionAudio: [.sound(name: "one.mp3"), .sound(name: "two.mp3"), .sound(name: "three.mp3")]), update: old)
            page.visibleToken = visibleToken
            page.webView.setIdle(false)
            try await page.present(token: visibleToken)
            let current = CardUpdate(card: 7, generation: 40, side: .question)
            page.begin(CardPayload(cardID: 7, question: "new", answer: "new", questionAudio: [.sound(name: "one.mp3")]), update: current)
            page.visibleToken = visibleToken
            var actions: [CardBridgeAction] = []
            var signals: [String] = []
            var confirmations: [String] = []
            page.onAction = { _, action in actions.append(action) }
            page.onSignal = { _, signal, _ in signals.append(signal) }
            page.onConfirmation = { _, body in confirmations.append(body["phase"] as? String ?? "") }
            let world = CardRuntime.interactionWorldName
            func send(_ update: CardUpdate, _ signal: String, _ fields: [String: Any] = [:]) {
                let base: [String: Any] = ["card": update.card, "generation": update.generation, "side": update.sideName, "signal": signal]
                configuration.bridge.receive(name: CardRuntime.signalHandlerName, world: world,
                                             body: base.merging(fields, uniquingKeysWith: { _, value in value }), page: page)
            }
            func paint(_ update: CardUpdate, _ phase: String, _ token: UInt64) {
                configuration.bridge.receive(name: CardRuntime.confirmationHandlerName, world: world,
                                             body: ["card": update.card, "generation": update.generation, "side": update.sideName,
                                                    "phase": phase, "token": token], page: page)
            }
            let before = pair.diagnostics.entries
            let oldSignals: [(String, [String: Any])] = [
                ("settled", [:]), ("shown", [:]), ("visible", ["token": visibleToken]),
                ("updateFailed", ["error": "old failure"]),
                ("openAttachmentPDF", ["filename": CardAttachmentPDFTests.name]),
                ("command", ["command": "ans"]), ("command", ["reveal": true]),
                // Valid for the old payload; outside the new AV list. Age must
                // be classified before consulting the new payload's bounds.
                ("command", ["command": "play:q:0002"]),
                ("error", ["kind": "exception", "phase": "script", "index": 1, "message": "TypeError: old"]),
                ("mediaError", ["tag": "img", "url": "old.png"]),
                ("selection", ["selected": true]), ("focus", ["editable": true, "typeans": true]),
                ("composition", ["active": true]), ("typedText", ["text": "old text"]),
                ("listeners", ["tap": true, "key": true]), ("mediaPlayback", ["playing": true]),
            ]
            for (signal, fields) in oldSignals { send(old, signal, fields) }
            paint(old, "settled", old.generation)
            for token in [UInt64(0), UInt64(2), UInt64(80)] { paint(old, "visible", token) }
            #expect(pair.diagnostics.entries == before && pair.diagnostics.templateErrors.isEmpty)
            #expect(actions.isEmpty && signals.isEmpty && confirmations.isEmpty)
            #expect(page.update == current && !page.settled && page.typedValue == nil && !page.acceptsVisibleConfirmation())
            #expect(page.visibleToken == visibleToken && page.presented)

            // Current payload bounds and exact identity/token still reject.
            send(current, "command", ["command": "play:q:1"])
            send(current, "visible", ["token": visibleToken + 1])
            paint(current, "visible", visibleToken) // no matching visible receipt yet
            #expect(actions.isEmpty && signals.isEmpty && confirmations.isEmpty && !page.acceptsVisibleConfirmation())
            let rejections = pair.diagnostics.entries
            #expect(rejections.filter { $0.kind == "bridgeRejected" }.reduce(0) { $0 + $1.repeats } == 3)
            send(current, "settled")
            paint(current, "settled", current.generation)
            send(current, "command", ["command": "ans"])
            send(current, "command", ["command": "play:q:0000"])
            send(current, "typedText", ["text": "current text"])
            send(current, "visible", ["token": visibleToken])
            paint(current, "visible", visibleToken + 1)
            #expect(confirmations == ["settled"] && page.acceptsVisibleConfirmation())
            #expect(pair.diagnostics.entries.filter { $0.kind == "bridgeRejected" }.reduce(0) { $0 + $1.repeats } == 4)
            let finalRejections = pair.diagnostics.entries
            paint(current, "visible", visibleToken)
            #expect(page.settled && page.acceptsVisibleConfirmation() && page.update == current)
            #expect(actions == [.reveal, .play(.question, 0)])
            #expect(signals == ["settled", "typedText", "visible"] && confirmations == ["settled", "visible"])
            #expect(page.typedValue?.text == "current text")
            #expect(pair.diagnostics.entries == finalRejections)
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
            let invalid: [(Any, CardBridge.RejectionReason)] = [
                ("{\"scheme\":\"other\",\"msg\":\"midCenter\"}", .legacyValue),
                ("{\"scheme\":\"ankitap\",\"msg\":\"middleCenter\"}", .legacyValue),
                (["scheme": "ankitap", "msg": "topLeft"], .body),
            ]
            for (value, reason) in invalid {
                configuration.bridge.receive(name: "cb", world: WKContentWorld.page.name, body: value, page: page)
                #expect(pair.diagnostics.entries.last?.kind == "bridgeRejected")
                #expect(pair.diagnostics.entries.last?.bridgeRejectionReason == reason)
                #expect(pair.diagnostics.entries.last?.name.isEmpty == true)
                #expect(actions == [.again, .again, .again, .good, .good, .good])
            }
            configuration.bridge.receive(name: "meguri", world: WKContentWorld.page.name, body: "play:a:000", page: page)
            #expect(actions.last == .play(.answer, 0) && actions.count == 7)
            // Tokenless adapters bind the actual sending page's native payload,
            // even while the other page has a different card, side and AV list.
            let other = pair.idle
            other.begin(CardPayload(cardID: 99, question: "other question", answer: "other answer"),
                        update: CardUpdate(card: 99, generation: 3, side: .question))
            var otherActions: [CardBridgeAction] = []
            other.onAction = { _, action in otherActions.append(action) }
            other.visibleToken = 2
            try await other.present(token: 2)
            configuration.bridge.receive(name: "cb", world: WKContentWorld.page.name,
                                         body: "{\"scheme\":\"ankitap\",\"msg\":\"topRight\"}", page: other)
            configuration.bridge.receive(name: "meguri", world: WKContentWorld.page.name, body: "play:a:0", page: other)
            #expect(otherActions == [.reveal])
            #expect(actions == [.again, .again, .again, .good, .good, .good, .play(.answer, 0)])
            #expect(pair.diagnostics.entries.last?.bridgeRejectionReason == .legacyValue)
        }
    }
}
