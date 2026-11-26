@testable import CardRendering
import Foundation
import QuartzCore
import Testing
import WebKit

/// `render/preview` V12 for the Note Types preview: a replay command plays
/// only right after a click or tap on the card, so a card script that calls
/// the bridge by itself (`pycmd('play:q:0')` on load) plays nothing.
@Suite struct PreviewTapGateTests {
    @Test func withoutTheRuleEveryReplayPlays() {
        #expect(CardPagePair.allowsPlay(needsTap: false, lastTap: nil, now: 100))
    }

    @Test func withTheRuleOnlyARecentTapLetsAReplayPlay() {
        // A script on load: no tap yet.
        #expect(!CardPagePair.allowsPlay(needsTap: true, lastTap: nil, now: 100))
        // The replay button's own click, a moment before its command arrives.
        #expect(CardPagePair.allowsPlay(needsTap: true, lastTap: 99.9, now: 100))
        #expect(CardPagePair.allowsPlay(needsTap: true, lastTap: 100 - CardPagePair.tapWindow, now: 100))
        // A script that waits for the tap's moment to pass.
        #expect(!CardPagePair.allowsPlay(needsTap: true, lastTap: 90, now: 100))
        // A clock that went back never counts.
        #expect(!CardPagePair.allowsPlay(needsTap: true, lastTap: 101, now: 100))
    }
}

extension WebKitSuites {
    /// The gate on a real page pair: the card's own `play:q:0` command, sent
    /// with no click or tap on the card, is dropped and recorded; the same
    /// command right after a tap goes to the audio controller.
    @Suite struct PreviewTapGatePairTests {
        @Test func previewMathIsReadyOnBothFaces() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent(), mediaNeedsGesture: true)
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved(), platform: .mac)
            pair.playNeedsTap = true
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let update = try await pair.prepare(CardPayload(cardID: 41,
                question: #"\(a^2+b^2=c^2\)"#, answer: #"\[c=\sqrt{a^2+b^2}\]"#))
            let prepared = pair.idle
            #expect(prepared.settled)
            let front = try await prepared.webView.callAsyncJavaScript(
                "return document.querySelectorAll('#qa mjx-container svg').length;", arguments: [:], in: nil, contentWorld: .page)
            #expect(front as? Int == 1)
            let commits = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            pair.onCommitted = { committed, _ in if committed == update { commits.continuation.yield(()) } }
            defer { pair.onCommitted = nil; commits.continuation.finish() }
            do {
                CATransaction.begin()
                defer { CATransaction.commit() }
                _ = try pair.present(update)
            }
            try await Spike.deadline("preview math native commit", timeout: .seconds(3)) {
                for await _ in commits.stream { return }
                throw Spike.Failure("missing preview math commit")
            }
            _ = try await pair.showAnswer()
            let back = try await pair.current.webView.callAsyncJavaScript(
                "return document.querySelectorAll('#qa mjx-container[display=\"true\"] svg').length;", arguments: [:], in: nil, contentWorld: .page)
            #expect(back as? Int == 1)
            #expect(pair.diagnostics.templateErrors.isEmpty)
        }

        @Test func malformedMathReachesNativeTemplateErrors() async throws {
            let fixture = try await CardBehaviorFixture()
            defer { fixture.close() }
            let pair = fixture.pair
            try await fixture.present(CardPayload(cardID: 42, ordinal: 2, templateDescriptor: "Math preview",
                question: #"\(x+1\)"#, answer: #"\(\frac{1}\)"#))
            _ = try await pair.showAnswer()
            let error = try #require(pair.diagnostics.templateErrors.first)
            #expect(error.update == pair.current.update && error.update.side == .answer && error.kind == "math" && error.phase == "typeset")
            #expect(error.templateOrdinal == 2 && error.templateDescriptor == "Math preview")
            #expect(error.message.contains("Missing") && !error.message.contains("[object Object]"))
            #expect(!pair.diagnostics.entries.contains { $0.kind == "bridgeRejected" })
            #expect(try await fixture.bool("return !!document.querySelector('#qa [data-mml-node=merror]') && document.getElementById('qa').style.opacity === '1';") == true)
        }

        @Test(.disabled(if: runsInVirtualMachine(), "no audio route in a virtual machine"))
        func aScriptsReplayCommandWithoutATapPlaysNothing() async throws {
            let fixture = MediaFixture()
            let configuration = CardWebConfiguration(store: .nonPersistent(), mediaNeedsGesture: true)
            let pair = CardPagePair(configuration: configuration, resolver: try await fixture.resolved(), platform: .mac)
            pair.playNeedsTap = true
            let window = SpikeWindow(pair.container)
            defer { pair.close(); window.close() }
            try await pair.load()
            let page = pair.current
            let update = CardUpdate(card: 7, generation: 40, side: .question)
            page.begin(CardPayload(cardID: 7, question: "card", answer: "card", questionAudio: [.sound(name: "one.mp3")]), update: update)
            page.visibleToken = 2
            page.webView.setIdle(false)
            try await page.present(token: 2)
            let world = CardRuntime.interactionWorldName
            func send(_ signal: String, _ fields: [String: Any] = [:]) {
                let base: [String: Any] = ["card": update.card, "generation": update.generation, "side": update.sideName, "signal": signal]
                configuration.bridge.receive(name: CardRuntime.signalHandlerName, world: world,
                                             body: base.merging(fields, uniquingKeysWith: { _, value in value }), page: page)
            }
            send("settled")
            configuration.bridge.receive(name: CardRuntime.confirmationHandlerName, world: world,
                                         body: ["card": update.card, "generation": update.generation, "side": update.sideName,
                                                "phase": "settled", "token": update.generation], page: page)
            func dropped() -> Int { pair.diagnostics.entries.filter { $0.kind == "playWithoutTap" }.reduce(0) { $0 + $1.repeats } }
            send("command", ["command": "play:q:0"])
            #expect(dropped() == 1, "no tap: the command plays nothing")
            pair.lastTap = ProcessInfo.processInfo.systemUptime
            send("command", ["command": "play:q:0"])
            #expect(dropped() == 1, "right after a tap the command reaches the audio controller")
        }
    }
}
