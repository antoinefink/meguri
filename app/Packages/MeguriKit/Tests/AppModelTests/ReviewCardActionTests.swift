@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// N20.8 and `review/card-actions` A4, A5 at the model level: Bury Card and
/// Suspend Card on the presented card, the next card, the announcement, the
/// error path, and an undo that leaves the answers alone.
extension ReviewCoordinatorTests {
    private static func mutation(next: Int64, removed: Int64) -> SchedMutation {
        let changes = ChangeSet(wholeCollection: false, source: .userUndoable, cards: [], notes: [], notetypes: [],
            decks: [], deckConfigs: [], configKeys: [], tags: [], revlogAdded: [], revlogRemoved: [],
            mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
            skippedIncomingDecks: [], collectionRow: false)
        return SchedMutation(summary: MutationSummary(seq: 1, generation: 1, undoName: "Bury", changes: changes,
            pastCancelPoint: true), count: 1, headDiff: ReviewHeadDiff(head: head(next), marks: [], removed: [removed]))
    }

    @Test(arguments: [ReviewCardAction.bury, .suspend])
    func aCardActionShowsTheNextCardOnItsQuestionAndAnnouncesIt(action: ReviewCardAction) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        var calls: [(ReviewCardAction, Int64, UInt64)] = []
        fixture.model.startCardAction = { action, card, session in
            calls.append((action, card, session))
            return Self.mutation(next: 2, removed: card)
        }
        #expect(fixture.model.canActOnCard)
        fixture.model.act(action)
        #expect(!fixture.model.canActOnCard, "One action at a time")
        await fixture.model.settled()
        #expect(calls.count == 1 && calls.first?.0 == action && calls.first?.1 == 1 && calls.first?.2 == 5)
        #expect(fixture.model.item?.cardId == 2 && fixture.model.phase == .question)
        #expect(fixture.model.cardActionAnnouncement?.action == action)
        #expect(fixture.client.answers == 0 && fixture.model.record.answers.isEmpty, "No answer is recorded (A1)")
        _ = await fixture.model.end()
    }

    @Test(arguments: [false, true])
    func aFailedCardActionKeepsTheCardOnItsSideAndSaysWhy(answerSide: Bool) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.now = 10
        if answerSide { fixture.model.reveal(); await fixture.model.settled() }
        let presented = fixture.surface.presented
        fixture.model.startCardAction = { _, _, _ in throw BridgeError.internal("refused") }
        fixture.now = 12
        fixture.model.act(.suspend); await fixture.model.settled()
        #expect(fixture.model.item?.cardId == 1 && fixture.model.phase == (answerSide ? .answer : .question))
        #expect(fixture.surface.presented == presented, "Nothing is presented again (A1)")
        #expect(fixture.model.notice != nil && fixture.model.cardActionAnnouncement == nil)
        if answerSide {
            fixture.now = 15
            fixture.model.rate(.good); await fixture.model.settled()
            #expect(fixture.client.elapsed == [15_000], "12 s before the action and 3 s after: the timer went on")
        }
        _ = await fixture.model.end()
    }

    @Test func undoingABuryLeavesTheAnswersAlone() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.model.record.answers.count == 1)
        fixture.model.startCardAction = { _, card, _ in Self.mutation(next: 3, removed: card) }
        // The bury is the newest step; the screen reads it after the bury.
        fixture.client.undoStep = (name: "Bury", kind: .other)
        fixture.model.act(.bury); await fixture.model.settled()
        fixture.client.restoredHead = Self.head(2)
        fixture.model.restore(); await fixture.model.settled()
        #expect(fixture.model.item?.cardId == 2, "The buried card comes back on screen")
        #expect(fixture.model.record.answers.count == 1 && fixture.model.record.undone.isEmpty)
        _ = await fixture.model.end()
    }

    @Test func cardActionsWaitWhileTheEditFormIsOpen() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        #expect(fixture.model.beginEditingNote())
        #expect(!fixture.model.canActOnCard)
        var calls = 0
        fixture.model.startCardAction = { _, card, _ in calls += 1; return Self.mutation(next: 2, removed: card) }
        fixture.model.act(.bury); await fixture.model.settled()
        #expect(calls == 0 && fixture.model.item?.cardId == 1)
        fixture.model.endEditingNote(saved: false); await fixture.model.settled()
        #expect(fixture.model.canActOnCard)
        _ = await fixture.model.end()
    }

    @Test func reviewStepsTakeTheirDisplayNames() {
        #expect(ReviewStepNames.display("Answer Card") == "Answer")
        #expect(ReviewStepNames.display("Bury") == "Bury Card")
        #expect(ReviewStepNames.display("Suspend") == "Suspend Card")
        #expect(ReviewStepNames.display("Update Note") == "Edit Note")
        #expect(ReviewStepNames.display("Rename Deck") == "Rename Deck")
    }
}
