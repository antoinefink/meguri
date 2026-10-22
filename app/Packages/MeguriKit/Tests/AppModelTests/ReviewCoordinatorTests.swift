import CardRendering
import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor
@Suite struct ReviewCoordinatorTests {
    @Test func doubleInputAndStaleAnswerNeverReplay() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); fixture.model.reveal()
        await fixture.model.settled()
        #expect(fixture.surface.reveals == 1)
        fixture.client.answerHead = .stale(freshHead: Self.head(2), notice: true)
        fixture.model.rate(.good); fixture.model.rate(.easy)
        await fixture.model.settled()
        #expect(fixture.client.answers == 1)
        #expect(fixture.model.phase == .question)
        #expect(fixture.model.record.answers.isEmpty)
        #expect(fixture.surface.presented.last == 2)
        fixture.model.rate(.good)
        #expect(fixture.client.answers == 1)
        _ = await fixture.model.end()
    }

    @Test func commitFailureAfterAdvanceRestoresOriginalQuestionAndRecord() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.model.item?.cardId == 2)
        #expect(fixture.model.record.answers.map(\.cardID) == [1])
        fixture.client.currentHead = Self.head(1)
        fixture.model.commitFailed(cardID: 1, error: .internal("injected disk failure"))
        await fixture.model.settled()
        #expect(fixture.model.phase == .question)
        #expect(fixture.surface.presented.last == 1)
        #expect(fixture.model.record.answers.isEmpty)
        #expect(fixture.model.notice?.contains("injected disk failure") == true)
        _ = await fixture.model.end()
    }

    @Test func undoRedoUsesRestoreAndPersistsActualRecord() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.hard); await fixture.model.settled()
        fixture.model.restore(); fixture.model.restore()
        await fixture.model.settled()
        #expect(fixture.client.restores == [false])
        #expect(fixture.model.phase == .question)
        #expect(fixture.model.record.answers.isEmpty)
        #expect(fixture.model.record.undone.count == 1)
        fixture.model.restore(redo: true); await fixture.model.settled()
        #expect(fixture.client.restores == [false, true])
        #expect(fixture.model.record.answers.first?.rating == "hard")
        let persisted = try await fixture.store.load()
        #expect(persisted?.answers == fixture.model.record.answers)
        #expect(await fixture.model.end())
        #expect(await fixture.model.end())
        #expect(fixture.client.ends == 1)
    }

    @Test func pausePersistsBeforeReturningAndDisablesInputUntilRefresh() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.easy); await fixture.model.settled()
        try await fixture.model.pause()
        #expect(fixture.surface.calls.last == "suspend")
        #expect(try await fixture.store.load()?.answers.count == 1)
        fixture.model.reveal(); fixture.model.rate(.good)
        #expect(fixture.client.answers == 1)
        fixture.client.currentHead = Self.head(2)
        try await fixture.model.resume()
        #expect(fixture.surface.calls.contains("resume"))
        #expect(fixture.client.headReads > 0)
        #expect(!fixture.model.paused)
        _ = await fixture.model.end()
    }

    @Test func policyRefusalDoesNotLoadPagesOrCreateSession() async {
        let fixture = Fixture()
        fixture.client.refusal = .fsrsPolicyPending
        var refused = false
        fixture.model.onPolicyRefusal = { refused = true }
        await fixture.model.start()
        #expect(refused)
        #expect(fixture.model.sessionID == nil)
        #expect(fixture.surface.calls.isEmpty)
    }

    @Test func activeTimeExcludesBackgroundAndCapsElapsed() {
        var timer = ReviewActiveTime()
        timer.start(at: 10); timer.pause(at: 15)
        #expect(timer.milliseconds(at: 100, capSeconds: 60) == 5_000)
        timer.resume(at: 100)
        #expect(timer.milliseconds(at: 200, capSeconds: 60) == 60_000)
    }

    @Test func inactiveIntervalsAndResumeRefreshExcludeElapsedTime() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.now = 5
        fixture.model.beginPause()
        fixture.model.reveal()
        #expect(fixture.surface.reveals == 0)
        try await fixture.model.pause()
        #expect(fixture.surface.calls.last == "suspend")
        #expect(try await fixture.store.load()?.activeMilliseconds == 5_000)
        fixture.now = 105
        fixture.surface.onResume = {
            #expect(fixture.model.paused)
            fixture.model.reveal()
        }
        try await fixture.model.resume()
        #expect(fixture.surface.calls.suffix(2) == ["resume", "render"])
        #expect(fixture.surface.reveals == 0)
        #expect(!fixture.model.paused)
        fixture.now = 108
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.client.elapsed == [8_000])
        _ = await fixture.model.end()
    }

    @Test func shortcutAdmissionRequiresCardFocusAcrossNativeControlsAndComposition() {
        for paused in [false, true] {
            for editing in [false, true] {
                #expect(!ReviewInputOwnership.admits(cardFocused: false, editing: editing, paused: paused, repeated: false))
            }
        }
        #expect(ReviewInputOwnership.admits(cardFocused: true, editing: false, paused: false, repeated: false))
        #expect(!ReviewInputOwnership.admits(cardFocused: true, editing: true, paused: false, repeated: false))
        #expect(!ReviewInputOwnership.admits(cardFocused: true, editing: false, paused: true, repeated: false))
        #expect(!ReviewInputOwnership.admits(cardFocused: true, editing: false, paused: false, repeated: true))
    }

    private final class Fixture {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let client = Client()
        let surface = Surface()
        let store: ReviewRecordStore
        var model: ReviewCoordinator!
        var now: TimeInterval = 0
        init() {
            store = ReviewRecordStore(file: directory.appending(path: "session.json"))
            model = ReviewCoordinator(deckID: 1, surface: surface, store: store,
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client, uptime: { [weak self] in self?.now ?? 0 })
        }
        isolated deinit { try? FileManager.default.removeItem(at: directory) }
    }
    private final class Surface: ReviewCardSurface {
        var pair: CardPagePair? { nil }
        var onAction: ((CardBridgeAction) -> Void)?
        var onRecovery: (() -> Void)?
        var editing = false
        var reveals = 0
        var presented: [Int64] = []
        var calls: [String] = []
        var onResume: (() -> Void)?
        func load() async throws { calls.append("load") }
        func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws { calls.append("render") }
        func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws { presented.append(item.cardId); calls.append("present") }
        func preload(item: ReviewHeadItem) async throws {}
        func reveal(inputTime: TimeInterval) async throws { reveals += 1 }
        func suspend() async { calls.append("suspend") }
        func resume() async { calls.append("resume"); onResume?() }
        func close() { calls.append("close") }
    }
    private final class Client: ReviewSessionClient {
        var answers = 0
        var elapsed: [UInt64] = []
        var restores: [Bool] = []
        var ends = 0
        var headReads = 0
        var refusal: ReviewRefusal?
        var currentHead = ReviewCoordinatorTests.head(1)
        var answerHead = ReviewAnswerOutcome.accepted(headDiff: ReviewHeadDiff(head: ReviewCoordinatorTests.head(2), marks: [], removed: [1]), leech: false, suspended: false, rolledOver: false)
        func start(deckID: Int64) async throws -> ReviewStartResult {
            if let refusal { return .refused(reason: refusal) }
            return .started(sessionId: 5, generation: 1, head: currentHead, timeLim: 0)
        }
        func head(sessionID: UInt64) async throws -> ReviewHead { headReads += 1; return currentHead }
        func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome {
            answers += 1
            self.elapsed.append(elapsed)
            if case .accepted(let diff, _, _, _) = answerHead { currentHead = diff.head }
            return answerHead
        }
        func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus {
            .available(step: ReviewStepInfo(id: 1, name: "Answer Card", kind: .answer, session: sessionID, longWrite: nil))
        }
        func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead { restores.append(redo); currentHead = ReviewCoordinatorTests.head(redo ? 2 : 1); return currentHead }
        func studyState(deckID: Int64) async throws -> ReviewDeckStudyState { throw BridgeError.internal("No empty head scripted") }
        func end(sessionID: UInt64) async throws { ends += 1 }
    }
    private static func head(_ id: Int64) -> ReviewHead {
        let state = ReviewSchedulingState.new(position: 1)
        let answer = ReviewAnswerState(state: state, delay: .days(amount: 1), left: 0, factor: 0,
            revlog: ReviewRevlogPreview(interval: 1, lastInterval: 0, kind: 0, factor: 0),
            label: ReviewAnswerLabel(lessThan: false, amount: "1", unit: .days))
        let item = ReviewHeadItem(cardId: id, noteId: id, kind: .new,
            stateToken: ReviewStateToken(bytes: Array(repeating: UInt8(id), count: 32)),
            renderKey: ReviewRenderKey(bytes: Array(repeating: UInt8(id), count: 32)),
            nextStates: ReviewSchedulingStates(current: state, again: answer, hard: answer, good: answer, easy: answer),
            predictedNext: ReviewPredictions(again: 2, hard: 2, good: 2, easy: 2), flag: 0, marked: false,
            homePreset: ReviewHomePreset(id: 1, autoplay: false, skipQuestionWhenReplayingAnswer: false, showTimer: false,
                stopTimerOnAnswer: false, maximumAnswerSeconds: 60, questionSeconds: 0, answerSeconds: 0,
                questionAction: 0, answerAction: 0, waitForAudio: false, leechAction: 0))
        return ReviewHead(items: [item], counts: ReviewCounts(new: 1, learning: 0, review: 0), validUntil: Int64.max)
    }
}
