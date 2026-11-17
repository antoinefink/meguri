import CardRendering
@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel
#if DEBUG
import Observation
import Synchronization
#endif

extension ReviewCollectionStartupTests {
    @Test func nativeRenderedCardPayloadPreservesMediaReferences() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: directory.appending(path: "synthetic-installation"))
        owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        do {
            await owner.start()
            let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
            owner.entry.present(); owner.entry.inspect(source); await owner.entry.waitForCompletion()
            try #require(owner.entry.preview != nil)
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            func flatten(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] { nodes.flatMap { [$0] + flatten($0.children) } }
            let deck = try #require(flatten(owner.decks).first { !$0.filtered && $0.counts.new > 0 })
            await owner.startStudy(deckID: deck.id)
            let item = try #require(owner.review?.item)
            await owner.finishReview()
            let notes = CoreNoteService()
            let note = try await notes.readNote(id: item.noteId)
            let fields = note.fields.map { $0 + "<img src=\"question.png\"><img src=\"answer.png\">[sound:spoken.mp3]" }
            _ = try await notes.updateNote(.init(id: note.id, fields: fields, tags: note.tags))
            #if os(macOS)
            let platform: CardRenderPlatform = .mac
            #else
            let platform: CardRenderPlatform = .ios
            #endif
            let rendered = try await Core.renderCard(id: item.cardId, platform: platform)
            #expect(Set(rendered.mediaRefs).isSuperset(of: ["question.png", "answer.png", "spoken.mp3"]))
            let payload = CardPayload(rendered, audioPolicy: .init(autoplay: false))
            #expect(payload.mediaRefs == rendered.mediaRefs)
            #expect(payload.question == rendered.question && payload.answer == rendered.answer)
            #expect(CardPayload(cardID: 1, question: "Offline", answer: "Answer").mediaRefs.isEmpty)
            #expect(await owner.close())
        } catch { _ = await owner.close(); throw error }
    }
}

@MainActor
@Suite(.serialized) struct ReviewCoordinatorTests {
    #if DEBUG
    @Test(arguments: [false, true])
    func startDiagnostic(cardLoad: Bool) async {
        let fixture = Fixture()
        let held = SummaryGate(state: Self.summary(waiting: false))
        defer { held.release() }
        if cardLoad {
            fixture.surface.onLoad = { _ = await held.value() }
        } else {
            fixture.client.onStartResult = { _ = await held.value() }
        }
        let start = Task { await fixture.model.start() }
        await held.requested()
        #expect(fixture.model.phase == .loading)
        #expect((fixture.model.sessionID != nil) == cardLoad)
        #expect(fixture.model.debugSessionStatus == (cardLoad ? "cardLoad" : "coreStart"))
        #expect(fixture.surface.presented.isEmpty)
        held.release()
        await start.value
        #expect(fixture.model.phase == .question)
        #expect(fixture.model.debugSessionStatus == "question")
        #expect(fixture.surface.presented == [1])
        #expect(fixture.client.answers == 0)
        #expect(await fixture.model.end())
    }

    @Test func startObservationBelongsToItsInvocationAndStopsAtCardLoad() async throws {
        let first = Fixture(), second = Fixture()
        let firstStart = SummaryGate(state: Self.summary(waiting: false))
        let secondStart = SummaryGate(state: Self.summary(waiting: false))
        let cardLoad = SummaryGate(state: Self.summary(waiting: false))
        defer { firstStart.release(); secondStart.release(); cardLoad.release() }
        var firstObservation: ReviewStartObservation?
        var secondObservation: ReviewStartObservation?
        first.client.onStartObservation = { firstObservation = $0; $0.enter(.jobWait) }
        second.client.onStartObservation = { secondObservation = $0; $0.enter(.startExport) }
        first.client.onStartResult = { _ = await firstStart.value() }
        second.client.onStartResult = { _ = await secondStart.value() }
        first.surface.onLoad = { _ = await cardLoad.value() }
        let firstTask = Task { await first.model.start() }
        let secondTask = Task { await second.model.start() }
        await firstStart.requested(); await secondStart.requested()
        let firstOwned = try #require(firstObservation)
        let secondOwned = try #require(secondObservation)
        #expect(firstOwned !== secondOwned)
        #expect(first.model.debugSessionStatus == "jobWait")
        #expect(second.model.debugSessionStatus == "startExport")
        #expect(first.model.sessionID == nil && second.model.sessionID == nil)
        let notified = Mutex(false)
        withObservationTracking {
            _ = first.model.debugSessionStatus
        } onChange: {
            _ = firstOwned.stage
            notified.withLock { $0 = true }
        }
        firstOwned.enter(.resultQueued)
        #expect(notified.withLock { $0 }, "the existing status must observe nested lane updates")
        #expect(first.model.debugSessionStatus == "resultQueued")
        #expect(second.model.debugSessionStatus == "startExport")

        firstStart.release()
        await cardLoad.requested()
        #expect(first.model.phase == .loading && first.model.sessionID != nil)
        #expect(first.model.debugSessionStatus == "cardLoad")
        firstOwned.enter(.resultReturned)
        #expect(first.model.debugSessionStatus == "cardLoad")
        #expect(second.model.debugSessionStatus == "startExport")
        cardLoad.release(); secondStart.release()
        await firstTask.value; await secondTask.value
        #expect(first.model.debugSessionStatus == "question")
        #expect(second.model.debugSessionStatus == "question")
        secondOwned.enter(.resultRead)
        #expect(first.model.debugSessionStatus == "question")
        #expect(second.model.debugSessionStatus == "question")
        #expect(first.surface.presented == [1] && second.surface.presented == [1])
        #expect(first.client.answers == 0 && second.client.answers == 0)
        #expect(await first.model.end())
        #expect(await second.model.end())
    }
    #endif

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

    @Test func defaultNoopRecordingKeepsPackageReviewIndependentOfInjectedRecording() async throws {
        var observations: [FieldDiagnosticObservation] = []
        let observed = Fixture(recordDiagnostic: { observations.append($0) })
        let library = Fixture()
        for fixture in [observed, library] {
            await fixture.model.start()
            fixture.model.reveal(); await fixture.model.settled()
            fixture.model.rate(.good); await fixture.model.settled()
        }
        #expect(observations.contains { $0.stage == .coreAccepted && $0.operation == .answer })
        let recorded = observations
        #expect(library.model.record.answers.map(\.cardID) == observed.model.record.answers.map(\.cardID))
        #expect(library.surface.presented == observed.surface.presented)
        library.model.reveal(); await library.model.settled()
        library.model.rate(.good); await library.model.settled()
        #expect(library.client.answers == 2)
        #expect(observations == recorded)
        #expect(await library.model.end())
        #expect(observations == recorded)
        #expect(await observed.model.end())
    }

