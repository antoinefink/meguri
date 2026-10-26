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

    @Test func acceptedFinishedHeadWinsOverOlderLearningSummaryAndWaitingHint() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.client.summary = Self.summary(waiting: true)
        fixture.client.answerHead = .accepted(headDiff: .init(head: Self.emptyHead(.finished), marks: [], removed: [1]),
                                               leech: false, suspended: false, rolledOver: false)
        fixture.model.rate(.easy); await fixture.model.settled()
        #expect(fixture.model.phase == .finished)
        #expect(fixture.model.waitingUntil == nil)
        await fixture.model.summaryTask?.value
        #expect(fixture.model.studyState?.learningRemainingToday == 1)
        #expect(fixture.model.record.answers.count == 1)
        fixture.model.terminalChanged(.waitingUntil(instant: 1_800_000_600))
        await fixture.model.settled()
        #expect(fixture.model.phase == .finished)
        #expect(fixture.model.waitingUntil == nil)
        #expect(fixture.client.headReads == 1)
        fixture.model.terminalChanged(.finished)
        await fixture.model.settled()
        #expect(fixture.client.headReads == 1, "Matching terminal hints do not poll recursively")
        _ = await fixture.model.end()
    }

    @Test func waitingHeadWinsOverFinishedSummaryAndFixedClockPollKeepsItsInstant() async throws {
        let fixture = Fixture()
        let instant: Int64 = 1_800_000_600
        fixture.client.summary = Self.summary(waiting: false)
        fixture.client.currentHead = Self.emptyHead(.waitingUntil(instant: instant))
        await fixture.model.start()
        #expect(fixture.model.phase == .waiting)
        await fixture.model.summaryTask?.value
        #expect(fixture.model.studyState?.finished == true)
        #expect(fixture.model.waitingUntil == Date(timeIntervalSince1970: Double(instant)))
        fixture.model.terminalChanged(.finished)
        await fixture.model.settled()
        fixture.model.requestRefresh(); await fixture.model.settled()
        #expect(fixture.model.phase == .waiting)
        #expect(fixture.model.waitingUntil == Date(timeIntervalSince1970: Double(instant)))
        #expect(fixture.client.headReads == 2)
        fixture.client.currentHead = Self.head(2)
        fixture.model.requestRefresh(); await fixture.model.settled()
        fixture.model.terminalChanged(.waitingUntil(instant: instant))
        await fixture.model.settled()
        #expect(fixture.model.phase == .question)
        #expect(fixture.model.item?.cardId == 2)
        #expect(fixture.model.waitingUntil == nil)
        #expect(fixture.model.studyState == nil)
        _ = await fixture.model.end()
    }

    @Test func waitingHeadSurvivesSummaryThrow() async {
        let fixture = Fixture()
        let instant: Int64 = 1_800_000_600
        fixture.client.currentHead = Self.emptyHead(.waitingUntil(instant: instant))
        await fixture.model.start()
        await fixture.model.summaryTask?.value
        #expect(fixture.model.phase == .waiting)
        #expect(fixture.model.waitingUntil == Date(timeIntervalSince1970: Double(instant)))
        #expect(fixture.model.studyState == nil)
        #expect(fixture.model.notice?.contains("No empty head scripted") == true)
        #expect(fixture.model.canFinish)
        #expect(fixture.model.canUndo)
        #expect(await fixture.model.end())
    }

    @Test func finishedHeadSurvivesSummaryThrow() async {
        let fixture = Fixture()
        fixture.client.currentHead = Self.emptyHead(.finished)
        await fixture.model.start()
        await fixture.model.summaryTask?.value
        #expect(fixture.model.phase == .finished)
        #expect(fixture.model.waitingUntil == nil)
        #expect(fixture.model.studyState == nil)
        #expect(fixture.model.notice?.contains("No empty head scripted") == true)
        #expect(fixture.model.canFinish)
        #expect(fixture.model.canUndo)
        #expect(await fixture.model.end())
    }

    @Test func suspendedSummaryLeavesTerminalControlsUsableAndCannotOverwriteNewerState() async {
        let instant: Int64 = 1_800_000_600
        for terminal in [ReviewTerminalState.waitingUntil(instant: instant), .finished] {
            for next in ["active", "terminal", "end", "generation"] {
                let fixture = Fixture()
                let held = SummaryGate(state: Self.summary(waiting: true))
                defer { held.release() }
                fixture.client.summaryGate = held
                fixture.client.currentHead = Self.emptyHead(terminal)
                let start = Task { await fixture.model.start() }
                await held.requested()
                let summaryTask = fixture.model.summaryTask
                let expectedPhase: ReviewCoordinator.Phase = terminal == .finished ? .finished : .waiting
                let applied = fixture.model.phase == expectedPhase
                #expect(applied)
                #expect(fixture.model.waitingUntil == (terminal == .finished ? nil : Date(timeIntervalSince1970: Double(instant))))
                // Release on failure too: the pre-fix start operation waits before applying phase.
                if !applied { held.release() }
                await start.value
                #expect(fixture.model.canFinish, "A suspended summary must not retain operation admission")
                #expect(fixture.model.canUndo)
                #expect(fixture.model.canRedo)
                fixture.client.summaryGate = nil
                switch next {
                case "active":
                    fixture.client.currentHead = Self.head(2)
                    fixture.model.requestRefresh(); await fixture.model.settled()
                case "terminal":
                    // Same marker/session still needs cancellation of the older summary owner.
                    fixture.client.summary = Self.summary(waiting: false)
                    fixture.model.requestRefresh(); await fixture.model.settled()
                    await fixture.model.summaryTask?.value
                case "end":
                    #expect(await fixture.model.end())
                default:
                    fixture.model.generationEnded()
                }
                held.release()
                await summaryTask?.value
                switch next {
                case "active":
                    #expect(fixture.model.phase == .question)
                    #expect(fixture.model.item?.cardId == 2)
                    #expect(fixture.model.studyState == nil)
                    #expect(fixture.model.waitingUntil == nil)
                case "terminal":
                    #expect(fixture.model.phase == (terminal == .finished ? .finished : .waiting))
                    #expect(fixture.model.studyState?.finished == true)
                default:
                    #expect(fixture.model.phase == .ended)
                    #expect(fixture.model.sessionID == nil)
                    #expect(fixture.model.studyState == nil)
                }
                #expect(fixture.model.notice == nil)
                if next == "active" || next == "terminal" { #expect(await fixture.model.end()) }
            }
        }
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

    @Test func nativeInputChangesOnlyAnswerElapsedAndKeepsRealHeadTokenAndUptime() async throws {
        let input = try NativeReviewInputFixture()
        defer { input.remove() }
        let owner = input.owner()
        try await input.activate(owner, bindSession: false)
        let fixture = Fixture(nativeInput: owner)
        fixture.now = 10
        await fixture.model.start()
        let realItem = try #require(fixture.model.item)
        fixture.now = 17
        fixture.model.reveal(); await fixture.model.settled()
        fixture.now = 19
        fixture.model.rate(.easy); fixture.model.rate(.easy)
        await fixture.model.settled()
        #expect(fixture.client.answers == 1 && fixture.client.elapsed == [1000])
        let call = try #require(fixture.client.used.first)
        #expect(call.session == 5 && call.item == realItem && call.item.stateToken == realItem.stateToken)
        #expect(call.rating == .easy && call.wall == FrozenNativeReviewInput.now)
        #expect(fixture.model.record.answers.first?.elapsedMilliseconds == 1000)
        #expect(fixture.model.item?.cardId == 2, "The ordinary client result supplies the next head")
        await owner.flushEvidence()
        let receipt = try input.receipt(owner)
        #expect(receipt.admission?.token == realItem.stateToken.bytes && receipt.completion == "accepted")
        fixture.now = 23
        try await fixture.model.pause()
        #expect(fixture.model.record.activeMilliseconds == 13_000, "The hook never changes uptime/active duration")
        #expect(await fixture.model.end())
        await owner.flushEvidence()
        #expect(try input.receipt(owner).ended)
    }
    @Test(arguments: ["rating", "card", "owner", "eligibility", "failed", "stale"])
    func nativeInputRefusesMismatchBeforeCallAndKeepsFailedCompletionSeparate(kind: String) async throws {
        let input = try NativeReviewInputFixture()
        defer { input.remove() }
        let owner = input.owner()
        try await input.activate(owner, bindSession: false)
        let fixture = Fixture(nativeInput: owner)
        if kind == "card" { fixture.client.currentHead = Self.head(2) }
        await fixture.model.start(); fixture.model.reveal(); await fixture.model.settled()
        if kind == "eligibility" { fixture.model.nativeInputEligibility = { false } }
        if kind == "owner" { _ = input.owner() }
        if kind == "stale" { fixture.client.answerHead = .stale(freshHead: Self.head(1), notice: false) }
        if kind == "failed" { fixture.client.answerFailure = true }
        fixture.model.rate(kind == "rating" ? .good : .easy)
        await fixture.model.settled(); await owner.flushEvidence()
        if kind == "failed" || kind == "stale" {
            #expect(fixture.client.answers == 1 && fixture.client.elapsed == [1000])
            let receipt = try input.receipt(owner)
            #expect(receipt.admission != nil && receipt.completion == kind)
            #expect(fixture.model.record.answers.isEmpty)
            fixture.model.reveal(); await fixture.model.settled(); fixture.model.rate(.easy); await fixture.model.settled()
            #expect(fixture.client.answers == 1)
        } else { #expect(fixture.client.answers == 0 && fixture.model.phase == .error) }
        _ = await fixture.model.end(); await owner.flushEvidence()
    }

    final class Fixture {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let client = Client()
        let surface = Surface()
        let store: ReviewRecordStore
        var model: ReviewCoordinator!
        var now: TimeInterval = 0
        init(nativeInput: NativeReviewInput? = nil) {
            store = ReviewRecordStore(file: directory.appending(path: "session.json"))
            model = ReviewCoordinator(deckID: 1, surface: surface, store: store,
                clock: .fixed(Date(timeIntervalSince1970: nativeInput == nil ? 1_800_000_000 : 1_894_698_000)), client: client,
                uptime: { [weak self] in self?.now ?? 0 }, nativeInput: nativeInput, nativeGeneration: 1)
            model.retryMedia = { [surface] names in surface.arrivals.append(names); return true }
        }
        isolated deinit { try? FileManager.default.removeItem(at: directory) }
    }
    final class Surface: ReviewCardSurface {
        var pair: CardPagePair? { nil }
        var onAction: ((CardBridgeAction) -> Void)?
        var onRecovery: (() -> Void)?
        var editing = false
        var reveals = 0
        var presented: [Int64] = []
        var calls: [String] = []
        var onLoad: (() async throws -> Void)?
        var onResume: (() -> Void)?
        var arrivals: [[String]] = []
        func load() async throws { calls.append("load"); try await onLoad?() }
        func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws { calls.append("render") }
        func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws { presented.append(item.cardId); calls.append("present") }
        func preload(item: ReviewHeadItem) async throws {}
        func reveal(inputTime: TimeInterval) async throws { reveals += 1 }
        func suspend() async { calls.append("suspend") }
        func resume() async { calls.append("resume"); onResume?() }
        func close() { calls.append("close") }
    }
    final class SummaryGate {
        let state: ReviewDeckStudyState
        private var continuation: CheckedContinuation<ReviewDeckStudyState, Never>?
        private var request: CheckedContinuation<Void, Never>?
        private var didRequest = false
        init(state: ReviewDeckStudyState) { self.state = state }
        func value() async -> ReviewDeckStudyState {
            await withCheckedContinuation {
                continuation = $0; didRequest = true
                request?.resume(); request = nil
            }
        }
        func requested() async {
            if didRequest { return }
            await withCheckedContinuation { request = $0 }
        }
        func release() {
            continuation?.resume(returning: state); continuation = nil
        }
    }
    final class Client: ReviewSessionClient {
        var answers = 0
        var answerFailure = false
        var elapsed: [UInt64] = []
        var used: [(session: UInt64, item: ReviewHeadItem, rating: ReviewRating, wall: Int64)] = []
        var restores: [Bool] = []
        var ends = 0
        var headReads = 0
        var refusal: ReviewRefusal?
        var summary: ReviewDeckStudyState?
        var summaryGate: SummaryGate?
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
            used.append((sessionID, item, rating, at))
            if answerFailure { throw BridgeError.internal("native input completion failure") }
            if case .accepted(let diff, _, _, _) = answerHead { currentHead = diff.head }
            return answerHead
        }
        func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus {
            .available(step: ReviewStepInfo(id: 1, name: "Answer Card", kind: .answer, session: sessionID, longWrite: nil))
        }
        func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead { restores.append(redo); currentHead = ReviewCoordinatorTests.head(redo ? 2 : 1); return currentHead }
        func studyState(deckID: Int64) async throws -> ReviewDeckStudyState {
            if let summaryGate { return await summaryGate.value() }
            guard let summary else { throw BridgeError.internal("No empty head scripted") }
            return summary
        }
        func end(sessionID: UInt64) async throws { ends += 1 }
    }
    private static func emptyHead(_ terminal: ReviewTerminalState) -> ReviewHead {
        ReviewHead(items: [], counts: .init(new: 0, learning: 0, review: 0), validUntil: Int64.max, terminalState: terminal)
    }
    private static func summary(waiting: Bool) -> ReviewDeckStudyState {
        let counts = ReviewCounts(new: 0, learning: waiting ? 1 : 0, review: 0)
        return ReviewDeckStudyState(deck: 1, queueCounts: counts, listCounts: counts,
            difference: .init(new: 0, learning: 0, review: 0), finished: !waiting,
            learningRemainingToday: waiting ? 1 : 0, learningLaterToday: 0,
            nextLearningDue: waiting ? 1_800_000_600 : nil, nextLearningReadyAt: waiting ? 1_800_000_600 : nil,
            reviewLimitReached: false, newLimitReached: false, learningRemaining: waiting ? 1 : 0,
            secsUntilNextLearn: waiting ? 600 : 0, reviewRemaining: false, newRemaining: false,
            siblingBuried: 0, userBuried: 0, tomorrow: .init(new: 0, learning: 0, review: 0), empty: !waiting)
    }
    static func head(_ id: Int64) -> ReviewHead {
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
        return ReviewHead(items: [item], counts: ReviewCounts(new: 1, learning: 0, review: 0), validUntil: Int64.max, terminalState: .active)
    }
}
