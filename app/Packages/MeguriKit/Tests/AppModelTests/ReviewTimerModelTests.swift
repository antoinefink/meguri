import Foundation
import Testing
@testable import AppModel
import CoreBridge

extension ReviewCoordinatorTests {
    @Test(arguments: [false, true])
    func displayFreezeDoesNotFreezeRecordedAnswer(stopOnAnswer: Bool) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.head(1, showTimer: true, stopTimerOnAnswer: stopOnAnswer)
        await fixture.model.start()
        #expect(fixture.model.timerPresentation.text == "00:00")
        #expect(fixture.model.timerPresentation.isCounting)
        fixture.now = 12.75
        fixture.model.reveal(); await fixture.model.settled()
        #expect(fixture.model.timerPresentation.text == "00:12")
        #expect(fixture.model.timerPresentation.isCounting == !stopOnAnswer)
        fixture.now = 20.5
        #expect(fixture.model.timerPresentation.text == (stopOnAnswer ? "00:12" : "00:20"))
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.client.elapsed == [20_500])
        #expect(fixture.model.record.answers.first?.elapsedMilliseconds == 20_500)
        #expect(fixture.client.used.first?.3 == 1_800_000_000_000)
        // The next card's hidden preset suppresses display work, not answer time.
        #expect(!fixture.model.timerPresentation.isVisible)
        #expect(!fixture.model.timerPresentation.isCounting)
        #expect(fixture.model.timerPresentation.text == "00:00")
        _ = await fixture.model.end()
    }

    @Test(arguments: [false, true])
    func capStaysVisibleOnBothSidesAndNextCardRestarts(stopOnAnswer: Bool) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.head(1, showTimer: true, stopTimerOnAnswer: stopOnAnswer)
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(2, showTimer: true,
            maximumAnswerSeconds: 120), marks: [], removed: [1]), leech: false, suspended: false, rolledOver: false)
        await fixture.model.start()
        fixture.now = 75
        #expect(fixture.model.timerPresentation.text == "01:00")
        #expect(fixture.model.timerPresentation.capped)
        #expect(!fixture.model.timerPresentation.isCounting)
        fixture.model.reveal(); await fixture.model.settled()
        fixture.now = 90
        #expect(fixture.model.timerPresentation.text == "01:00")
        #expect(fixture.model.timerPresentation.capped)
        #expect(!fixture.model.timerPresentation.isCounting)
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.client.elapsed == [60_000])
        #expect(fixture.model.timerPresentation.text == "00:00")
        #expect(fixture.model.timerPresentation.capSeconds == 120)
        #expect(!fixture.model.timerPresentation.capped)
        #expect(fixture.model.timerPresentation.isCounting)
        fixture.now = 95
        #expect(fixture.model.timerPresentation.text == "00:05")
        _ = await fixture.model.end()
    }

    @Test(arguments: [false, true])
    func pauseResumeKeepsTheCurrentAnswerAndFrozenDisplay(stopOnAnswer: Bool) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.head(1, showTimer: true, stopTimerOnAnswer: stopOnAnswer)
        await fixture.model.start()
        fixture.now = 5
        try await fixture.model.pause()
        fixture.now = 605
        #expect(fixture.model.timerPresentation.text == "00:05")
        #expect(!fixture.model.timerPresentation.isCounting)
        try await fixture.model.resume()
        #expect(fixture.model.timerPresentation.isCounting)
        fixture.now = 608
        #expect(fixture.model.timerPresentation.text == "00:08")
        fixture.model.reveal(); await fixture.model.settled()
        try await fixture.model.pause()
        fixture.now = 1_208
        try await fixture.model.resume()
        fixture.now = 1_210
        #expect(fixture.model.timerPresentation.text == (stopOnAnswer ? "00:08" : "00:10"))
        #expect(fixture.model.timerPresentation.isCounting == !stopOnAnswer)
        _ = await fixture.model.end()
    }

    @Test func refreshedPresetChangesVisibilityAndCapWithoutResettingElapsed() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.now = 5.25
        #expect(!fixture.model.timerPresentation.isVisible)
        #expect(!fixture.model.timerPresentation.isCounting)
        fixture.client.currentHead = Self.head(1, showTimer: true)
        fixture.model.requestRefresh(); await fixture.model.settled()
        #expect(fixture.model.timerPresentation.text == "00:05")
        #expect(fixture.model.timerPresentation.isCounting)
        fixture.now = 65
        #expect(fixture.model.timerPresentation.capped)
        fixture.client.currentHead = Self.head(1, showTimer: true, maximumAnswerSeconds: 120)
        fixture.model.requestRefresh(); await fixture.model.settled()
        #expect(fixture.model.timerPresentation.text == "01:05")
        #expect(fixture.model.timerPresentation.isCounting)
        fixture.client.currentHead = Self.head(1)
        fixture.model.requestRefresh(); await fixture.model.settled()
        #expect(!fixture.model.timerPresentation.isVisible)
        #expect(!fixture.model.timerPresentation.isCounting)
        fixture.client.currentHead = Self.head(1, showTimer: true, maximumAnswerSeconds: 120)
        fixture.model.requestRefresh(); await fixture.model.settled()
        #expect(fixture.model.timerPresentation.text == "01:05")
        #expect(fixture.model.timerPresentation.isCounting)
        _ = await fixture.model.end()
    }

    @Test func timerArithmeticUsesCurrentProductionClock() async {
        let fixture = Fixture()
        fixture.client.currentHead = Self.head(1, showTimer: true, maximumAnswerSeconds: 7_200)
        await fixture.model.start()
        for (seconds, text) in [(0, "00:00"), (5, "00:05"), (60, "01:00"), (754, "12:34"), (7_200, "120:00")] {
            fixture.now = Double(seconds)
            #expect(fixture.model.timerPresentation.text == text)
            #expect(fixture.model.timerPresentation.capped == (seconds == 7_200))
        }
        fixture.now = 7_500
        #expect(fixture.model.timerPresentation.text == "120:00")
        #expect(!fixture.model.timerPresentation.isCounting)
        _ = await fixture.model.end()
    }
}
