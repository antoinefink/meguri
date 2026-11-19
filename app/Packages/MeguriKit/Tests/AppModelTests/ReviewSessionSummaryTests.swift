@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

extension ReviewCoordinatorTests {
    @Test func progressCountsAnswersAgainstAnswersPlusRemaining() {
        let empty = ReviewSessionProgress(answered: 0, remaining: 0)
        #expect(empty.total == 0 && empty.fraction == 0)
        let part = ReviewSessionProgress(answered: 12, remaining: 28)
        #expect(part.answered == 12 && part.total == 40 && part.fraction == 0.3)
        let done = ReviewSessionProgress(answered: 5, remaining: 0)
        #expect(done.total == 5 && done.fraction == 1)
        let invalid = ReviewSessionProgress(answered: -1, remaining: -4)
        #expect(invalid.answered == 0 && invalid.total == 0)
    }

    @Test func summaryDurationShowsSecondsThenRoundedMinutes() {
        #expect(ReviewSessionSummary(cards: 1, seconds: 0).duration == .seconds(0))
        #expect(ReviewSessionSummary(cards: 1, seconds: 59).duration == .seconds(59))
        #expect(ReviewSessionSummary(cards: 1, seconds: 60).duration == .seconds(60))
        #expect(ReviewSessionSummary(cards: 1, seconds: 89).duration == .seconds(60))
        #expect(ReviewSessionSummary(cards: 1, seconds: 90).duration == .seconds(120))
        #expect(ReviewSessionSummary(cards: 1, seconds: 12 * 60 + 20).duration == .seconds(12 * 60))
    }

    @Test func finishedSessionReportsDistinctCardsAndActiveTime() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        #expect(fixture.model.sessionProgress == ReviewSessionProgress(answered: 0, remaining: 1))
        #expect(fixture.model.sessionSummary == ReviewSessionSummary(cards: 0, seconds: 0))
        fixture.now = 4
        fixture.model.reveal(); await fixture.model.settled()
        // The same card comes back once, then the session finishes.
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(1), marks: [], removed: []),
                                               leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.again); await fixture.model.settled()
        #expect(fixture.model.sessionProgress == ReviewSessionProgress(answered: 1, remaining: 1))
        #expect(fixture.model.progress == 0.5)
        fixture.now = 9
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.emptyHead(.finished), marks: [], removed: [1]),
                                               leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.model.phase == .finished)
        #expect(fixture.model.sessionProgress == ReviewSessionProgress(answered: 2, remaining: 0))
        #expect(fixture.model.sessionSummary == ReviewSessionSummary(cards: 1, seconds: 9))
        // The clock stopped with the session: waiting on the end screen adds nothing.
        fixture.now = 500
        #expect(fixture.model.sessionSummary.seconds == 9)
        #expect(await fixture.model.end())
    }
}
