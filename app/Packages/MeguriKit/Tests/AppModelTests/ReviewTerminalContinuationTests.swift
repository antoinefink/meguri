@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

extension ReviewCoordinatorTests {
    @Test(arguments: [false, true])
    func automaticActiveHeadKeepsTerminalContentUntilFreshContinue(waiting: Bool) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.emptyHead(waiting ? .waitingUntil(instant: 1_800_000_600) : .finished)
        fixture.client.summary = Self.summary(waiting: !waiting) // deliberately conflicts with the head
        await fixture.model.start()
        await fixture.model.summaryTask?.value
        let terminalPhase = fixture.model.phase
        #expect(!fixture.model.continueAvailable && !fixture.model.canContinue)
        fixture.model.continueReview()
        #expect(fixture.client.headReads == 0)
        fixture.now = 50
        fixture.client.currentHead = Self.head(2)
        fixture.model.requestRefresh()
        #expect(fixture.model.phase == terminalPhase, "Refresh must not flash a retained card via transition")
        await fixture.model.settled()
        #expect(fixture.model.phase == terminalPhase && fixture.model.item?.cardId == 2)
        #expect(fixture.model.counts == Self.head(2).counts)
        #expect(fixture.surface.calls == ["load"] && fixture.surface.presented.isEmpty)
        #expect(fixture.model.visibleMediaNames.isEmpty)
        #expect(fixture.model.record.activeMilliseconds == 0 && fixture.model.canContinue)
        fixture.now = 100
        fixture.client.currentHead = Self.head(3)
        let reads = fixture.client.headReads
        fixture.model.continueReview(); fixture.model.continueReview()
        await fixture.model.settled()
        #expect(fixture.client.headReads == reads + 1)
        #expect(fixture.model.phase == .question && fixture.model.item?.cardId == 3)
        #expect(fixture.surface.presented == [3] && fixture.surface.calls == ["load", "render", "present"])
        #expect(fixture.model.record.activeMilliseconds == 0 && !fixture.model.canContinue)
        fixture.now = 103
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.client.elapsed == [3_000])
        #expect(fixture.model.record.activeMilliseconds == 3_000)
        #expect(await fixture.model.end())
    }

    @Test(arguments: [false, true])
    func continueWithEmptyHeadDoesNotLeaveIntentForLaterRefresh(future: Bool) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.emptyHead(.waitingUntil(instant: 1_800_000_000))
        fixture.client.summary = Self.summary(waiting: false)
        await fixture.model.start()
        #expect(fixture.model.continueAvailable && fixture.model.canContinue)
        fixture.client.currentHead = Self.emptyHead(.waitingUntil(instant: future ? 1_800_000_600 : 1_800_000_000))
        fixture.model.continueReview(); await fixture.model.settled()
        #expect(fixture.client.headReads == 1 && fixture.model.phase == .waiting)
        #expect(fixture.model.continueAvailable == !future)
        #expect(fixture.surface.calls == ["load"])
        fixture.client.currentHead = Self.head(2)
        fixture.model.requestRefresh(); await fixture.model.settled()
        #expect(fixture.model.phase == .waiting && fixture.model.canContinue)
        #expect(fixture.surface.presented.isEmpty && fixture.model.record.activeMilliseconds == 0)
        fixture.model.continueReview(); await fixture.model.settled()
        #expect(fixture.model.phase == .question && fixture.surface.presented == [2])
        #expect(await fixture.model.end())
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func terminalRefreshBusyAndResumeKeepPresentationHeld(waiting: Bool) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.emptyHead(waiting ? .waitingUntil(instant: 1_800_000_600) : .finished)
        fixture.client.summary = Self.summary(waiting: false)
        await fixture.model.start()
        let phase = fixture.model.phase
        let held = CancellableGate()
        defer { held.release() }
        fixture.client.currentHead = Self.head(2)
        fixture.client.onHeadResult = { try await held.wait() }
        fixture.model.requestRefresh()
        await held.requested()
        #expect(fixture.model.phase == phase && !fixture.model.canContinue)
        fixture.model.continueReview() // busy must reject, never queue an intent
        fixture.model.requestRefresh()
        fixture.model.beginPause()
        fixture.model.continueReview()
        held.release()
        try await fixture.model.pause()
        #expect(fixture.model.paused && fixture.surface.presented.isEmpty)
        #expect(!fixture.model.canContinue && fixture.model.record.activeMilliseconds == 0)
        fixture.client.onHeadResult = nil
        fixture.client.currentHead = Self.head(3)
        fixture.now = 100
        try await fixture.model.resume()
        #expect(!fixture.model.paused && fixture.model.phase == phase && fixture.model.canContinue)
        #expect(fixture.model.item?.cardId == 3 && fixture.surface.presented.isEmpty)
        #expect(!fixture.surface.calls.contains("render"))
        fixture.model.continueReview(); await fixture.model.settled()
        #expect(fixture.surface.presented == [3] && fixture.model.phase == .question)
        #expect(fixture.model.record.activeMilliseconds == 0)
        #expect(await fixture.model.end())
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["pause", "close", "generation"])
    func continueReadCannotEscapeLifecycleFence(action: String) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.emptyHead(.waitingUntil(instant: 1_800_000_000))
        await fixture.model.start()
        let held = CancellableGate()
        defer { held.release() }
        fixture.client.currentHead = Self.head(2)
        fixture.client.onHeadResult = { try await held.wait() }
        fixture.model.continueReview()
        await held.requested()
        var closing: Task<Bool, Never>?
        if action == "generation" { fixture.model.generationEnded() }
        else {
            fixture.model.beginPause()
            if action == "close" { closing = Task { await fixture.model.end() } }
        }
        held.release()
        await fixture.model.settled()
        #expect(fixture.surface.presented.isEmpty && !fixture.surface.calls.contains("render"))
        #expect(!fixture.model.canContinue && fixture.model.record.activeMilliseconds == 0)
        fixture.client.onHeadResult = nil
        if action == "pause" {
            try await fixture.model.pause()
            try await fixture.model.resume()
            #expect(fixture.model.phase == .waiting && fixture.model.canContinue)
            #expect(fixture.surface.presented.isEmpty, "Cancelled Continue is not resume authority")
            #expect(await fixture.model.end())
        } else if let closing { #expect(await closing.value) }
        else { #expect(fixture.model.phase == .ended && fixture.model.sessionID == nil) }
    }

    @Test(arguments: [false, true])
    func explicitUndoAndRedoFromTerminalStillRestoreQuestion(redo: Bool) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.emptyHead(.finished), marks: [], removed: [1]),
            leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.model.phase == .finished && !fixture.model.canContinue)
        fixture.model.restore(redo: redo); await fixture.model.settled()
        #expect(fixture.client.restores == [redo])
        #expect(fixture.model.phase == .question && fixture.model.item?.cardId == (redo ? 2 : 1))
        #expect(fixture.surface.presented == [1, redo ? 2 : 1])
        #expect(await fixture.model.end())
    }

    @Test func failedAcceptedAnswerFromTerminalStillReopensOriginalQuestion() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.emptyHead(.finished), marks: [], removed: [1]),
            leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.model.phase == .finished && fixture.model.record.answers.count == 1)
        fixture.client.currentHead = Self.head(1)
        fixture.model.commitFailed(cardID: 1, error: .internal("injected save failure"))
        await fixture.model.settled()
        #expect(fixture.model.phase == .question && fixture.surface.presented == [1, 1])
        #expect(fixture.model.record.answers.isEmpty && fixture.model.notice?.contains("injected save failure") == true)
        #expect(await fixture.model.end())
    }

    @Test(arguments: [false, true])
    func checkNowAndExternalEventKeepThePreviousCardHidden(waiting: Bool) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.now = 5
        fixture.client.answerHead = .accepted(headDiff: .init(
            head: Self.emptyHead(waiting ? .waitingUntil(instant: 1_800_000_600) : .finished), marks: [], removed: [1]),
            leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.good); await fixture.model.settled()
        let phase = fixture.model.phase
        let calls = fixture.surface.calls
        fixture.client.currentHead = Self.head(1)
        // Ignore this event: its session identifier differs from the session under review.
        fixture.model.receive(CoreEvent(.init(seq: 1, generation: 1, causeSeq: nil,
            kind: .currentCardChanged(sessionId: 99, cardId: 1, change: .eligible))))
        #expect(fixture.client.headReads == 0)
        fixture.model.receive(CoreEvent(.init(seq: 2, generation: 1, causeSeq: nil,
            kind: .currentCardChanged(sessionId: 5, cardId: 1, change: .eligible))))
        #expect(fixture.model.phase == phase)
        await fixture.model.settled()
        fixture.now = 100
        fixture.model.retry(); await fixture.model.settled() // Check Now is never Continue.
        #expect(fixture.model.phase == phase && fixture.model.canContinue)
        #expect(fixture.client.headReads == 2 && fixture.surface.calls == calls)
        #expect(fixture.surface.presented == [1] && fixture.model.record.activeMilliseconds == 5_000)
        fixture.model.continueReview(); await fixture.model.settled()
        #expect(fixture.model.phase == .question && fixture.surface.presented == [1, 1])
        #expect(fixture.surface.reveals == 1, "Continue must reset even the same card to its question")
        #expect(await fixture.model.end())
    }

    @Test func failedContinueCanRetryPresentationExplicitly() async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.emptyHead(.waitingUntil(instant: 1_800_000_000))
        await fixture.model.start()
        fixture.client.currentHead = Self.head(2)
        fixture.surface.onRender = { throw BridgeError.internal("injected render failure") }
        fixture.model.continueReview(); await fixture.model.settled()
        #expect(fixture.model.phase == .error && fixture.surface.presented.isEmpty)
        fixture.surface.onRender = nil
        fixture.model.retry(); await fixture.model.settled()
        #expect(fixture.model.phase == .question && fixture.surface.presented == [2])
        #expect(fixture.client.headReads == 2)
        #expect(await fixture.model.end())
    }

    @Test(.timeLimit(.minutes(1)))
    func failedAnswerDuringAcceptedEmptyReadRequiresAnotherFreshHead() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.emptyHead(.finished), marks: [], removed: [1]),
            leech: false, suspended: false, rolledOver: false)
        let held = CancellableGate()
        defer { held.release() }
        fixture.client.onAnswerResult = { fixture.model.requestRefresh() }
        fixture.client.onHeadResult = { try await held.wait() }
        fixture.model.rate(.good)
        await held.requested()
        #expect(fixture.model.record.answers.map(\.cardID) == [1])
        fixture.client.currentHead = Self.head(1)
        fixture.model.commitFailed(cardID: 1, error: .internal("injected save failure"))
        fixture.client.onHeadResult = nil
        held.release()
        await fixture.model.settled()
        #expect(fixture.client.headReads == 2 && fixture.client.answers == 1 && fixture.client.restores.isEmpty)
        #expect(fixture.model.record.answers.isEmpty && fixture.model.record.undone.isEmpty)
        #expect(fixture.model.phase == .question && fixture.model.item?.cardId == 1)
        #expect(fixture.surface.presented == [1, 1] && fixture.surface.reveals == 1)
        #expect(fixture.surface.calls == ["load", "render", "present", "render", "present"])
        #expect(await fixture.model.end())
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["undo", "redo", "failure"], ["result", "render", "present"])
    func terminalRestorationSurvivesPauseUntilQuestionIsPresented(action: String, stage: String) async throws {
        let fixture = await answeredTerminalFixture()
        let held = CancellableGate()
        defer { held.release() }
        var completed: [Int64] = []
        let target: Int64 = action == "redo" ? 2 : 1
        if stage == "result" {
            if action == "failure" { fixture.client.onHeadResult = { try await held.wait() } }
            else { fixture.client.onRestoreResult = { try await held.wait() } }
        } else if stage == "render" { fixture.surface.onRender = { try await held.wait() } }
        fixture.surface.onPresent = {
            if stage == "present" { try await held.wait() }
            completed.append(target)
        }
        if action == "failure" {
            fixture.client.currentHead = Self.head(target)
            fixture.model.commitFailed(cardID: 1, error: .internal("injected save failure"))
        } else { fixture.model.restore(redo: action == "redo") }
        await held.requested()
        fixture.model.beginPause()
        if stage == "result" { held.release() }
        try await fixture.model.pause()
        #expect(fixture.model.paused && completed.isEmpty)
        #expect(stage == "result" || held.cancelled)
        #expect(fixture.client.answers == 1)
        fixture.client.onHeadResult = nil; fixture.client.onRestoreResult = nil
        fixture.surface.onRender = nil
        fixture.surface.onPresent = { completed.append(target) }
        fixture.now = 100
        try await fixture.model.resume()
        await fixture.model.settled()
        #expect(completed == [target] && fixture.model.phase == .question && !fixture.model.paused)
        #expect(fixture.model.item?.cardId == target && fixture.surface.reveals == 1)
        #expect(fixture.client.restores == (action == "failure" ? [] : [action == "redo"]))
        #expect(fixture.client.headReads == (action == "failure" ? 2 : 1) && fixture.client.answers == 1)
        #expect(fixture.model.record.answers.count == (action == "redo" ? 1 : 0))
        #expect(fixture.model.record.undone.count == (action == "undo" ? 1 : 0))
        #expect(await fixture.model.end())
    }

    @Test(arguments: ["undo", "redo", "failure"])
    func emptyRestorationDoesNotAuthorizeUnrelatedActiveWork(action: String) async throws {
        let fixture = await answeredTerminalFixture()
        fixture.client.restoredHead = Self.emptyHead(.finished)
        if action == "failure" {
            fixture.client.currentHead = Self.emptyHead(.finished)
            fixture.model.commitFailed(cardID: 1, error: .internal("injected save failure"))
        } else { fixture.model.restore(redo: action == "redo") }
        await fixture.model.settled()
        #expect(fixture.model.phase == .finished && fixture.surface.presented == [1])
        try await fixture.model.pause()
        fixture.client.currentHead = Self.head(3)
        try await fixture.model.resume()
        await fixture.model.settled()
        #expect(fixture.model.phase == .finished && fixture.model.canContinue)
        #expect(fixture.surface.presented == [1] && fixture.client.answers == 1)
        fixture.model.requestRefresh(); await fixture.model.settled()
        #expect(fixture.model.phase == .finished && fixture.surface.presented == [1])
        fixture.model.continueReview(); await fixture.model.settled()
        #expect(fixture.model.phase == .question && fixture.surface.presented == [1, 3])
        #expect(await fixture.model.end())
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func newerFailureDuringRecoveryReadSurvivesOlderCompletion(resuming: Bool) async throws {
        let fixture = await answeredTerminalFixture()
        if resuming { try await fixture.model.pause() }
        let held = CancellableGate()
        defer { held.release() }
        fixture.client.currentHead = Self.emptyHead(.finished)
        fixture.client.onHeadResult = { try await held.wait() }
        fixture.model.commitFailed(cardID: 1, error: .internal("first save failure"))
        var resume: Task<Void, any Error>?
        if resuming { resume = Task { try await fixture.model.resume() } }
        await held.requested()
        fixture.client.currentHead = Self.head(1)
        fixture.model.commitFailed(cardID: 1, error: .internal("newer save failure"))
        fixture.client.onHeadResult = nil
        held.release()
        try await resume?.value
        await fixture.model.settled()
        #expect(fixture.model.phase == .question && fixture.model.item?.cardId == 1)
        #expect(fixture.model.record.answers.isEmpty && fixture.client.answers == 1)
        #expect(fixture.client.headReads == 2 && fixture.surface.presented == [1, 1])
        #expect(fixture.surface.reveals == 1 && fixture.client.restores.isEmpty)
        #expect(await fixture.model.end())
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["render", "present"])
    func cancelledContinuePreparationDoesNotAuthorizeResume(stage: String) async throws {
        let fixture = Fixture()
        fixture.client.currentHead = Self.emptyHead(.waitingUntil(instant: 1_800_000_000))
        await fixture.model.start()
        let held = CancellableGate()
        defer { held.release() }
        fixture.client.currentHead = Self.head(2)
        var completed = 0
        if stage == "render" { fixture.surface.onRender = { try await held.wait() } }
        fixture.surface.onPresent = {
            if stage == "present" { try await held.wait() }
            completed += 1
        }
        fixture.model.continueReview()
        await held.requested()
        try await fixture.model.pause()
        #expect(held.cancelled && completed == 0)
        fixture.surface.onRender = nil
        fixture.surface.onPresent = { completed += 1 }
        try await fixture.model.resume()
        #expect(fixture.model.phase == .waiting && fixture.model.canContinue && completed == 0)
        #expect(fixture.client.headReads == 2 && fixture.client.answers == 0)
        fixture.model.continueReview(); await fixture.model.settled()
        #expect(fixture.model.phase == .question && completed == 1)
        #expect(fixture.client.headReads == 3 && fixture.client.answers == 0)
        #expect(await fixture.model.end())
    }

    @Test(.timeLimit(.minutes(1)))
    func newerFailureDuringOlderPresentationStillGetsFreshRecoveryRead() async throws {
        let fixture = await answeredTerminalFixture()
        let held = CancellableGate()
        defer { held.release() }
        fixture.client.currentHead = Self.head(2)
        fixture.surface.onRender = { try await held.wait() }
        fixture.model.commitFailed(cardID: 1, error: .internal("first save failure"))
        await held.requested()
        fixture.client.currentHead = Self.head(1)
        fixture.model.commitFailed(cardID: 1, error: .internal("newer save failure"))
        fixture.surface.onRender = nil
        held.release()
        await fixture.model.settled()
        #expect(fixture.client.headReads == 2 && fixture.client.answers == 1 && fixture.client.restores.isEmpty)
        #expect(fixture.model.phase == .question && fixture.model.item?.cardId == 1)
        #expect(fixture.model.record.answers.isEmpty && fixture.surface.presented == [1, 2, 1])
        #expect(fixture.surface.reveals == 1)
        #expect(await fixture.model.end())
    }

    private func answeredTerminalFixture() async -> Fixture {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.emptyHead(.finished), marks: [], removed: [1]),
            leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.model.phase == .finished && fixture.model.record.answers.count == 1)
        return fixture
    }

}
