import Foundation
import Synchronization
import Testing

@testable import Instrumentation

/// The R1862 contract read per KTD11: one token per swap, the interval ends
/// at the presented frame, and the post-swap confirmation must name the
/// swap's card.
@MainActor
@Suite struct VisibleConfirmationTests {
    /// What the recorder did with its interval, in order.
    final class Events: Sendable {
        let log = Mutex<[String]>([])
        var all: [String] { log.withLock { $0 } }

        var emitter: IntervalEmitter {
            IntervalEmitter(
                begin: { interval in
                    self.log.withLock { $0.append("begin \(interval.name)") }
                    return Signposts.begin(interval)
                },
                end: { token in
                    self.log.withLock { $0.append("end \(token.interval.name)") }
                    Signposts.end(token)
                })
        }
    }

    /// The samples a recorder reported.
    @MainActor final class Samples {
        var all: [VisibleSample] = []
    }

    func makeRecorder(_ events: Events, _ samples: Samples) -> SwapRecorder {
        SwapRecorder(emitter: events.emitter) { samples.all.append($0) }
    }

    @Test func theMessageContract() {
        #expect(VisibleConfirmation.messageName == "meguriVisible")
        let message = VisibleConfirmation.Message(token: 3, cardID: 1_700_000_000_123, phase: .visible)
        #expect(VisibleConfirmation.Message(body: message.body) == message)
        // As WKScriptMessage delivers a JavaScript object: NSNumber values.
        let fromPage: [String: Any] = [
            "token": NSNumber(value: 3), "card": NSNumber(value: 1_700_000_000_123.0), "phase": "visible",
        ]
        #expect(VisibleConfirmation.Message(body: fromPage) == message)
        #expect(VisibleConfirmation.Message(body: ["token": 1, "card": 2, "phase": "settled"])?.phase == .settled)
    }

    @Test func malformedBodiesAreNotConfirmations() {
        let bad: [Any] = [
            "visible",
            ["token": 1, "card": 2],
            ["token": 1, "card": 2, "phase": "shown"],
            ["token": -1, "card": 2, "phase": "visible"],
            ["token": 1, "card": 2.5, "phase": "visible"],
            ["token": "1", "card": 2, "phase": "visible"],
        ]
        for body in bad {
            #expect(VisibleConfirmation.Message(body: body) == nil, "\(body)")
        }
    }

    @Test func theIntervalEndsAtThePresentedFrameNotAtTheConfirmation() {
        let events = Events()
        let samples = Samples()
        let recorder = makeRecorder(events, samples)
        let token = recorder.beginSwap(to: 42, inputTime: 10.000)
        #expect(events.all == ["begin review.answerToNextVisible"])
        recorder.presented(token, frameTime: 10.008)
        #expect(events.all == ["begin review.answerToNextVisible", "end review.answerToNextVisible"])
        #expect(samples.all.isEmpty, "a sample waits for its confirmation")

        recorder.receive(["token": token.sequence, "card": 42, "phase": "visible"], at: 10.040)
        #expect(events.all.count == 2, "the confirmation does not touch the interval")
        #expect(samples.all.count == 1)
        let sample = samples.all[0]
        #expect(sample.outcome == .confirmed)
        #expect(!sample.isMiss)
        #expect(sample.token == token)
        #expect(sample.presentedTime == 10.008)
        #expect(sample.confirmedTime == 10.040)
        #expect(abs(sample.inputToPresented - 0.008) < 1e-9)
    }

    @Test func aConfirmationNamingAnotherCardIsAMiss() {
        let samples = Samples()
        let recorder = makeRecorder(Events(), samples)
        let token = recorder.beginSwap(to: 42, inputTime: 1)
        recorder.presented(token, frameTime: 1.008)
        recorder.receive(["token": token.sequence, "card": 41, "phase": "visible"], at: 1.03)
        #expect(samples.all.map(\.outcome) == [.wrongCard(reported: 41)])
        #expect(samples.all[0].isMiss)
    }

    @Test func aSwapNeverConfirmedBeforeTheNextOneIsAMiss() {
        let samples = Samples()
        let recorder = makeRecorder(Events(), samples)
        let first = recorder.beginSwap(to: 1, inputTime: 1)
        recorder.presented(first, frameTime: 1.008)
        let second = recorder.beginSwap(to: 2, inputTime: 2)
        #expect(samples.all.map(\.outcome) == [.unconfirmed])
        #expect(samples.all[0].token == first)
        #expect(samples.all[0].confirmedTime == nil)
        #expect(second.sequence == first.sequence + 1)
    }

