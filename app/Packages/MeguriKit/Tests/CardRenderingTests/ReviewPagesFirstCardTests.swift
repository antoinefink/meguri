@testable import AppModel
@testable import CardRendering
import CoreBridge
import Foundation
import Testing
import WebKit

extension WebKitSuites {
    /// The card prepared before a session (R1813, N16.8) is shown only when the
    /// session's own first card is the same card in the same state with the same
    /// render; any other card is prepared again, so Study shows exactly what it
    /// would have shown without the preload.
    @Suite(.timeLimit(.minutes(2))) struct ReviewPagesFirstCardTests {
        enum Session: CaseIterable, Sendable { case same, otherContent, otherState, otherCard }

        @Test(arguments: Session.allCases)
        func aPreparedFirstCardIsUsedOnlyWhenItIsTheSessionsOwn(_ session: Session) async throws {
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            try await pages.load()
            let early = Self.item(1)
            try await pages.prepareFirst(item: early, payload: CardPayload(cardID: 1, question: "<p>Early</p>", answer: "Answer"))
            let prepared = try #require(pair.idle.update)
            #expect(!pair.idle.presented && pair.current.update == nil)

            var head = early
            var question = "<p>Early</p>"
            switch session {
            case .same: break
            case .otherContent: question = "<p>Edited</p>"
            case .otherState: head = Self.item(1, token: 9)
            case .otherCard: head = Self.item(2); question = "<p>Other</p>"
            }
            let rendered = CardPayload(cardID: head.cardId, question: question, answer: "Answer")
            pages.renderHead = { _, _, _ in [rendered] }
            try await pages.render(sessionID: 5, items: [head])
            try await pages.present(item: head, inputTime: 0)
            #expect((pair.current.update == prepared) == (session == .same))
            #expect(pair.current.payload?.question == question && pair.current.update?.card == head.cardId)
            #expect(pair.current.settled)
        }

        @Test func speculativeFirstAudioCardIsAdmittedOnlyWhenReviewPresentsIt() async throws {
            let fixture = MediaFixture()
            fixture.add("/first.mp3", Data([1]), "audio/mpeg")
            let resolver = try await fixture.resolved()
            let backend = CardAudioControllerTests.Backend(), session = CardAudioControllerTests.Session()
            let audio = CardAudioController(resolver: resolver, backend: backend, session: session)
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: resolver, audio: audio)
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            try await pages.load()
            let item = Self.item(1, autoplay: true)
            let card = CardPayload(cardID: 1, question: "First", answer: "Answer", questionAudio: [.sound(name: "first.mp3")])
            try await pages.prepareFirst(item: item, payload: card)
            let early = try #require(pair.idle.update)
            // Join native preparation without waiting on any real audio hardware.
            await audio.prepare(payload: card, update: early)?.value
            #expect(session.continuityOwners.isEmpty && session.owners.isEmpty && backend.events.isEmpty)
            pages.renderHead = { _, _, _ in [card] }
            try await pages.render(sessionID: 5, items: [item])
            try await pages.present(item: item, inputTime: 0)
            #expect(pair.current.update == early && session.continuityOwners.count == 1)
            await pages.suspend()
            #expect(session.continuityOwners.isEmpty)
            await pages.resume()
            #expect(session.continuityOwners.isEmpty)
        }