    @Test func acceptedAnswerDiagnosticsSurviveSessionRecordSaveFailure() async throws {
        var observations: [FieldDiagnosticObservation] = []
        let fixture = Fixture(recordDiagnostic: { observations.append($0) })
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        // Fail only the presentation-history store after Core start succeeds.
        try FileManager.default.removeItem(at: fixture.directory)
        try Data([0]).write(to: fixture.directory)
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.client.answers == 1)
        #expect(fixture.model.record.answers.map(\.cardID) == [1])
        #expect(observations.filter { $0.operation == .answer && $0.stage == .coreAccepted }.map(\.outcome) == [.succeeded])
        #expect(observations.contains { $0.operation == .save && $0.stage == .persistence && $0.outcome == .failed && $0.failure == .storageUnavailable })
        #expect(observations.contains { $0.operation == .answer && $0.stage == .presentation && $0.outcome == .succeeded })
        try FileManager.default.removeItem(at: fixture.directory)
        #expect(await fixture.model.end())
    }

    @Test(arguments: ["head", "presentation"])
    func acceptedAnswerDiagnosticsRetainAcceptanceWhenFollowupFails(boundary: String) async throws {
        var observations: [FieldDiagnosticObservation] = []
        let fixture = Fixture(recordDiagnostic: { observations.append($0) })
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        if boundary == "head" {
            fixture.client.onAnswerResult = { fixture.model.requestRefresh() }
            fixture.client.onHeadResult = { throw BridgeError.internal("synthetic head refresh failure") }
        } else {
            fixture.surface.onPresent = { throw BridgeError.internal("synthetic presentation failure") }
        }
        fixture.model.rate(.good); await fixture.model.settled()
        #expect(fixture.client.answers == 1)
        #expect(fixture.model.record.answers.map(\.cardID) == [1])
        #expect(try await fixture.store.load()?.answers == fixture.model.record.answers)
        #expect(observations.filter { $0.operation == .answer && $0.stage == .coreAccepted }.map(\.outcome) == [.succeeded])
        #expect(observations.filter { $0.operation == .answer && $0.stage == .presentation }.map(\.outcome) == [.failed])
        #expect(observations.contains { $0.operation == .save && $0.stage == .persistence && $0.outcome == .succeeded })
        #expect(fixture.model.phase == .error)
        if boundary == "head" { #expect(fixture.client.headReads >= 2) }
        fixture.client.onHeadResult = nil; fixture.client.onAnswerResult = nil; fixture.surface.onPresent = nil
        #expect(await fixture.model.end())
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

    @Test(.timeLimit(.minutes(1)), arguments: ["pause-resume", "refresh-close", "media-error-close"])
    func startupOwnershipSurvivesPhaseChanges(action: String) async throws {
        let fixture = Fixture()
        let render = CancellableGate()
        defer { render.release() }
        fixture.surface.onRender = { try await render.wait() }
        let starting = Task { await fixture.model.start() }
        defer { starting.cancel() }
        await render.requested()
        #expect(fixture.model.phase == .loading && fixture.model.sessionID == 5)
        if action == "refresh-close" {
            fixture.model.requestRefresh()
            #expect(fixture.model.phase == .transition)
        } else if action == "media-error-close" {
            fixture.model.mediaUnavailable()
            #expect(fixture.model.phase == .error)
        }
        if action == "pause-resume" {
            try await fixture.model.pause()
            await starting.value
            #expect(render.cancelled && fixture.model.paused && fixture.client.ends == 0)
            #expect(fixture.surface.presented.isEmpty)
            #expect(try await fixture.store.load()?.endedAt == nil)
            fixture.surface.onRender = nil
            fixture.client.currentHead = Self.head(2)
            try await fixture.model.resume()
            #expect(fixture.client.headReads == 1 && fixture.surface.presented == [2])
            #expect(fixture.model.phase == .question && !fixture.model.paused)
        }
        #expect(await fixture.model.end())
        await starting.value
        #expect(render.cancelled && fixture.client.ends == 1)
        #expect(fixture.surface.calls.last == "close" && fixture.model.record.answers.isEmpty)
        #expect(try await fixture.store.load()?.endedAt != nil)
    }

    @Test(.timeLimit(.minutes(1))) func closeDrainsAcceptedAnswerAndPhysicalStopBeforeCoreEnd() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        let answer = CancellableGate(), physicalStop = CancellableGate()
        defer { answer.release(); physicalStop.release() }
        fixture.client.onAnswerResult = { try await answer.wait() }
        fixture.surface.onSuspend = { _ = try? await physicalStop.wait() }
        fixture.model.rate(.good)
        await answer.requested()
        fixture.model.beginPause()
        let closing = Task { await fixture.model.end() }
        defer { closing.cancel() }
        #expect(fixture.client.ends == 0 && !fixture.surface.calls.contains("suspend"))
        answer.release()
        await physicalStop.requested()
        #expect(!answer.cancelled && fixture.model.record.answers.map(\.cardID) == [1])
        #expect(fixture.client.ends == 0 && fixture.model.record.endedAt == nil)
        #expect(try await fixture.store.load()?.answers.map(\.cardID) == [1])
        #expect(!fixture.surface.calls.contains("close"))
        physicalStop.release()
        #expect(await closing.value)
        #expect(fixture.client.ends == 1 && fixture.surface.calls.last == "close")
        let saved = try #require(try await fixture.store.load())
        #expect(saved.answers.map(\.cardID) == [1] && saved.endedAt != nil)
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func secondPauseCancelsAndJoinsResumedStartup(close: Bool) async throws {
        let fixture = Fixture()
        let first = CancellableGate(), resumed = CancellableGate(), stopped = CancellableGate()
        var starting: Task<Void, Never>?
        var resuming: Task<Void, any Error>?
        var pausing: Task<Bool, any Error>?
        let trace = LifecycleTrace("secondPauseCancelsAndJoinsResumedStartup(close: \(close))", fixture: fixture,
                                   gates: [("first", first), ("resumed", resumed), ("stopped", stopped)])
        try await Self.lifecycleDeadline(trace, unblocking: {
            first.release(); resumed.release(); stopped.release()
        }, cleanup: {
            await starting?.value
            _ = await resuming?.result
            _ = await pausing?.result
            _ = await fixture.model.end()
        }) {
            fixture.surface.onRender = { trace.checkpoint = "first.wait"; try await first.wait() }
            starting = Task { trace.checkpoint = "starting.start"; await fixture.model.start() }
            trace.checkpoint = "first.requested"
            await first.requested()
            trace.checkpoint = "model.pause"
            try await fixture.model.pause()
            trace.checkpoint = "starting?.value"
            await starting?.value
            fixture.surface.onRender = { trace.checkpoint = "resumed.wait"; try await resumed.wait() }
            resuming = Task { trace.checkpoint = "resuming.resume"; try await fixture.model.resume() }
            trace.checkpoint = "resumed.requested"
            await resumed.requested()
            fixture.surface.onSuspend = { trace.checkpoint = "stopped.wait"; _ = try? await stopped.wait() }
            if !close { fixture.model.beginPause() }
            pausing = Task {
                if close { trace.checkpoint = "pausing.end"; return await fixture.model.end() }
                trace.checkpoint = "model.pause"
                try await fixture.model.pause()
                return true
            }
            trace.checkpoint = "stopped.requested"
            await stopped.requested()
            trace.checkpoint = "resuming?.value"
            try await resuming?.value
            #expect(first.cancelled && resumed.cancelled && fixture.model.paused)
            #expect(fixture.surface.presented.isEmpty && fixture.client.ends == 0)
            stopped.release()
            trace.checkpoint = "pausing?.value"
            #expect(try await pausing?.value == true)
            trace.checkpoint = "endedAt.store.load"
            #expect((try await fixture.store.load()?.endedAt != nil) == close)
            fixture.surface.onSuspend = nil; fixture.surface.onRender = nil
            if !close {
                fixture.client.currentHead = Self.head(2)
                trace.checkpoint = "model.resume"
                try await fixture.model.resume()
                #expect(fixture.model.canReveal && fixture.surface.presented == [2])
            }
            trace.checkpoint = "model.end"
            #expect(await fixture.model.end())
            #expect(fixture.client.ends == 1 && fixture.model.record.answers.isEmpty)
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["render", "present"])
    func callerCancellationKeepsResumePausedUntilNewHeadIsPresented(stage: String) async throws {
        let fixture = Fixture(), held = CancellableGate()
        var resuming: Task<Void, any Error>?
        var completedPresentations: [Int64] = []
        let trace = LifecycleTrace("callerCancellationKeepsResumePausedUntilNewHeadIsPresented(stage: \(stage))", fixture: fixture,
                                   gates: [("held", held)])
        try await Self.lifecycleDeadline(trace, unblocking: { held.release() }, cleanup: {
            _ = await resuming?.result
            _ = await fixture.model.end()
            fixture.surface.onRender = nil; fixture.surface.onPresent = nil
        }) {
            trace.checkpoint = "model.start"
            await fixture.model.start()
            fixture.model.reveal(); trace.checkpoint = "reveal.settled"; await fixture.model.settled()
            try #require(fixture.model.canRate && fixture.surface.presented == [1])
            trace.checkpoint = "model.pause"
            try await fixture.model.pause()
            fixture.client.currentHead = Self.head(2)
            let reads = fixture.client.headReads
            if stage == "render" { fixture.surface.onRender = { trace.checkpoint = "held.wait"; try await held.wait() } }
            fixture.surface.onPresent = {
                if stage == "present" { trace.checkpoint = "held.wait"; try await held.wait() }
                completedPresentations.append(2)
            }
            let caller = Task { trace.checkpoint = "resuming.resume"; try await fixture.model.resume() }
            resuming = caller
            trace.checkpoint = "held.requested"
            await held.requested()
            #expect(fixture.model.paused && !fixture.model.canReveal && !fixture.model.canRate)
            #expect(completedPresentations.isEmpty)
            // Cancel only this caller; no second lifecycle pause changes the revision.
            caller.cancel()
            trace.checkpoint = "caller.result"
            let result = await caller.result
            switch result {
            case .success: Issue.record("Caller cancellation incorrectly completed Resume")
            case .failure(let error): #expect(error is CancellationError)
            }
            #expect(held.cancelled && fixture.model.paused)
            #expect(!fixture.model.canReveal && !fixture.model.canRate)
            #expect(!fixture.model.canUndo && !fixture.model.canRedo)
            #expect(completedPresentations.isEmpty)
            if stage == "render" { #expect(fixture.surface.presented == [1]) }
            fixture.model.rate(.good); trace.checkpoint = "rate.settled"; await fixture.model.settled()
            #expect(fixture.client.answers == 0 && fixture.model.record.answers.isEmpty)
            #expect(fixture.client.ends == 0 && fixture.surface.calls.filter { $0 == "suspend" }.count == 1)
            fixture.surface.onRender = nil
            fixture.surface.onPresent = { completedPresentations.append(2) }
            trace.checkpoint = "model.resume"
            try await fixture.model.resume()
            #expect(!fixture.model.paused && fixture.model.phase == .question)
            #expect(fixture.model.canReveal && !fixture.model.canRate && fixture.model.item?.cardId == 2)
            #expect(completedPresentations == [2] && fixture.surface.presented.last == 2)
            #expect(fixture.client.starts == 1 && fixture.client.headReads == reads + 2 && fixture.client.answers == 0)
            trace.checkpoint = "store.load"
            #expect(try await fixture.store.load()?.answers.isEmpty == true)
            trace.checkpoint = "model.end"
            #expect(await fixture.model.end())
            #expect(fixture.client.ends == 1 && fixture.surface.calls.last == "close")
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: [true, false])
    func resumedStartupLoadPreservesCancellationOwnership(cancelCaller: Bool) async throws {
        let fixture = Fixture(), initial = CancellableGate(), resumed = CancellableGate()
        var starting: Task<Void, Never>?
        var resuming: Task<Void, any Error>?
        var loadEntries = 0, completedLoads = 0
        let trace = LifecycleTrace("resumedStartupLoadPreservesCancellationOwnership(cancelCaller: \(cancelCaller))", fixture: fixture,
                                   gates: [("initial", initial), ("resumed", resumed)])
        try await Self.lifecycleDeadline(trace, unblocking: {
            initial.release(); resumed.release()
        }, cleanup: {
            await starting?.value
            _ = await resuming?.result
            _ = await fixture.model.end()
            fixture.surface.onLoad = nil
        }) {
            let acceptedHead = try #require(fixture.client.currentHead.items.first)
            fixture.surface.onLoad = {
                loadEntries += 1
                trace.checkpoint = "initial.wait"; try await initial.wait()
                completedLoads += 1
            }
            starting = Task { trace.checkpoint = "starting.start"; await fixture.model.start() }
            trace.checkpoint = "initial.requested"
            await initial.requested()
            // requested() can also finish during cleanup: prove load actually entered.
            try #require(loadEntries == 1 && fixture.client.starts == 1 && fixture.model.sessionID == 5)
            trace.checkpoint = "model.pause"
            try await fixture.model.pause()
            trace.checkpoint = "starting?.value"
            await starting?.value
            let sessionID = try #require(fixture.model.sessionID)
            #expect(initial.cancelled && completedLoads == 0 && fixture.model.paused)
            #expect(fixture.model.item?.cardId == acceptedHead.cardId && fixture.surface.presented.isEmpty)

            fixture.surface.onLoad = {
                loadEntries += 1
                trace.checkpoint = "resumed.wait"; try await resumed.wait()
                completedLoads += 1
            }
            let caller = Task { trace.checkpoint = "resuming.resume"; try await fixture.model.resume() }
            resuming = caller
            trace.checkpoint = "resumed.requested"
            await resumed.requested()
            try #require(loadEntries == 2 && completedLoads == 0 && !resumed.cancelled)
            try #require(fixture.surface.calls.suffix(2) == ["resume", "load"])
            #expect(fixture.model.paused && !fixture.model.canReveal && !fixture.model.canRate)
            #expect(fixture.client.headReads == 0 && fixture.surface.presented.isEmpty)
            if cancelCaller {
                // No second Pause: only this caller cancels the Resume owner.
                caller.cancel()
            } else {
                trace.checkpoint = "model.pause"
                try await fixture.model.pause()
            }
            trace.checkpoint = "caller.result"
            switch await caller.result {
            case .success:
                if cancelCaller { Issue.record("Caller cancellation incorrectly completed startup-load Resume") }
            case .failure(let error):
                #expect(cancelCaller && error is CancellationError)
            }
            #expect(caller.isCancelled == cancelCaller && resumed.cancelled && fixture.model.paused)
            #expect(!fixture.model.canReveal && !fixture.model.canRate)
            #expect(!fixture.model.canUndo && !fixture.model.canRedo)
            fixture.model.reveal(); fixture.model.rate(.good); trace.checkpoint = "rate.settled"; await fixture.model.settled()
            #expect(fixture.surface.reveals == 0 && fixture.surface.presented.isEmpty && completedLoads == 0)
            #expect(fixture.model.sessionID == sessionID && fixture.model.item?.cardId == acceptedHead.cardId)
            #expect(fixture.model.item?.stateToken == acceptedHead.stateToken && fixture.model.item?.renderKey == acceptedHead.renderKey)
            #expect(fixture.client.starts == 1 && fixture.client.headReads == 0 && fixture.client.answers == 0)
            #expect(fixture.client.restores.isEmpty && fixture.client.ends == 0 && fixture.model.record.answers.isEmpty)
            #expect(fixture.surface.calls.filter { $0 == "suspend" }.count == (cancelCaller ? 1 : 2))
            trace.checkpoint = "store.load"
            let saved = try #require(try await fixture.store.load())
            #expect(saved.answers.isEmpty && saved.endedAt == nil)

            fixture.surface.onLoad = { loadEntries += 1; completedLoads += 1 }
            trace.checkpoint = "model.resume"
            try await fixture.model.resume()
            #expect(loadEntries == 3 && completedLoads == 1)
            #expect(!fixture.model.paused && fixture.model.phase == .question && fixture.model.canReveal && !fixture.model.canRate)
            #expect(fixture.model.sessionID == sessionID && fixture.model.item?.cardId == acceptedHead.cardId)
            #expect(fixture.model.item?.stateToken == acceptedHead.stateToken && fixture.model.item?.renderKey == acceptedHead.renderKey)
            #expect(fixture.surface.presented == [acceptedHead.cardId] && fixture.surface.reveals == 0)
            #expect(fixture.client.starts == 1 && fixture.client.headReads == 1 && fixture.client.answers == 0)
            #expect(fixture.client.restores.isEmpty && fixture.client.ends == 0 && fixture.model.record.answers.isEmpty)
            trace.checkpoint = "model.end"
            #expect(await fixture.model.end())
            #expect(fixture.client.ends == 1 && fixture.surface.calls.last == "close")
        }
    }

    @Test(.timeLimit(.minutes(1))) func resumeAfterCloseJoinedWorkCannotRestartPages() async throws {
        let fixture = Fixture(), stopped = CancellableGate()
        var closing: Task<Bool, Never>?
        let trace = LifecycleTrace("resumeAfterCloseJoinedWorkCannotRestartPages()", fixture: fixture,
                                   gates: [("stopped", stopped)])
        try await Self.lifecycleDeadline(trace, unblocking: { stopped.release() }, cleanup: {
            _ = await closing?.value
            _ = await fixture.model.end()
        }) {
            trace.checkpoint = "model.start"
            await fixture.model.start()
            fixture.surface.onSuspend = { trace.checkpoint = "stopped.wait"; _ = try? await stopped.wait() }
            closing = Task { trace.checkpoint = "closing.end"; return await fixture.model.end() }
            trace.checkpoint = "stopped.requested"
            await stopped.requested() // End has joined all original preparation.
            let calls = fixture.surface.calls, reads = fixture.client.headReads
            trace.checkpoint = "model.resume"
            try await fixture.model.resume()
            trace.checkpoint = "model.start"
            await fixture.model.start()
            #expect(fixture.surface.calls == calls && fixture.client.headReads == reads)
            #expect(fixture.model.paused && fixture.client.ends == 0)
            trace.checkpoint = "endedAt.store.load"
            #expect(try await fixture.store.load()?.endedAt == nil)
            stopped.release()
            trace.checkpoint = "closing?.value"
            #expect(await closing?.value == true)
            #expect(fixture.client.ends == 1 && fixture.surface.calls.last == "close")
            trace.checkpoint = "endedAt.store.load"
            #expect(try await fixture.store.load()?.endedAt != nil)
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["core", "render", "present", "failure"], [false, true])
    func acceptedAnswerPreparationIsCancelledWithoutCancellingCommit(stage: String, close: Bool) async throws {
        let fixture = Fixture()
        let accepted = CancellableGate(), preparation = CancellableGate(), stopped = CancellableGate()
        var pausing: Task<Bool, any Error>?
        var order: [String] = []
        var physicallyStopped = false
        let trace = LifecycleTrace("acceptedAnswerPreparationIsCancelledWithoutCancellingCommit(stage: \(stage), close: \(close))", fixture: fixture,
                                   gates: [("accepted", accepted), ("preparation", preparation), ("stopped", stopped)])
        try await Self.lifecycleDeadline(trace, unblocking: {
            accepted.release(); preparation.release(); stopped.release()
        }, cleanup: {
            _ = await pausing?.result
            await fixture.model.settled()
            _ = await fixture.model.end()
            fixture.client.onAnswerResult = nil; fixture.client.onEnd = nil
            fixture.surface.onSuspend = nil; fixture.surface.onRender = nil; fixture.surface.onPresent = nil
            fixture.model.onRecordChanged = nil
        }) {
            trace.checkpoint = "model.start"
            await fixture.model.start()
            fixture.model.reveal(); trace.checkpoint = "reveal.settled"; await fixture.model.settled()
            fixture.client.onAnswerResult = {
                trace.checkpoint = "accepted.wait"; try await accepted.wait()
                // Real ordered fan-out path, including commit-before-result.
                if stage == "core" { fixture.model.receive(Self.answerCommitted()) }
            }
            if stage == "present" { fixture.surface.onPresent = { trace.checkpoint = "preparation.wait"; try await preparation.wait() } }
            else {
                fixture.surface.onRender = {
                    do { trace.checkpoint = "preparation.wait"; try await preparation.wait() }
                    catch {
                        if stage == "failure" { throw BridgeError.internal("post-answer preparation failed") }
                        throw error
                    }
                }
            }
            fixture.surface.onSuspend = {
                order.append("physical-stop")
                trace.checkpoint = "stopped.wait"; _ = try? await stopped.wait()
                physicallyStopped = true
                order.append("stopped")
            }
            fixture.model.onRecordChanged = { record in
                guard record.answers.count == 1 else { return }
                order.append(record.endedAt != nil ? "ended-persist" : physicallyStopped ? "pause-persist" : "accepted-persist")
            }
            fixture.client.onEnd = {
                #expect(order.last == "pause-persist")
                do {
                    trace.checkpoint = "onEnd.store.load"
                    #expect(try await fixture.store.load()?.answers.first?.durable == true)
                } catch {
                    Issue.record(error)
                }
                order.append("CoreEnd")
            }
            fixture.model.rate(.good)
            trace.checkpoint = "accepted.requested"
            await accepted.requested()
            if stage != "core" {
                accepted.release()
                trace.checkpoint = "preparation.requested"
                await preparation.requested()
                fixture.model.receive(Self.answerCommitted())
            }
            fixture.model.beginPause()
            pausing = Task {
                if close { trace.checkpoint = "pausing.end"; return await fixture.model.end() }
                trace.checkpoint = "model.pause"
                try await fixture.model.pause()
                return true
            }
            // A pause requested before the answer returns must still retain it.
            accepted.release()
            trace.checkpoint = "stopped.requested"
            await stopped.requested()
            #expect(!accepted.cancelled && fixture.client.answers == 1 && fixture.client.ends == 0)
            #expect(stage == "core" || preparation.cancelled)
            #expect(fixture.model.record.answers.count == 1 && fixture.model.record.answers[0].durable)
            trace.checkpoint = "store.load"
            let saved = try #require(try await fixture.store.load())
            #expect(saved.answers == fixture.model.record.answers && saved.endedAt == nil)
            #expect(order.last == "physical-stop" && order.contains("accepted-persist"))
            #expect(!fixture.surface.calls.contains("close"))
            if stage == "failure" { #expect(fixture.model.notice?.contains("post-answer preparation failed") == true) }
            else { #expect(fixture.model.notice == nil) }
            stopped.release()
            trace.checkpoint = "pausing?.value"
            #expect(try await pausing?.value == true)
            fixture.surface.onSuspend = nil; fixture.surface.onRender = nil; fixture.surface.onPresent = nil
            if !close {
                trace.checkpoint = "model.resume"
                try await fixture.model.resume()
                #expect(fixture.model.phase == .question && fixture.model.item?.cardId == 2 && fixture.model.canReveal)
                #expect(fixture.surface.presented.last == 2 && fixture.client.headReads > 0)
            }
            trace.checkpoint = "model.end"
            #expect(await fixture.model.end())
            #expect(fixture.client.answers == 1 && fixture.client.ends == 1)
            trace.checkpoint = "ended.store.load"
            let ended = try #require(try await fixture.store.load())
            #expect(ended.answers == saved.answers && ended.endedAt != nil)
            #expect(order.suffix(2) == ["CoreEnd", "ended-persist"] && fixture.surface.calls.last == "close")
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["reveal", "refresh", "undo", "redo", "undo-core", "reveal-failure", "refresh-failure", "undo-failure"], [false, true])
    func visualPreparationIsCancelledAndJoinedBeforeLifecycleStop(path: String, close: Bool) async throws {
        let fixture = Fixture()
        let visual = CancellableGate(), restored = CancellableGate(), stopped = CancellableGate()
        let action = path.replacingOccurrences(of: "-failure", with: "")
        let genuineFailure = path.hasSuffix("-failure")
        var visualJoined = false, physicallyStopped = false
        var pausing: Task<Bool, any Error>?
        let trace = LifecycleTrace("visualPreparationIsCancelledAndJoinedBeforeLifecycleStop(path: \(path), close: \(close))", fixture: fixture,
                                   gates: [("visual", visual), ("restored", restored), ("stopped", stopped)])
        try await Self.lifecycleDeadline(trace, unblocking: {
            visual.release(); restored.release(); stopped.release()
        }, cleanup: {
            _ = await pausing?.result
            await fixture.model.settled()
            _ = await fixture.model.end()
            fixture.client.onAnswerResult = nil; fixture.client.onEnd = nil; fixture.client.onRestoreResult = nil
            fixture.surface.onSuspend = nil; fixture.surface.onReveal = nil; fixture.surface.onRender = nil
        }) {
            trace.checkpoint = "model.start"
            await fixture.model.start()
            fixture.model.reveal(); trace.checkpoint = "reveal.settled"; await fixture.model.settled()
            // Synthetic ordered commit fan-out before the original result.
            fixture.client.onAnswerResult = { fixture.model.receive(Self.answerCommitted()) }
            fixture.model.rate(.good); trace.checkpoint = "rate.settled"; await fixture.model.settled()
            trace.checkpoint = "store.load"
            try #require(try await fixture.store.load()?.answers.first?.durable == true)
            if action == "redo" { fixture.model.restore(); trace.checkpoint = "restore.settled"; await fixture.model.settled() }
            let heldVisual: () async throws -> Void = {
                defer { visualJoined = true }
                do { trace.checkpoint = "visual.wait"; try await visual.wait() }
                catch {
                    if genuineFailure { throw BridgeError.internal("concrete \(action) visual failure") }
                    throw error
                }
            }
            if action == "reveal" { fixture.surface.onReveal = heldVisual }
            else { fixture.surface.onRender = heldVisual }
            if action == "undo-core" { fixture.client.onRestoreResult = { trace.checkpoint = "restored.wait"; try await restored.wait() } }
            fixture.surface.onSuspend = {
                #expect(action == "undo-core" || (visualJoined && visual.cancelled))
                #expect(fixture.client.starts == 1 && fixture.client.answers == 1 && fixture.client.ends == 0)
                trace.checkpoint = "store.load"
                let saved = try? await fixture.store.load()
                #expect(saved?.answers == fixture.model.record.answers && saved?.undone == fixture.model.record.undone)
                #expect(saved?.endedAt == nil)
                trace.checkpoint = "stopped.wait"; _ = try? await stopped.wait()
                physicallyStopped = true
            }
            fixture.client.onEnd = {
                #expect(physicallyStopped)
                trace.checkpoint = "onEnd.store.load"
                let saved = try #require(try await fixture.store.load())
                #expect(saved.answers == fixture.model.record.answers && saved.undone == fixture.model.record.undone)
                #expect(saved.endedAt == nil)
            }
            switch action {
            case "reveal": fixture.model.reveal()
            case "refresh": fixture.model.requestRefresh(question: true)
            default: fixture.model.restore(redo: action == "redo")
            }
            if action == "undo-core" { trace.checkpoint = "restored.requested"; await restored.requested() }
            else { trace.checkpoint = "visual.requested"; await visual.requested() }
            // Redo's restored record becomes durable through ordinary fan-out.
            fixture.model.receive(Self.answerCommitted())
            fixture.model.beginPause()
            pausing = Task {
                if close { trace.checkpoint = "pausing.end"; return await fixture.model.end() }
                trace.checkpoint = "model.pause"
                try await fixture.model.pause()
                return true
            }
            if action == "undo-core" {
                #expect(!fixture.surface.calls.contains("suspend") && fixture.client.ends == 0)
                restored.release()
            }
            trace.checkpoint = "stopped.requested"
            await stopped.requested()
            #expect(!restored.cancelled)
            #expect(action == "undo-core" || (visualJoined && visual.cancelled))
            let expectedRestores: [Bool] = action == "redo" ? [false, true] : action.hasPrefix("undo") ? [false] : []
            #expect(fixture.client.restores == expectedRestores)
            trace.checkpoint = "store.load"
            let saved = try #require(try await fixture.store.load())
            if action.hasPrefix("undo") {
                #expect(saved.answers.isEmpty && saved.undone.map(\.cardID) == [1] && saved.undone.first?.durable == true)
            } else {
                #expect(saved.answers.map(\.cardID) == [1] && saved.answers.first?.durable == true && saved.undone.isEmpty)
            }
            if genuineFailure { #expect(fixture.model.notice?.contains("concrete \(action) visual failure") == true) }
            else { #expect(fixture.model.notice == nil) }
            stopped.release()
            trace.checkpoint = "pausing?.value"
            #expect(try await pausing?.value == true)
            fixture.surface.onSuspend = nil; fixture.surface.onReveal = nil; fixture.surface.onRender = nil
            fixture.client.onRestoreResult = nil
            if !close {
                let reads = fixture.client.headReads
                fixture.client.currentHead = Self.head(3)
                trace.checkpoint = "model.resume"
                try await fixture.model.resume()
                #expect(fixture.client.headReads == reads + 1 && fixture.model.item?.cardId == 3)
                #expect(fixture.model.canReveal && !fixture.model.canRate && fixture.surface.presented.last == 3)
            }
            trace.checkpoint = "model.end"
            #expect(await fixture.model.end())
            #expect(fixture.client.starts == 1 && fixture.client.answers == 1 && fixture.client.restores == expectedRestores)
            #expect(fixture.client.ends == 1 && fixture.surface.calls.last == "close")
            trace.checkpoint = "ended.store.load"
            let ended = try #require(try await fixture.store.load())
            #expect(ended.answers == saved.answers && ended.undone == saved.undone && ended.endedAt != nil)
        }
    }

    @Test(arguments: ["reveal", "refresh", "undo"])
    func uncancelledVisualCancellationErrorRemainsAFailure(path: String) async throws {
        let fixture = Fixture()
        await fixture.model.start()
        if path == "reveal" {
            fixture.surface.onReveal = { throw CancellationError() }
            fixture.model.reveal()
        } else {
            fixture.surface.onRender = { throw CancellationError() }
            if path == "refresh" { fixture.model.requestRefresh(question: true) }
            else { fixture.model.restore() }
        }
        await fixture.model.settled()
        #expect(!fixture.model.paused && fixture.model.phase == .error)
        #expect(fixture.model.notice?.contains("CancellationError") == true)
        #expect(fixture.client.starts == 1 && fixture.client.answers == 0)
        #expect(fixture.client.restores == (path == "undo" ? [false] : []))
        #expect(await fixture.model.end())
    }

    private static func answerCommitted() -> CoreEvent {
        CoreEvent(.init(seq: 1, generation: 1, causeSeq: nil, kind: .changes(changes: .init(
            wholeCollection: false, source: .userUndoable,
            cards: [.init(id: 1, noteId: 1, kind: .updated, memoryStateOnly: false)],
            notes: [], notetypes: [], decks: [], deckConfigs: [], configKeys: [], tags: [],
            revlogAdded: [1], revlogRemoved: [], mediaAdded: [], mediaRemoved: [],
            skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false))))
    }

    @MainActor
    private final class LifecycleTrace {
        let label: String
        var checkpoint = "scenario-not-started"
        unowned let fixture: Fixture
        let gates: [(String, CancellableGate)]

        init(_ label: String, fixture: Fixture, gates: [(String, CancellableGate)]) {
            self.label = label; self.fixture = fixture; self.gates = gates
        }

        // Last reached checkpoint across scenario and callbacks, not proof of a stalled await.
        func snapshot() -> String {
            let client = fixture.client, surface = fixture.surface
            let gateFacts = gates.map { name, gate in
                "\(name)(entries=\(gate.waitEntries),active=\(gate.activeWaits),cancelled=\(gate.cancelled))"
            }.joined(separator: ",")
            return "\(label) checkpoint=\(checkpoint) " +
                "client(starts=\(client.starts),headReads=\(client.headReads),answers=\(client.answers),restores=\(client.restores.count),ends=\(client.ends)) " +
                "surface(presented=\(surface.presented.count),reveals=\(surface.reveals),calls=\(surface.calls)) " +
                "model(paused=\(fixture.model.paused),answers=\(fixture.model.record.answers.count),undone=\(fixture.model.record.undone.count),ended=\(fixture.model.record.endedAt != nil)) " +
                "gates=[\(gateFacts)]"
        }
    }

    private struct LifecycleDeadline: Error, CustomStringConvertible {
        let snapshot: String
        var description: String { "LifecycleDeadline(\(snapshot))" }
    }
    private static func lifecycleDeadline(
        _ trace: LifecycleTrace,
        unblocking: @escaping @MainActor () -> Void,
        cleanup: @escaping @MainActor () async -> Void,
        _ body: @escaping @MainActor @Sendable () async throws -> Void
    ) async throws {
        let results = AsyncStream<Result<Void, any Error>>.makeStream(bufferingPolicy: .bufferingOldest(1))
        let work = Task { @MainActor in
            do { try await body(); results.continuation.yield(.success(())) }
            catch { results.continuation.yield(.failure(error)) }
        }
        let timeout = Task {
            do { try await Task.sleep(for: .seconds(3)) }
            catch { return }
            // Freeze primary evidence before cancellation, gate release, or cleanup.
            let snapshot = trace.snapshot()
            results.continuation.yield(.failure(LifecycleDeadline(snapshot: snapshot)))
        }
        let result: Result<Void, any Error> = await withTaskCancellationHandler {
            for await result in results.stream { return result }
            return .failure(CancellationError())
        } onCancel: {
            work.cancel(); timeout.cancel(); results.continuation.finish()
        }
        work.cancel(); timeout.cancel()
        unblocking()
        await work.value
        await cleanup() // Join retained original work before Fixture removes its root.
        await timeout.value
        results.continuation.finish()
        try result.get()
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["core", "core-cancellation", "renderer", "persistence"])
    func genuineStartupFailureSurvivesLaterCancellation(stage: String) async throws {
        let fixture = Fixture()
        let held = SummaryGate(state: Self.summary(waiting: false))
        let failure: any Error = stage == "core-cancellation" ? CancellationError() :
            BridgeError.internal("concrete \(stage) failure after cancel")
        if stage.hasPrefix("core") {
            fixture.client.onStartResult = { _ = await held.value(); throw failure }
        } else if stage == "renderer" {
            fixture.surface.onRender = { _ = await held.value(); throw failure }
        } else {
            // persist() must report a real filesystem failure even though the
            // startup task was cancelled after presentation, before its save.
            fixture.surface.onPresent = {
                fixture.model.beginPause()
                try Data([0]).write(to: fixture.directory)
            }
        }
        let starting = Task { await fixture.model.start() }
        if stage != "persistence" {
            await held.requested()
            fixture.model.beginPause()
            held.release()
        }
        await starting.value
        #expect(fixture.model.paused && fixture.model.phase == .error)
        let notice = try #require(fixture.model.notice)
        if stage == "core-cancellation" { #expect(notice.contains("CancellationError")) }
        else if stage != "persistence" { #expect(notice.contains("concrete \(stage) failure after cancel")) }
        else {
            #expect(!notice.contains("CancellationError"))
            try FileManager.default.removeItem(at: fixture.directory)
        }
        fixture.surface.onPresent = nil
        #expect(await fixture.model.end())
        #expect(fixture.client.ends == (stage.hasPrefix("core") ? 0 : 1))
    }

    @Test(.timeLimit(.minutes(1))) func genuineResumedRenderFailureSurvivesSecondPause() async throws {
        let fixture = Fixture()
        let initial = CancellableGate(), failed = SummaryGate(state: Self.summary(waiting: false))
        fixture.surface.onRender = { try await initial.wait() }
        let starting = Task { await fixture.model.start() }
        await initial.requested()
        try await fixture.model.pause()
        await starting.value
        fixture.surface.onRender = {
            _ = await failed.value()
            throw BridgeError.internal("resumed renderer failed after cancel")
        }
        let resuming = Task { try await fixture.model.resume() }
        await failed.requested()
        fixture.model.beginPause()
        failed.release()
        switch await resuming.result {
        case .success: Issue.record("resume-render-error-missing")
        case .failure(let error): #expect(String(describing: error).contains("resumed renderer failed after cancel"))
        }
        #expect(fixture.model.phase == .error && fixture.model.paused)
        #expect(fixture.model.notice?.contains("resumed renderer failed after cancel") == true)
        #expect(await fixture.model.end())
        initial.release()
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
        #expect(fixture.model.phase == .waiting)
        #expect(fixture.surface.presented.isEmpty)
        #expect(fixture.model.canContinue)
        fixture.model.continueReview(); await fixture.model.settled()
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
                    #expect(fixture.model.phase == expectedPhase)
                    #expect(fixture.surface.presented.isEmpty)
                    #expect(fixture.model.canContinue)
                    fixture.model.continueReview(); await fixture.model.settled()
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
        let fixture = Fixture(nativeInput: owner, surface: FactSurface())
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
        #expect(receipt.image == nil && receipt.audio == nil && receipt.journal == nil)
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
        let fixture = Fixture(nativeInput: owner, surface: FactSurface())
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

    @Test(arguments: [true, false])
    func revealSurfaceFailurePreservesNoticeAndFailsNativeEvidence(nativeOwned: Bool) async throws {
        let input = try NativeReviewInputFixture()
        defer { input.remove() }
        let owner = nativeOwned ? input.owner() : nil
        if let owner { try await input.activate(owner, bindSession: false) }
        let fixture = Fixture(nativeInput: owner)
        await fixture.model.start()
        #expect(fixture.model.phase == .question)
        if let owner {
            await owner.flushEvidence()
            #expect(try input.receipt(owner).failure == nil)
        }
        let failure = CardPage.Failure.notPrepared
        fixture.surface.onReveal = { throw failure }
        fixture.model.reveal(); await fixture.model.settled()
        #expect(fixture.surface.reveals == 1)
        #expect(fixture.model.phase == .error)
        #expect(fixture.model.notice == String(describing: failure))
        #expect(!fixture.model.canReveal && !fixture.model.canRate)
        fixture.model.rate(.easy); await fixture.model.settled()
        #expect(fixture.client.answers == 0 && fixture.client.elapsed.isEmpty && fixture.client.used.isEmpty)
        #expect(fixture.model.record.answers.isEmpty)
        if let owner {
            await owner.flushEvidence()
            let receipt = try input.receipt(owner)
            #expect(owner.flushStatus == .saved)
            #expect(receipt.failure == String(describing: failure))
            #expect(receipt.admission == nil && receipt.completion == nil)
            #expect(receipt.image == nil && receipt.audio == nil && receipt.journal == nil)
        }
        _ = await fixture.model.end()
    }

    @Test func nativeTapWithoutRendererRefusesBeforeAnswerDispatch() async throws {
        let input = try NativeReviewInputFixture()
        defer { input.remove() }
        let owner = input.owner()
        try await input.activate(owner, bindSession: false)
        let fixture = Fixture(nativeInput: owner)
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.easy); await fixture.model.settled()
        #expect(fixture.client.answers == 0 && fixture.client.elapsed.isEmpty)
        #expect(fixture.model.phase == .error && fixture.model.record.answers.isEmpty)
        #expect(fixture.model.notice?.contains("missing native renderer") == true)
        await owner.flushEvidence()
        #expect(try input.receipt(owner).admission == nil)
        #expect(try input.receipt(owner).completion == nil)
        _ = await fixture.model.end()
    }

    final class Fixture {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let client = Client()
        let surface: Surface
        let store: ReviewRecordStore
        var model: ReviewCoordinator!
        var now: TimeInterval = 0
        init(nativeInput: NativeReviewInput? = nil, surface: Surface = Surface(),
             recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in }) {
            self.surface = surface
            store = ReviewRecordStore(file: directory.appending(path: "session.json"))
            model = ReviewCoordinator(deckID: 1, surface: surface, store: store,
                clock: .fixed(Date(timeIntervalSince1970: nativeInput == nil ? 1_800_000_000 : 1_894_698_000)), client: client,
                uptime: { [weak self] in self?.now ?? 0 }, nativeInput: nativeInput, nativeGeneration: 1,
                recordDiagnostic: recordDiagnostic)
            model.retryMedia = { [surface] names in surface.arrivals.append(names); return true }
        }
        isolated deinit { try? FileManager.default.removeItem(at: directory) }
    }
    class Surface: ReviewCardSurface {
        var pair: CardPagePair? { nil }
        var onAction: ((CardBridgeAction) -> Void)?
        var onRecovery: (() -> Void)?
        var editing = false
        var reveals = 0
        var presented: [Int64] = []
        var calls: [String] = []
        var onLoad: (() async throws -> Void)?
        var onRender: (() async throws -> Void)?
        var onSuspend: (() async -> Void)?
        var onReveal: (() async throws -> Void)?
        var onResume: (() -> Void)?
        var onPresent: (() async throws -> Void)?
        var arrivals: [[String]] = []
        func load() async throws { calls.append("load"); try await onLoad?() }
        func render(sessionID: UInt64, items: [ReviewHeadItem]) async throws { calls.append("render"); try await onRender?() }
        func present(item: ReviewHeadItem, inputTime: TimeInterval) async throws {
            presented.append(item.cardId); calls.append("present"); try await onPresent?()
        }
        func preload(item: ReviewHeadItem) async throws {}
        func reveal(inputTime: TimeInterval) async throws { reveals += 1; try await onReveal?() }
        func admitNativeInput(_ input: NativeReviewInput, coordinator: UUID, session: UInt64, generation: UInt64,
                              deck: Int64, item: ReviewHeadItem, rating: ReviewRating, instant: Int64,
                              eligible: Bool) throws -> NativeReviewInput.AnswerUse {
            guard let pair else { throw NativeReviewInput.Refusal(reason: "missing native renderer") }
            try input.validateMediaCurrent(pair: pair, coordinator: coordinator, session: session, generation: generation)
            return try input.admitFacts(coordinator: coordinator, session: session, generation: generation,
                deck: deck, item: item, rating: rating, instant: instant, eligible: eligible)
        }
        func suspend() async { calls.append("suspend"); await onSuspend?() }
        func resume() async { calls.append("resume"); onResume?() }
        func close() { calls.append("close") }
    }
    // Test double for facts only; it has no renderer or media evidence.
    final class FactSurface: Surface {
        override func admitNativeInput(
            _ request: NativeReviewInput,
            coordinator owner: UUID,
            session sessionID: UInt64,
            generation generationID: UInt64,
            deck deckID: Int64,
            item head: ReviewHeadItem,
            rating choice: ReviewRating,
            instant time: Int64,
            eligible allowed: Bool
        ) throws -> NativeReviewInput.AnswerUse {
            return try request.admitFacts(
                coordinator: owner, session: sessionID, generation: generationID,
                deck: deckID, item: head, rating: choice, instant: time, eligible: allowed
            )
        }
    }
    // Model admission/ordering only; this gate supplies no renderer evidence.
    final class CancellableGate {
        private let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        private let held = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        private(set) var cancelled = false
        private(set) var waitEntries = 0
        private(set) var activeWaits = 0
        func wait() async throws {
            waitEntries += 1; activeWaits += 1
            defer { activeWaits -= 1 }
            entered.continuation.yield(())
            for await _ in held.stream { break }
            cancelled = Task.isCancelled
            try Task.checkCancellation()
        }
        func requested() async { for await _ in entered.stream { return } }
        func release() { held.continuation.finish(); entered.continuation.finish() }
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
        var starts = 0
        var answers = 0
        var answerFailure = false
        var elapsed: [UInt64] = []
        var used: [(session: UInt64, item: ReviewHeadItem, rating: ReviewRating, wall: Int64)] = []
        var restores: [Bool] = []
        var restoredHead: ReviewHead?
        var ends = 0
        var headReads = 0
        var refusal: ReviewRefusal?
        var summary: ReviewDeckStudyState?
        var summaryGate: SummaryGate?
        var onStartResult: (() async throws -> Void)?
        var onAnswerResult: (() async throws -> Void)?
        var onRestoreResult: (() async throws -> Void)?
        var onEnd: (() async throws -> Void)?
        var onHeadResult: (() async throws -> Void)?
        #if DEBUG
        var onStartObservation: ((ReviewStartObservation) -> Void)?
        func start(deckID: Int64, observation: ReviewStartObservation) async throws -> ReviewStartResult {
            onStartObservation?(observation)
            return try await start(deckID: deckID)
        }
        #endif
        var currentHead = ReviewCoordinatorTests.head(1)
        var answerHead = ReviewAnswerOutcome.accepted(headDiff: ReviewHeadDiff(head: ReviewCoordinatorTests.head(2), marks: [], removed: [1]), leech: false, suspended: false, rolledOver: false)
        func start(deckID: Int64) async throws -> ReviewStartResult {
            starts += 1
            if let refusal { return .refused(reason: refusal) }
            let result = ReviewStartResult.started(sessionId: 5, generation: 1, head: currentHead, timeLim: 0)
            try await onStartResult?()
            return result
        }
        func head(sessionID: UInt64) async throws -> ReviewHead {
            headReads += 1
            let head = currentHead
            try await onHeadResult?()
            return head
        }
        func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome {
            answers += 1
            try await onAnswerResult?()
            self.elapsed.append(elapsed)
            used.append((sessionID, item, rating, at))
            if answerFailure { throw BridgeError.internal("native input completion failure") }
            if case .accepted(let diff, _, _, _) = answerHead { currentHead = diff.head }
            return answerHead
        }
        func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus {
            .available(step: ReviewStepInfo(id: 1, name: "Answer Card", kind: .answer, session: sessionID, longWrite: nil))
        }
        func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead {
            restores.append(redo)
            try await onRestoreResult?()
            currentHead = restoredHead ?? ReviewCoordinatorTests.head(redo ? 2 : 1)
            return currentHead
        }
        func studyState(deckID: Int64) async throws -> ReviewDeckStudyState {
            if let summaryGate { return await summaryGate.value() }
            guard let summary else { throw BridgeError.internal("No empty head scripted") }
            return summary
        }
        func end(sessionID: UInt64) async throws { try await onEnd?(); ends += 1 }
    }
    static func emptyHead(_ terminal: ReviewTerminalState) -> ReviewHead {
        ReviewHead(items: [], counts: .init(new: 0, learning: 0, review: 0), validUntil: Int64.max, terminalState: terminal)
    }
    static func summary(waiting: Bool) -> ReviewDeckStudyState {
        let counts = ReviewCounts(new: 0, learning: waiting ? 1 : 0, review: 0)
        return ReviewDeckStudyState(deck: 1, queueCounts: counts, listCounts: counts,
            difference: .init(new: 0, learning: 0, review: 0), finished: !waiting,
            learningRemainingToday: waiting ? 1 : 0, learningLaterToday: 0,
            nextLearningDue: waiting ? 1_800_000_600 : nil, nextLearningReadyAt: waiting ? 1_800_000_600 : nil,
            reviewLimitReached: false, newLimitReached: false, learningRemaining: waiting ? 1 : 0,
            secsUntilNextLearn: waiting ? 600 : 0, reviewRemaining: false, newRemaining: false,
            siblingBuried: 0, userBuried: 0, tomorrow: .init(new: 0, learning: 0, review: 0), empty: !waiting)
    }
    static func head(_ id: Int64, showTimer: Bool = false, stopTimerOnAnswer: Bool = false,
                     maximumAnswerSeconds: UInt32 = 60) -> ReviewHead {
        let state = ReviewSchedulingState.new(position: 1)
        let answer = ReviewAnswerState(state: state, delay: .days(amount: 1), left: 0, factor: 0,
            revlog: ReviewRevlogPreview(interval: 1, lastInterval: 0, kind: 0, factor: 0),
            label: ReviewAnswerLabel(lessThan: false, amount: "1", unit: .days))
        let item = ReviewHeadItem(cardId: id, noteId: id, kind: .new,
            stateToken: ReviewStateToken(bytes: Array(repeating: UInt8(id), count: 32)),
            renderKey: ReviewRenderKey(bytes: Array(repeating: UInt8(id), count: 32)),
            nextStates: ReviewSchedulingStates(current: state, again: answer, hard: answer, good: answer, easy: answer),
            predictedNext: ReviewPredictions(again: 2, hard: 2, good: 2, easy: 2), flag: 0, marked: false,
            homePreset: ReviewHomePreset(id: 1, autoplay: false, skipQuestionWhenReplayingAnswer: false, showTimer: showTimer,
                stopTimerOnAnswer: stopTimerOnAnswer, maximumAnswerSeconds: maximumAnswerSeconds, questionSeconds: 0, answerSeconds: 0,
                questionAction: 0, answerAction: 0, waitForAudio: false, leechAction: 0))
        return ReviewHead(items: [item], counts: ReviewCounts(new: 1, learning: 0, review: 0), validUntil: Int64.max, terminalState: .active)
    }
}