    @Test func theLastSwapStillUnconfirmedWhenTheSessionFinishesIsAMiss() {
        let samples = Samples()
        let recorder = makeRecorder(Events(), samples)
        let first = recorder.beginSwap(to: 1, inputTime: 1)
        recorder.presented(first, frameTime: 1.008)
        recorder.receive(["token": first.sequence, "card": 1, "phase": "visible"], at: 1.04)
        let last = recorder.beginSwap(to: 2, inputTime: 2)
        recorder.presented(last, frameTime: 2.008)
        #expect(samples.all.map(\.outcome) == [.confirmed], "the last swap still waits for its confirmation")

        recorder.finish()
        #expect(samples.all.map(\.outcome) == [.confirmed, .unconfirmed])
        #expect(samples.all[1].token == last)
        #expect(samples.all[1].presentedTime == 2.008)
        #expect(samples.all[1].confirmedTime == nil)

        // The session is over: a late confirmation adds nothing, and a
        // second finish reports nothing again.
        recorder.receive(["token": last.sequence, "card": 2, "phase": "visible"], at: 2.05)
        recorder.finish()
        #expect(samples.all.count == 2)
    }

    @Test func finishReportsWaitingSwapsInSwapOrderAndForgetsOnesNeverPresented() {
        let samples = Samples()
        let recorder = makeRecorder(Events(), samples)
        // Two swaps begun before either was presented: neither reports the
        // other, so both are still waiting at the end.
        let tokens = (1...2).map { recorder.beginSwap(to: Int64($0), inputTime: Double($0)) }
        for token in tokens.reversed() {
            recorder.presented(token, frameTime: Double(token.cardID) + 0.008)
        }
        let neverPresented = recorder.beginSwap(to: 3, inputTime: 3)
        #expect(samples.all.map(\.token) == tokens, "the third swap reports both, in swap order")
        samples.all.removeAll()

        let more = (4...9).map { recorder.beginSwap(to: Int64($0), inputTime: Double($0)) }
        for token in more.shuffled() {
            recorder.presented(token, frameTime: Double(token.cardID) + 0.008)
        }
        recorder.finish()
        #expect(samples.all.map(\.token) == more)
        #expect(samples.all.allSatisfy { $0.outcome == .unconfirmed })
        recorder.presented(neverPresented, frameTime: 3.008)
        recorder.receive(["token": neverPresented.sequence, "card": 3, "phase": "visible"], at: 3.04)
        #expect(samples.all.count == more.count, "a swap that never reached the screen is no sample, and finish forgot it")
    }

    @Test func aConfirmationBeforeThePresentedCallbackCompletesAtIt() {
        let events = Events()
        let samples = Samples()
        let recorder = makeRecorder(events, samples)
        let token = recorder.beginSwap(to: 7, inputTime: 5)
        recorder.receive(["token": token.sequence, "card": 7, "phase": "visible"], at: 5.03)
        #expect(samples.all.isEmpty)
        #expect(events.all == ["begin review.answerToNextVisible"])
        recorder.presented(token, frameTime: 5.008)
        #expect(samples.all.map(\.outcome) == [.confirmed])
        #expect(samples.all[0].presentedTime == 5.008)
    }

    @Test func settledMessagesAndUnknownTokensDoNotCompleteASwap() {
        let samples = Samples()
        let recorder = makeRecorder(Events(), samples)
        let token = recorder.beginSwap(to: 7, inputTime: 5)
        recorder.presented(token, frameTime: 5.008)
        #expect(recorder.receive(["token": token.sequence, "card": 7, "phase": "settled"], at: 5.01)?.phase == .settled)
        #expect(recorder.receive(["token": token.sequence + 9, "card": 7, "phase": "visible"], at: 5.02) != nil)
        #expect(recorder.receive("not a message", at: 5.03) == nil)
        #expect(samples.all.isEmpty)
        recorder.receive(["token": token.sequence, "card": 7, "phase": "visible"], at: 5.04)
        #expect(samples.all.map(\.outcome) == [.confirmed])
    }
}
