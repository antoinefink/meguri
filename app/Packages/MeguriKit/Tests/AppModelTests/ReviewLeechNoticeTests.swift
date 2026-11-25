@testable import CoreBridge
import Testing
@testable import AppModel

/// N16.11: a leech answer's notice is shown as one icon in the top controls,
/// for exactly as long as the notice lasts (`review/reviewer-strings#S3`).
extension ReviewCoordinatorTests {
    @Test(arguments: [false, true])
    func leechAnswerIsShownAsTheIcon(suspended: Bool) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        #expect(fixture.model.leechNotice == nil)
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(2), marks: [], removed: [1]),
            leech: true, suspended: suspended, rolledOver: false)
        fixture.model.rate(.again); await fixture.model.settled()
        // The answer path's own text maps to the icon and its variant.
        let notice = try #require(fixture.model.leechNotice)
        #expect(notice.suspended == suspended)
        // Another notice is its own banner, and the icon goes.
        fixture.model.showNotice("Another notice")
        #expect(fixture.model.leechNotice == nil)
        #expect(await fixture.model.end())
    }

    /// N16.11 (2026-11-25): the notice names the answered card, not the one on
    /// screen, and its question arrives from the core's excerpt of that card.
    @Test func leechNoticeNamesTheAnsweredCard() async throws {
        let fixture = Fixture()
        fixture.client.questions = [1: "余程"]
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(2), marks: [], removed: [1]),
            leech: true, suspended: false, rolledOver: false)
        fixture.model.rate(.again); await fixture.model.settled()
        await fixture.model.leechQuestionSettled()
        let notice = try #require(fixture.model.leechNotice)
        #expect(fixture.model.shownItem?.cardId == 2 && notice.cardID == 1)
        #expect(notice.question == "余程" && fixture.client.questionReads == [1])
        // The sample head's next state is a new card's: no lapse count to show.
        #expect(notice.lapses == nil)
        #expect(await fixture.model.end())
    }

    /// A question read that comes back after its notice went changes nothing.
    @Test func aLateQuestionNeverReachesAnotherNotice() async throws {
        let fixture = Fixture()
        fixture.client.questions = [1: "余程"]
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.head(2), marks: [], removed: [1]),
            leech: true, suspended: false, rolledOver: false)
        fixture.model.rate(.again); await fixture.model.settled()
        fixture.model.showNotice("Another notice")
        await fixture.model.leechQuestionSettled()
        #expect(fixture.model.leechNotice == nil && fixture.model.notice == "Another notice")
        #expect(await fixture.model.end())
    }

    /// The count is the lapse count the answer gave the card: a review card's
    /// Again relearns or stays in review, and both carry it.
    @Test func leechNoticeCountsTheLapsesOfTheAnswer() throws {
        let item = try #require(Self.head(1).items.first)
        func answer(_ state: ReviewSchedulingState) -> ReviewAnswerState {
            ReviewAnswerState(state: state, delay: .days(amount: 1), left: 0, factor: 0,
                revlog: ReviewRevlogPreview(interval: 1, lastInterval: 0, kind: 0, factor: 0),
                label: ReviewAnswerLabel(lessThan: false, amount: "1", unit: .days))
        }
        let review = ReviewReviewState(scheduledDays: 1, elapsedDays: 0, easeFactor: 2.3, lapses: 8, leeched: true, memoryState: nil)
        let learning = ReviewLearningState(remainingSteps: 1, scheduledSecs: 600, elapsedSecs: 0, memoryState: nil)
        let kept = ReviewReviewState(scheduledDays: 9, elapsedDays: 0, easeFactor: 2.5, lapses: 7, leeched: false, memoryState: nil)
        func card(again: ReviewSchedulingState) -> ReviewHeadItem {
            ReviewHeadItem(cardId: item.cardId, noteId: item.noteId, kind: .review, stateToken: item.stateToken,
                renderKey: item.renderKey,
                nextStates: ReviewSchedulingStates(current: .review(state: kept), again: answer(again),
                    hard: answer(.review(state: kept)), good: answer(.review(state: kept)), easy: answer(.review(state: kept))),
                predictedNext: item.predictedNext, flag: 0, marked: false, homePreset: item.homePreset)
        }
        #expect(ReviewLeechNotice.lapses(after: .again, of: card(again: .relearning(learning: learning, review: review))) == 8)
        #expect(ReviewLeechNotice.lapses(after: .again, of: card(again: .review(state: review))) == 8)
        #expect(ReviewLeechNotice.lapses(after: .good, of: card(again: .review(state: review))) == 7)
        #expect(ReviewLeechNotice.lapses(after: .again, of: item) == nil, "a new card's next state has none")
    }

    @Test func answersThatAreNotLeechesShowNoIcon() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.again); await fixture.model.settled()
        #expect(fixture.model.leechNotice == nil)
        #expect(ReviewLeechNotice(notice: "Card changed, please answer again") == nil)
        #expect(await fixture.model.end())
    }
}
