import Foundation
import Testing
import Instrumentation
import MeguriCoreBindings
@testable import CoreBridge

@Suite struct ReviewSessionBridgeTests {
    static let memory = MeguriCoreBindings.ReviewMemoryState(stability: 2.75, difficulty: 6.125)
    static let learning = MeguriCoreBindings.ReviewLearningState(remainingSteps: 3, scheduledSecs: 91, elapsedSecs: 17, memoryState: memory)
    static let review = MeguriCoreBindings.ReviewReviewState(scheduledDays: 24, elapsedDays: 29, easeFactor: 2.375, lapses: 8, leeched: true, memoryState: memory)
    static func answer(_ state: MeguriCoreBindings.ReviewSchedulingState, _ delay: MeguriCoreBindings.ReviewDelay, _ amount: String, _ unit: MeguriCoreBindings.ReviewTimeUnit) -> MeguriCoreBindings.ReviewAnswerState {
        .init(state: state, delay: delay, left: 3, factor: 2375,
              revlog: .init(interval: -91, lastInterval: 24, kind: 2, factor: 750),
              label: .init(lessThan: true, amount: amount, unit: unit))
    }
    static var item: MeguriCoreBindings.ReviewHeadItem {
        .init(cardId: -41, noteId: -51, kind: .interdayLearning,
              stateToken: .init(bytes: Data(repeating: 0x17, count: 32)), renderKey: .init(bytes: Data(repeating: 0xa3, count: 32)),
              nextStates: .init(current: .new(position: 97),
                                again: answer(.learning(state: learning), .seconds(amount: 91), "2", .minutes),
                                hard: answer(.review(state: review), .days(amount: 24), "24", .days),
                                good: answer(.relearning(learning: learning, review: review), .seconds(amount: 121), "3", .minutes),
                                easy: answer(.new(position: 12), .days(amount: 365), "1", .years)),
              predictedNext: .init(again: -42, hard: nil, good: 43, easy: 44), flag: 7, marked: true,
              homePreset: .init(id: -61, autoplay: true, skipQuestionWhenReplayingAnswer: true,
                                waitForAudio: true, showTimer: true, stopTimerOnAnswer: false, maximumAnswerSeconds: 123,
                                questionSeconds: 1.25, questionAction: 2, answerSeconds: 2.75, answerAction: 3,
                                leechAction: 1))
    }
    static var head: MeguriCoreBindings.ReviewHead {
        .init(items: [item], counts: .init(new: 11, learning: 12, review: 13), validUntil: 1_894_701_723, terminalState: .active)
    }
    static var diff: MeguriCoreBindings.ReviewHeadDiff {
        .init(head: head, marks: [.init(added: true, restate: false, rerender: true, relabel: true)], removed: [-70, 71])
    }
    static var finished: MeguriCoreBindings.ReviewDeckStudyState {
        .init(deck: -4, finished: true, empty: false,
              queueCounts: .init(new: 1, learning: 2, review: 3), listCounts: .init(new: 4, learning: 5, review: 6),
              difference: .init(new: -3, learning: 3, review: -9), tomorrow: .init(new: 7, learning: 8, review: 9),
              newLimitReached: false, newRemaining: true, reviewLimitReached: true, reviewRemaining: false,
              learningRemaining: 16, learningRemainingToday: 14, learningLaterToday: 15, secsUntilNextLearn: -17,
              nextLearningDue: -123, nextLearningReadyAt: -456, siblingBuried: 18, userBuried: 19)
    }