        @Test func lossBeforeCommitFinishesVisualsWithoutRevivingAutoplayOrAnswerLookahead() async throws {
            let fixture = MediaFixture()
            fixture.add("/q.mp3", Data([1]), "audio/mpeg")
            fixture.add("/a.mp3", Data([1]), "audio/mpeg")
            let resolver = try await fixture.resolved()
            let backend = CardAudioControllerTests.Backend(), session = CardAudioControllerTests.Session()
            let audio = CardAudioController(resolver: resolver, backend: backend, session: session)
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: resolver, audio: audio)
            let pages = ReviewPages(pair: pair), window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            try await pages.load()
            let item = Self.item(1, autoplay: true)
            pages.renderHead = { _, _, _ in [CardPayload(cardID: 1, question: "Question", answer: "Answer",
                questionAudio: [.sound(name: "q.mp3")], answerAudio: [.sound(name: "a.mp3")])] }
            try await pages.render(sessionID: 5, items: [item])
            // This is after ReviewPages' actual prepare await, before its CA
            // transaction/commit. The visual work must still commit normally.
            pages.onWillPresent = { audio.sessionEvent(.pause) }
            try await pages.present(item: item, inputTime: 0)
            try await Self.shown(pair)
            await audio.transportSettled()
            #expect(pair.current.update?.side == .question && audio.current == nil)
            #expect(backend.events.isEmpty && session.continuityOwners.isEmpty)
            #expect(!backend.clips.contains { $0.name == "a.mp3" })
            pair.replay() // Explicit action on the now-visible card remains valid.
            #expect(await backend.nextPlay() == "q.mp3")
        }

        @Test func lossDuringActualPresentPreparationCannotCreateFreshAutoplay() async throws {
            let fixture = MediaFixture()
            fixture.add("/q.mp3", Data([1]), "audio/mpeg")
            let resolver = try await fixture.resolved()
            let backend = CardAudioControllerTests.Backend(), session = CardAudioControllerTests.Session()
            let audio = CardAudioController(resolver: resolver, backend: backend, session: session)
            let configuration = CardWebConfiguration(store: .nonPersistent())
            let receipts = SettlementGate()
            configuration.configuration.userContentController.add(receipts, contentWorld: CardPage.world, name: "audioPreparationHeld")
            defer {
                receipts.events.continuation.finish()
                configuration.configuration.userContentController.removeScriptMessageHandler(forName: "audioPreparationHeld", contentWorld: CardPage.world)
            }
            let pair = CardPagePair(configuration: configuration, resolver: resolver, audio: audio)
            let pages = ReviewPages(pair: pair), window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            try await pages.load()
            let item = Self.item(1, autoplay: true)
            pages.renderHead = { _, _, _ in [CardPayload(cardID: 1, question: "Question", answer: "Answer",
                questionAudio: [.sound(name: "q.mp3")])] }
            try await pages.render(sessionID: 5, items: [item])
            let idle = pair.idle
            try await idle.run("""
                const handler = webkit.messageHandlers.meguriCard;
                const post = handler.postMessage;
                window.audioHeldHandler = handler;
                let held;
                handler.postMessage = function(message) {
                    if (message.signal === 'settled') {
                        held = message;
                        webkit.messageHandlers.audioPreparationHeld.postMessage(null);
                    } else post.call(handler, message);
                };
                window.releaseAudioPreparation = function() {
                    handler.postMessage = post;
                    if (held) post.call(handler, held);
                };
                """, world: CardPage.world)
            let presentation = Task { try await pages.present(item: item, inputTime: 0) }
            defer { presentation.cancel() }
            try await Spike.deadline("actual present prepare held", timeout: .seconds(3)) {
                for await _ in receipts.events.stream { return }
                throw Spike.Failure("No held preparation")
            }
            audio.sessionEvent(.pause)
            try await idle.run("releaseAudioPreparation();", world: CardPage.world)
            try await presentation.value
            try await Self.shown(pair)
            await audio.transportSettled()
            #expect(backend.events.isEmpty && audio.current == nil && session.continuityOwners.isEmpty)
        }

        @Test func routeLossDuringTypedComparisonFinishesAnswerWithoutAutoplay() async throws {
            let fixture = MediaFixture()
            fixture.add("/a.mp3", Data([1]), "audio/mpeg")
            let resolver = try await fixture.resolved()
            let backend = CardAudioControllerTests.Backend(), session = CardAudioControllerTests.Session()
            let audio = CardAudioController(resolver: resolver, backend: backend, session: session)
            let compare = CardAudioControllerTests.Hold()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: resolver, audio: audio,
                compare: { _, _ in await compare.wait(); return "Compared" })
            let pages = ReviewPages(pair: pair), window = SpikeWindow(pair.container)
            defer { compare.release(); pages.close(); window.close() }
            try await pages.load()
            let item = Self.item(1, autoplay: true)
            pages.renderHead = { _, _, _ in [CardPayload(cardID: 1, question: "<input id=typeans>",
                answer: TypeAnswerDescriptor.placeholderHTML, answerAudio: [.sound(name: "a.mp3")], typeIn: .init(expected: "typed"))] }
            try await pages.render(sessionID: 5, items: [item])
            try await pages.present(item: item, inputTime: 0)
            try await Self.shown(pair)
            let reveal = Task { try await pages.reveal(inputTime: 1) }
            defer { reveal.cancel() }
            try await Spike.deadline("typed comparison held", timeout: .seconds(3)) { await compare.reached() }
            audio.sessionEvent(.pause)
            compare.release()
            try await reveal.value
            #expect(pair.current.update?.side == .answer && audio.current == nil)
            #expect(backend.events.isEmpty && session.continuityOwners.isEmpty)
            pair.replay()
            #expect(await backend.nextPlay() == "a.mp3")
        }

        private static func shown(_ pair: CardPagePair) async throws {
            try await CardBehaviorFixture.until("review question committed") { pair.current.presented }
            let update = try #require(pair.current.update)
            try await pair.current.waitForShown(update)
        }
        private final class SettlementGate: NSObject, WKScriptMessageHandler {
            let events = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
            func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
                events.continuation.yield(())
            }
        }

        @Test func reloadingWarmPagesLeavesTheirPreparedCard() async throws {
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            try await pages.load()
            #expect(pages.reusable && pair.documentsReady)
            try await pages.prepareFirst(item: Self.item(1), payload: CardPayload(cardID: 1, question: "<p>Early</p>", answer: "Answer"))
            let prepared = try #require(pair.idle.update)
            // The session's own load finds the documents loaded and does nothing.
            try await pages.load()
            #expect(pair.idle.update == prepared && pair.idle.settled)
            pages.discardFirst()
            pages.renderHead = { _, _, _ in [CardPayload(cardID: 1, question: "<p>Early</p>", answer: "Answer")] }
            try await pages.render(sessionID: 5, items: [Self.item(1)])
            try await pages.present(item: Self.item(1), inputTime: 0)
            // Discarded: prepared again, never the earlier update.
            #expect(pair.current.update != prepared && pair.current.payload?.question == "<p>Early</p>")
        }

        /// A pause empties the idle page under a preloaded card. The next
        /// present prepares that card again and never fails with `notPrepared`.
        @Test func aPreloadLostToAPauseIsPreparedAgain() async throws {
            let fixture = MediaFixture()
            let pair = CardPagePair(configuration: CardWebConfiguration(store: .nonPersistent()), resolver: try await fixture.resolved())
            let pages = ReviewPages(pair: pair)
            let window = SpikeWindow(pair.container)
            defer { pages.close(); window.close() }
            try await pages.load()
            let items = [Self.item(1), Self.item(2)]
            pages.renderHead = { _, keys, _ in
                keys.map { key in
                    let id: Int64 = key == items[0].renderKey ? 1 : 2
                    return CardPayload(cardID: id, question: "<p>Card \(id)</p>", answer: "Answer")
                }
            }
            try await pages.render(sessionID: 5, items: items)
            try await pages.present(item: items[0], inputTime: 0)
            try await pages.preload(item: items[1])
            let preloaded = try #require(pair.idle.update)
            await pages.suspend()
            await pages.resume()
            #expect(pair.idle.update == nil)
            try await pages.present(item: items[1], inputTime: 0)
            #expect(pair.current.update != preloaded && pair.current.update?.card == 2)
            #expect(pair.current.payload?.question == "<p>Card 2</p>" && pair.current.settled)
        }

        private static func item(_ id: Int64, token: UInt8? = nil, autoplay: Bool = false) -> ReviewHeadItem {
            let state = ReviewSchedulingState.new(position: 1)
            let answer = ReviewAnswerState(state: state, delay: .days(amount: 1), left: 0, factor: 0,
                revlog: .init(interval: 1, lastInterval: 0, kind: 0, factor: 0),
                label: .init(lessThan: false, amount: "1", unit: .days))
            return ReviewHeadItem(cardId: id, noteId: id, kind: .new,
                stateToken: .init(bytes: Array(repeating: token ?? UInt8(id), count: 32)),
                renderKey: .init(bytes: Array(repeating: UInt8(id), count: 32)),
                nextStates: .init(current: state, again: answer, hard: answer, good: answer, easy: answer),
                predictedNext: .init(again: 2, hard: 2, good: 2, easy: 2), flag: 0, marked: false,
                homePreset: .init(id: 1, autoplay: autoplay, skipQuestionWhenReplayingAnswer: false, maximumAnswerSeconds: 60, questionSeconds: 0, answerSeconds: 0,
                    questionAction: 0, answerAction: 0, waitForAudio: false, leechAction: 0))
        }
    }
}
