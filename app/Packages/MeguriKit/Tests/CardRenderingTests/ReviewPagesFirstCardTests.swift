@testable import AppModel
@testable import CardRendering
import CoreBridge
import Foundation
import Testing

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

        private static func item(_ id: Int64, token: UInt8? = nil) -> ReviewHeadItem {
            let state = ReviewSchedulingState.new(position: 1)
            let answer = ReviewAnswerState(state: state, delay: .days(amount: 1), left: 0, factor: 0,
                revlog: .init(interval: 1, lastInterval: 0, kind: 0, factor: 0),
                label: .init(lessThan: false, amount: "1", unit: .days))
            return ReviewHeadItem(cardId: id, noteId: id, kind: .new,
                stateToken: .init(bytes: Array(repeating: token ?? UInt8(id), count: 32)),
                renderKey: .init(bytes: Array(repeating: UInt8(id), count: 32)),
                nextStates: .init(current: state, again: answer, hard: answer, good: answer, easy: answer),
                predictedNext: .init(again: 2, hard: 2, good: 2, easy: 2), flag: 0, marked: false,
                homePreset: .init(id: 1, autoplay: false, skipQuestionWhenReplayingAnswer: false, maximumAnswerSeconds: 60, questionSeconds: 0, answerSeconds: 0,
                    questionAction: 0, answerAction: 0, waitForAudio: false, leechAction: 0))
        }
    }
}