    @Test func completeHeadKeepsFourStatesPredictionsPresetAndTimeUnits() throws {
        let mapped = CoreBridge.ReviewHead(Self.head)
        #expect(mapped.counts == CoreBridge.ReviewCounts(new: 11, learning: 12, review: 13))
        #expect(mapped.validUntil == 1_894_701_723)
        let item = try #require(mapped.items.first)
        #expect(item.cardId == -41 && item.noteId == -51 && item.flag == 7 && item.marked)
        #expect(item.kind == .interdayLearning)
        #expect(item.stateToken.bytes == Array(repeating: 0x17, count: 32))
        #expect(item.renderKey.bytes == Array(repeating: 0xa3, count: 32))
        #expect(Array(item.stateToken.raw.bytes) == item.stateToken.bytes)
        #expect(Array(item.renderKey.raw.bytes) == item.renderKey.bytes)
        #expect(item.predictedNext == CoreBridge.ReviewPredictions(again: -42, hard: nil, good: 43, easy: 44))
        #expect(item.nextStates.current == .new(position: 97))
        guard case .learning(let learning) = item.nextStates.again.state,
              case .review(let review) = item.nextStates.hard.state else { Issue.record("State kinds lost"); return }
        #expect(learning == CoreBridge.ReviewLearningState(remainingSteps: 3, scheduledSecs: 91, elapsedSecs: 17,
                                                         memoryState: .init(stability: 2.75, difficulty: 6.125)))
        #expect(review == CoreBridge.ReviewReviewState(scheduledDays: 24, elapsedDays: 29, easeFactor: 2.375,
                                                     lapses: 8, leeched: true, memoryState: .init(stability: 2.75, difficulty: 6.125)))
        #expect(item.nextStates.good.state == .relearning(learning: learning, review: review))
        #expect(item.nextStates.easy.state == .new(position: 12))
        #expect(item.nextStates.again.delay == .seconds(amount: 91))
        #expect(item.nextStates.hard.delay == .days(amount: 24))
        #expect(item.nextStates.good.left == 3 && item.nextStates.good.factor == 2375)
        #expect(item.nextStates.good.revlog == CoreBridge.ReviewRevlogPreview(interval: -91, lastInterval: 24, kind: 2, factor: 750))
        #expect(item.nextStates.easy.label == CoreBridge.ReviewAnswerLabel(lessThan: true, amount: "1", unit: .years))
        #expect(item.homePreset == CoreBridge.ReviewHomePreset(id: -61, autoplay: true, skipQuestionWhenReplayingAnswer: true,
                                                             showTimer: true, stopTimerOnAnswer: false, maximumAnswerSeconds: 123,
                                                             questionSeconds: 1.25, answerSeconds: 2.75, questionAction: 2, answerAction: 3,
                                                             waitForAudio: true, leechAction: 1))
        let units: [(MeguriCoreBindings.ReviewTimeUnit, CoreBridge.ReviewTimeUnit)] = [
            (.seconds, .seconds), (.minutes, .minutes), (.hours, .hours), (.days, .days), (.months, .months), (.years, .years), (.end, .end),
        ]
        for (raw, expected) in units { #expect(CoreBridge.ReviewTimeUnit(raw) == expected) }
        let kinds: [(MeguriCoreBindings.ReviewQueueKind, CoreBridge.ReviewQueueKind)] = [
            (.new, .new), (.learning, .learning), (.interdayLearning, .interdayLearning), (.review, .review),
        ]
        for (raw, expected) in kinds { #expect(CoreBridge.ReviewQueueKind(raw) == expected) }
    }

    @Test func admissionPeekAnswerAndHeadMarksKeepAllAlternatives() {
        let head = CoreBridge.ReviewHead(Self.head)
        let diff = CoreBridge.ReviewHeadDiff(Self.diff)
        #expect(diff.head == head)
        #expect(diff.marks == [.init(added: true, restate: false, rerender: true, relabel: true)])
        #expect(diff.removed == [-70, 71])
        #expect(CoreBridge.ReviewStartResult(.started(sessionId: .max, generation: 29, head: Self.head, timeLim: 123)) == .started(sessionId: .max, generation: 29, head: head, timeLim: 123))
        let refusals: [(MeguriCoreBindings.ReviewRefusal, CoreBridge.ReviewRefusal)] = [
            (.fsrsPolicyPending, .fsrsPolicyPending), (.presetUnsupported(presetIds: [-3, 7]), .presetUnsupported(presetIds: [-3, 7])),
        ]
        for (raw, expected) in refusals {
            #expect(CoreBridge.ReviewStartResult(.refused(reason: raw)) == .refused(reason: expected))
            #expect(CoreBridge.ReviewPeekResult(.refused(reason: raw)) == .refused(reason: expected))
        }
        #expect(CoreBridge.ReviewPeekResult(.card(item: Self.item)) == .card(item: CoreBridge.ReviewHeadItem(Self.item)))
        #expect(CoreBridge.ReviewPeekResult(.empty) == .empty)
        #expect(CoreBridge.ReviewAnswerOutcome(.accepted(headDiff: Self.diff, leech: true, suspended: false, rolledOver: true)) == .accepted(headDiff: diff, leech: true, suspended: false, rolledOver: true))
        for notice in [true, false] {
            #expect(CoreBridge.ReviewAnswerOutcome(.stale(freshHead: Self.head, notice: notice)) == .stale(freshHead: head, notice: notice))
        }
        #expect(CoreBridge.ReviewAnswerOutcome(.notAtHead(freshHead: Self.head)) == .notAtHead(freshHead: head))
    }

    @Test func headKeepsEveryAuthoritativeTerminalState() {
        let cases: [(MeguriCoreBindings.ReviewTerminalState, CoreBridge.ReviewTerminalState)] = [
            (.active, .active), (.waitingUntil(instant: -123), .waitingUntil(instant: -123)), (.finished, .finished),
        ]
        for (raw, expected) in cases {
            let head = MeguriCoreBindings.ReviewHead(items: expected == .active ? [Self.item] : [],
                counts: .init(new: 0, learning: 0, review: 0), validUntil: 456, terminalState: raw)
            let mapped = CoreBridge.ReviewHead(head)
            #expect(mapped.terminalState == expected)
            #expect(mapped.validUntil == 456)
            #expect(mapped.items.isEmpty == (expected != .active))
        }
    }

    @Test func undoReasonsScopesAndFullStepTimingRemainTyped() {
        let reasons: [(MeguriCoreBindings.ReviewDisabledReason, CoreBridge.ReviewDisabledReason)] = [
            (.none, .none), (.clearedByRollover, .clearedByRollover), (.clearedBySync, .clearedBySync),
            (.clearedByOneWay, .clearedByOneWay), (.clearedByNonUndoableOp, .clearedByNonUndoableOp),
            (.changedExternally, .changedExternally), (.alreadyUploaded, .alreadyUploaded), (.uploadPending, .uploadPending),
        ]
        for (raw, expected) in reasons {
            #expect(CoreBridge.ReviewUndoStatus(.unavailable(reason: raw)) == .unavailable(reason: expected))
        }
        #expect(CoreBridge.ReviewUndoScope.global.raw == .global)
        #expect(CoreBridge.ReviewUndoScope.session(sessionId: .max).raw == .session(sessionId: .max))
        let raw = MeguriCoreBindings.ReviewStepInfo(id: .max, name: "Review bridge step", kind: .answer, session: 91,
                                                  longWrite: .init(seconds: 3, nanoseconds: 123_456_789))
        #expect(CoreBridge.ReviewUndoStatus(.available(step: raw)) == .available(step: .init(id: .max, name: "Review bridge step", kind: .answer, session: 91, longWrite: .init(seconds: 3, nanoseconds: 123_456_789))))
        #expect(CoreBridge.ReviewStepInfo(.init(id: 1, name: "Other step", kind: .other, session: nil, longWrite: nil)) == .init(id: 1, name: "Other step", kind: .other, session: nil, longWrite: nil))
        let ratings: [(CoreBridge.ReviewRating, MeguriCoreBindings.ReviewRating)] = [(.again, .again), (.hard, .hard), (.good, .good), (.easy, .easy)]
        for (value, expected) in ratings { #expect(value.raw == expected) }
    }

    @Test func allSessionEventsKeepPayloadSequenceGenerationAndCause() {
        let finished = CoreBridge.ReviewDeckStudyState(Self.finished)
        #expect(finished == .init(deck: -4, queueCounts: .init(new: 1, learning: 2, review: 3), listCounts: .init(new: 4, learning: 5, review: 6), difference: .init(new: -3, learning: 3, review: -9), finished: true, learningRemainingToday: 14, learningLaterToday: 15, nextLearningDue: -123, nextLearningReadyAt: -456, reviewLimitReached: true, newLimitReached: false, learningRemaining: 16, secsUntilNextLearn: -17, reviewRemaining: false, newRemaining: true, siblingBuried: 18, userBuried: 19, tomorrow: .init(new: 7, learning: 8, review: 9), empty: false))
        let events: [(MeguriCoreBindings.CoreEventKind, CoreBridge.CoreEvent.Kind)] = [
            (.sessionStarted(sessionId: 91, deckId: -4), .sessionStarted(sessionId: 91, deckId: -4)),
            (.sessionEnded(sessionId: 91), .sessionEnded(sessionId: 91)),
            (.queueHeadChanged(sessionId: 91, diff: Self.diff), .queueHeadChanged(sessionId: 91, diff: CoreBridge.ReviewHeadDiff(Self.diff))),
            (.currentCardChanged(sessionId: 91, cardId: -41, change: .eligible), .currentCardChanged(sessionId: 91, cardId: -41, change: .eligible)),
            (.currentCardChanged(sessionId: 91, cardId: -41, change: .removed), .currentCardChanged(sessionId: 91, cardId: -41, change: .removed)),
            (.currentCardChanged(sessionId: 91, cardId: -41, change: .memoryOnly), .currentCardChanged(sessionId: 91, cardId: -41, change: .memoryOnly)),
            (.commitFailed(sessionId: 91, cardId: -41, error: .CommitCompleted(seq: 73, message: "durability follow-up")), .commitFailed(sessionId: 91, cardId: -41, error: .commitCompleted(sequence: 73, message: "durability follow-up"))),
            (.waitingUntil(sessionId: 91, instant: -123), .waitingUntil(sessionId: 91, instant: -123)),
            (.finished(sessionId: 91, state: Self.finished), .finished(sessionId: 91, state: finished)),
            (.presetUnsupported(sessionId: 91, presetIds: [-3, 7]), .presetUnsupported(sessionId: 91, presetIds: [-3, 7])),
            (.recoveryDropped(event: .max, cardId: -41, reason: "stale accepted context"), .recoveryDropped(event: .max, cardId: -41, reason: "stale accepted context")),
        ]
        for (offset, pair) in events.enumerated() {
            let event = CoreBridge.CoreEvent(.init(seq: UInt64(100 + offset), generation: 29, causeSeq: 17, kind: pair.0))
            #expect(event.seq == UInt64(100 + offset) && event.generation == 29 && event.causeSeq == 17)
            #expect(event.kind == pair.1)
        }
        let errors: [(MeguriCoreBindings.CoreError, BridgeError)] = [
            (.Cancelled, .cancelled), (.Busy(runningOp: "write"), .busy(runningOperation: "write")),
            (.Unavailable(reason: "closed"), .unavailable(reason: "closed")), (.Unsupported(feature: "preset"), .unsupported(feature: "preset")),
            (.InvalidArgument(message: "input"), .invalidArgument("input")), (.Internal(message: "failure"), .internal("failure")),
            (.MediaCommitPending(seq: 74, message: "media"), .mediaCommitPending(sequence: 74, message: "media")),
            (.InvalidTypeAnswerReplacement, .invalidTypeAnswerReplacement),
        ]
        for (raw, expected) in errors {
            let event = CoreBridge.CoreEvent(.init(seq: 200, generation: 29, causeSeq: nil, kind: .commitFailed(sessionId: 91, cardId: -41, error: raw)))
            #expect(event.kind == .commitFailed(sessionId: 91, cardId: -41, error: expected))
            #expect(event.causeSeq == nil)
        }
    }

    @Test func renderedMediaRefsSurviveAlongsideHTMLAudioAndTypeIn() {
        let raw = MeguriCoreBindings.RenderedCard(cardId: -41, ordinal: 2, mode: .review, question: "<img src=\"q.png\">", answer: "<span data-meguri-type-answer></span>", questionAv: [.sound(name: "q.mp3")], answerAv: [.tts(language: "en", content: "answer")], outcome: .rendered, css: ".card {}", hidePlayButtons: true, reviewExtra: "extra", flag: 7, marked: true, typeIn: .init(expected: "answer", mode: .nc, caseInsensitive: true, font: "Serif", size: 22), mediaRefs: ["q.png", "q.mp3", "a.woff2"])
        let mapped = CoreBridge.RenderedCard(raw)
        #expect(mapped.mediaRefs == ["q.png", "q.mp3", "a.woff2"])
        #expect(mapped.cardID == -41 && mapped.ordinal == 2 && mapped.mode == .review)
        #expect(mapped.question == "<img src=\"q.png\">" && mapped.answer == "<span data-meguri-type-answer></span>")
        #expect(mapped.questionAudio == [.sound(name: "q.mp3")] && mapped.answerAudio == [.tts(language: "en", content: "answer")])
        #expect(mapped.outcome == .rendered && mapped.css == ".card {}" && mapped.hidePlayButtons && mapped.reviewExtra == "extra")
        #expect(mapped.flag == 7 && mapped.marked)
        #expect(mapped.typeIn == TypeAnswerDescriptor(expected: "answer", mode: .nc, caseInsensitive: true, font: "Serif", size: 22))
    }


    @Test func malformedTokensAndRenderKeysReportTypedArgumentErrors() async {
        for count in [0, 31, 33] {
            do {
                _ = try await Core.answerSession(sessionID: 91, cardID: -41, rating: .again, stateToken: .init(bytes: Array(repeating: 0, count: count)), answeredAtMs: 1, elapsedMs: 1)
                Issue.record("Malformed state token accepted")
            } catch { guard case .invalidArgument = error else { Issue.record("\(error)"); continue } }
            do {
                _ = try await Core.renderSessionHead(sessionID: 91, keys: [.init(bytes: Array(repeating: 0, count: count))], platform: .mac)
                Issue.record("Malformed render key accepted")
            } catch { guard case .invalidArgument = error else { Issue.record("\(error)"); continue } }
        }
    }
}

// Collection ownership is process-wide; serialize this refusal with U37 opens.
extension EventTests {
    @Test func closedReviewCallsUseRegisteredLanes() async {
        // U37 open/read/close tests share this serialized suite.
        let calls: [() async throws -> Void] = [
            { _ = try await Core.startSession(deckID: 1, resumeCardID: -41) },
            { _ = try await Core.peekFirstCard(deckID: 1) },
            { _ = try await Core.sessionHead(sessionID: 91, count: 3) },
            { _ = try await Core.refreshSessionCard(sessionID: 91, cardID: -41) },
            { _ = try await Core.renderSessionHead(sessionID: 91, keys: [.init(bytes: Array(repeating: 0xa3, count: 32))], platform: .mac) },
            { _ = try await Core.answerSession(sessionID: 91, cardID: -41, rating: .good, stateToken: .init(bytes: Array(repeating: 0x17, count: 32)), answeredAtMs: 1_894_701_600_000, elapsedMs: .max) },
            { try await Core.endSession(sessionID: 91) },
            { _ = try await Core.peekReviewUndo(scope: .session(sessionId: 91)) },
            { _ = try await Core.peekReviewUndo(scope: .global) },
            { _ = try await CoreUndoService().startUndo(scope: .session(sessionId: 91)) },
            { _ = try await CoreUndoService().startRedo(scope: .global) },
            { try await Core.flushReviewForBackground() },
        ]
        for call in calls {
            do { try await call(); Issue.record("Expected closed-collection refusal") }
            catch let error as BridgeError {
                guard case .unavailable = error else { Issue.record("Expected unavailable, got \(error)"); continue }
            } catch { Issue.record("Untyped bridge error: \(error)") }
        }
    }

}
