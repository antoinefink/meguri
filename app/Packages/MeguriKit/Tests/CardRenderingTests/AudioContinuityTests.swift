@testable import CardRendering
import Dispatch
import Foundation
import Synchronization
import Testing
#if os(iOS)
import AVFoundation
#endif

@Suite struct AudioContinuityTests {
    private nonisolated final class Output: Sendable {
        nonisolated struct State: Sendable {
            var operations: [SessionHardware.Operation] = []
            var failNext: SessionHardware.Operation?
            var rendering = false
            var renderedFrames: Int = 0
        }
        nonisolated enum Failure: Error { case synthetic }
        let state = Mutex(State())
        func perform(_ operation: SessionHardware.Operation) throws {
            try state.withLock {
                $0.operations.append(operation)
                // Model partial startup: failure still requires physical cleanup.
                if operation == .startSilence { $0.rendering = true }
                if operation == .stopSilence { $0.rendering = false }
                if $0.failNext == operation {
                    $0.failNext = nil
                    throw Failure.synthetic
                }
            }
        }
        var operations: [SessionHardware.Operation] { state.withLock { $0.operations } }
        func fail(_ operation: SessionHardware.Operation) { state.withLock { $0.failNext = operation } }
        func render(seconds: Int) { state.withLock { if $0.rendering { $0.renderedFrames += seconds * 48_000 } } }
        var frames: Int { state.withLock { $0.renderedFrames } }
    }

    @Test func configurationRepairsOnlyStoppedOutputOnTheSameRouteAndLease() async throws {
        let output = Output(), keeper = UUID(), word = UUID()
        let engine = NSObject(), staleEngine = NSObject()
        let identity = ObjectIdentifier(engine)
        let hardware = SessionHardware(perform: output.perform,
            isRendering: { output.state.withLock { $0.rendering } })
        var route: String? = "a2dp:headset-A"
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { route != nil }, routeIdentity: { route })
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        try await session.activate(word)
        let initial = output.operations
        session.configurationChanged(engine: identity) // Category notification; still rendering.
        session.routeChanged(previous: route, disconnected: false)
        await session.hardwareSettled()
        #expect(output.operations == initial)
        output.state.withLock { $0.rendering = false } // Apple stopped/uninitialized the engine.
        // No yield: the foreign event occupies the work slot immediately
        // before the owned event. The owned graph still gets reevaluated.
        session.configurationChanged(engine: ObjectIdentifier(staleEngine))
        session.configurationChanged(engine: identity)
        await session.hardwareSettled()
        #expect(output.operations == initial + [.stopSilence, .startSilence])
        // Recovery never activates, changes ducking, or submits a word.
        let repaired = output.operations
        session.configurationChanged(engine: identity)
        await session.hardwareSettled()
        #expect(output.operations == repaired)
        output.state.withLock { $0.rendering = false }
        route = "a2dp:headset-B" // Same port type, different UID.
        session.configurationChanged(engine: identity)
        await session.hardwareSettled()
        #expect(output.operations == repaired)
        session.routeChanged(previous: "a2dp:headset-A", disconnected: false)
        session.release(word) // Model the audible owner's completed physical pause.
        await session.hardwareSettled()
        let ended = output.operations
        route = "a2dp:headset-A"
        session.configurationChanged(engine: identity)
        session.emit(.available)
        await session.hardwareSettled()
        #expect(output.operations == ended && ended.last == .deactivate)
        withExtendedLifetime((engine, staleEngine)) {}
    }

    @Test func configurationRepairSurvivesAudibleRevisionsWithinContinuityLease() async throws {
        let output = Output()
        let hardware = SessionHardware(perform: output.perform,
            isRendering: { output.state.withLock { $0.rendering } })
        // One continuity lifetime spans unrelated word/HTML activation and
        // release revisions. Its graph's revision remains the original one.
        try await hardware.set(audible: false, continuity: true, revision: 1, lifetime: 1)
        await hardware.settled()
        try await hardware.set(audible: true, continuity: true, revision: 2, lifetime: 1)
        try await hardware.set(audible: false, continuity: true, revision: 3, lifetime: 1)
        let initial = output.operations
        output.state.withLock { $0.rendering = false }
        await hardware.configurationChanged(route: "a2dp:A", lifetime: 1)
        #expect(output.operations == initial + [.stopSilence, .startSilence])
        try await hardware.set(audible: false, continuity: false, revision: 4, lifetime: 2)
        await hardware.settled()
        let closed = output.operations
        await hardware.configurationChanged(route: "a2dp:A", lifetime: 1)
        #expect(output.operations == closed)
        try await hardware.set(audible: false, continuity: true, revision: 5, lifetime: 3)
        await hardware.settled()
        output.state.withLock { $0.rendering = false }
        let restarted = output.operations
        await hardware.configurationChanged(route: "a2dp:A", lifetime: 1)
        #expect(output.operations == restarted) // Prior lease cannot repair a new graph.
        await hardware.configurationChanged(route: "a2dp:A", lifetime: 3)
        #expect(output.operations == restarted + [.stopSilence, .startSilence])
        try await hardware.set(audible: false, continuity: false, revision: 6, lifetime: 4)
        await hardware.settled()
    }

    @Test func failedConfigurationRepairFallsBackWithoutRepeatedAttempts() async throws {
        let output = Output(), engine = NSObject(), keeper = UUID(), word = UUID()
        let identity = ObjectIdentifier(engine)
        let hardware = SessionHardware(perform: output.perform,
            isRendering: { output.state.withLock { $0.rendering } })
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true }, routeIdentity: { "a2dp:A" })
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        output.state.withLock { $0.rendering = false }
        output.fail(.startSilence)
        session.configurationChanged(engine: identity)
        await session.hardwareSettled()
        let failed = output.operations
        session.configurationChanged(engine: identity)
        await session.hardwareSettled()
        #expect(output.operations == failed && failed.last == .deactivate)
        try await session.activate(word)
        #expect(output.operations.last == .activate)
        #expect(output.operations.filter { $0 == .startSilence }.count == 2)
        session.release(word); session.releaseContinuity(keeper)
        await session.hardwareSettled()
        withExtendedLifetime(engine) {}
    }

    @Test func disconnectionDuringSystemActivationRefusesFirstWordBeforeRouteNotification() async throws {
        let gate = DispatchSemaphore(value: 0)
        let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        defer { gate.signal(); entered.continuation.finish() }
        let output = Output(), word = UUID()
        let hardware = SessionHardware(perform: { operation in
            if operation == .activate { entered.continuation.yield(()); gate.wait() }
            try output.perform(operation)
        })
        var route: String? = "a2dp:A"
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { route != nil }, routeIdentity: { route })
        var events: [AudioSessionCoordinator.Event] = []
        let observer = session.observe { events.append($0) }
        defer { session.removeObserver(observer) }
        let activation = Task { try await session.activate(word) }
        try await Spike.deadline("system activation held", timeout: .seconds(3)) {
            for await _ in entered.stream { return }
            throw Spike.Failure("Activation did not enter")
        }
        route = nil // The system route has changed; no notification delivered yet.
        gate.signal()
        await #expect(throws: CancellationError.self) { try await activation.value }
        await session.hardwareSettled()
        #expect(events == [.pause] && output.operations.last == .deactivate)
    }

    @Test func zeroOutputPersistsAcrossLongGapsWithoutSessionChurn() async throws {
        let output = Output(), keeper = UUID(), word = UUID(), html = UUID()
        let hardware = SessionHardware(perform: output.perform)
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations == [.category(ducking: false), .activate, .startSilence])
        output.render(seconds: 3)
        try await session.activate(word)
        try await session.activate(html)
        session.release(word)
        await session.hardwareSettled()
        #expect(output.operations.last == .category(ducking: true)) // HTML still owns ducking.
        session.release(html)
        await session.hardwareSettled()
        let idle = output.operations
        #expect(idle.last == .category(ducking: false))
        // Advance the synthetic output by twenty minutes, with no real sleep.
        output.render(seconds: 20 * 60)
        for _ in 0..<3 { session.maintainContinuity(keeper) }
        await session.hardwareSettled()
        #expect(output.frames == 1_203 * 48_000 && output.operations == idle)
        try await session.activate(word)
        session.release(word)
        await session.hardwareSettled()
        #expect(output.operations.filter { $0 == .startSilence }.count == 1)
        #expect(!output.operations.contains(.deactivate))
        session.releaseContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations.suffix(2) == [.stopSilence, .deactivate])
    }

    @Test func lifecycleEndsOutputAndAvailabilityCannotRestartIt() async throws {
        for event in [AudioSessionCoordinator.Event.stop, .pause] {
            let output = Output(), keeper = UUID(), audible = UUID()
            let hardware = SessionHardware(perform: output.perform)
            let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
            session.maintainContinuity(keeper)
            await session.hardwareSettled()
            try await session.activate(audible)
            session.emit(event)
            await session.hardwareSettled()
            #expect(!output.operations.contains(.deactivate)) // Audible cleanup still owns the lease.
            session.release(audible)
            await session.hardwareSettled()
            let stopped = output.operations
            session.emit(.available)
            session.release(audible); session.releaseContinuity(keeper)
            await session.hardwareSettled()
            output.render(seconds: 600)
            #expect(output.frames == 0 && output.operations == stopped)
            #expect(stopped.suffix(2) == [.stopSilence, .deactivate])
        }
    }

    @Test func audibleActivationDoesNotWaitAndLifecycleRevokesLateSilentStartup() async throws {
        let gate = DispatchSemaphore(value: 0)
        let entered = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        defer { gate.signal(); entered.continuation.finish() }
        let output = Output(), keeper = UUID(), word = UUID()
        let hardware = SessionHardware(perform: { operation in
            if operation == .startSilence {
                entered.continuation.yield(())
                gate.wait()
            }
            try output.perform(operation)
        })
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
        session.maintainContinuity(keeper)
        try await Spike.deadline("silent worker entered", timeout: .seconds(3)) {
            for await _ in entered.stream { return }
            throw Spike.Failure("silent worker did not enter")
        }
        try await Spike.deadline("word activation independent of keeper", timeout: .seconds(3)) {
            try await session.activate(word)
        }
        #expect(output.operations.last == .category(ducking: true))
        session.emit(.pause)
        session.emit(.available)
        session.release(word)
        gate.signal()
        await session.hardwareSettled()
        output.render(seconds: 600)
        #expect(output.frames == 0 && output.operations.last == .deactivate)
        let stopped = output.operations
        session.release(word); session.releaseContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations == stopped)
    }

    @Test(arguments: [AudioSessionCoordinator.Event.pause, .stop])
    func lifecycleRetainsAudibleLeaseUntilHeldPhysicalCleanupCompletes(_ event: AudioSessionCoordinator.Event) async throws {
        let fixture = MediaFixture()
        fixture.add("/word.mp3", Data([1]), "audio/mpeg")
        let output = Output(), backend = CardAudioControllerTests.Backend()
        let hardware = SessionHardware(perform: output.perform)
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
        let audio = CardAudioController(resolver: try await fixture.resolved(), backend: backend, session: session)
        let held = CardAudioControllerTests.Hold()
        defer { held.release(); audio.close() }
        audio.admitReview()
        audio.present(payload: CardPayload(cardID: 1, question: "", answer: "", questionAudio: [.sound(name: "word.mp3")]),
                      update: CardUpdate(card: 1, generation: 1, side: .question))
        #expect(await backend.nextPlay() == "word.mp3")
        let clip = try #require(backend.clips.first)
        if event == .pause { clip.pauseHold = held } else { clip.stopHold = held }
        session.emit(event)
        await held.reached()
        await session.hardwareSettled()
        #expect(!output.operations.contains(.deactivate))
        #expect(!clip.isPaused && !clip.stopped)
        session.emit(.available)
        #expect(backend.events == ["word.mp3"])
        held.release()
        await audio.transportSettled()
        await session.hardwareSettled()
        #expect(output.operations.filter { $0 == .deactivate }.count == 1)
        #expect(event == .pause ? clip.isPaused : clip.stopped)
        #expect(backend.events == ["word.mp3"])
    }

    @Test func reviewStopReleasesIdleContinuityAndFencesOldIntentWithoutAffectingAnotherReview() async throws {
        let fixture = MediaFixture()
        fixture.add("/word.mp3", Data([1]), "audio/mpeg")
        let resolver = try await fixture.resolved()
        let output = Output(), backend = CardAudioControllerTests.Backend()
        let hardware = SessionHardware(perform: output.perform)
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
        let audio = CardAudioController(resolver: resolver, backend: backend, session: session)
        let other = UUID()
        defer { audio.close(); session.releaseContinuity(other) }
        audio.admitReview()
        await session.hardwareSettled()
        #expect(output.operations.isEmpty) // Admission alone has no playable resource.
        let payload = CardPayload(cardID: 1, question: "", answer: "", questionAudio: [.sound(name: "word.mp3")])
        let update = CardUpdate(card: 1, generation: 1, side: .question)
        audio.present(payload: payload, update: update)
        #expect(await backend.nextPlay() == "word.mp3")
        let clip = try #require(backend.clips.first)
        clip.onEnd?(true)
        await audio.transportSettled()
        await session.hardwareSettled()
        #expect(audio.current == nil && output.operations.contains(.startSilence))
        session.maintainContinuity(other)
        let oldIntent = audio.audioIntent
        audio.stop() // The pair's terminal/error teardown uses this same physical stop.
        await audio.transportSettled()
        await session.hardwareSettled()
        #expect(!output.operations.contains(.deactivate))
        session.releaseContinuity(other)
        await session.hardwareSettled()
        #expect(output.operations.suffix(2) == [.stopSilence, .deactivate])
        let stopped = output.operations
        audio.admitReview(intent: oldIntent)
        await audio.prepare(payload: payload, update: update, intent: oldIntent)?.value
        audio.present(payload: payload, update: update, intent: oldIntent)
        audio.resume()
        await audio.transportSettled()
        await session.hardwareSettled()
        #expect(output.operations == stopped && backend.events == ["word.mp3"])
        let fresh = CardUpdate(card: 1, generation: 2, side: .question)
        audio.admitReview(intent: audio.audioIntent)
        audio.present(payload: payload, update: fresh, intent: audio.audioIntent)
        #expect(await backend.nextPlay() == "word.mp3")
        audio.stop()
        await audio.transportSettled()
        await session.hardwareSettled()
        #expect(output.operations.filter { $0 == .deactivate }.count == 2)
    }

    @Test func failedSilentStartupCleansUpAndNormalPlaybackStillStarts() async throws {
        let output = Output(), keeper = UUID(), word = UUID()
        output.fail(.startSilence)
        let hardware = SessionHardware(perform: output.perform)
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations.suffix(2) == [.stopSilence, .deactivate])
        try await session.activate(word)
        #expect(output.operations.suffix(2) == [.category(ducking: true), .activate])
        session.release(word)
        await session.hardwareSettled()
        #expect(output.operations.last == .deactivate)
        #expect(output.operations.filter { $0 == .startSilence }.count == 1)
        // A genuinely fresh lease can attempt silence again.
        session.releaseContinuity(keeper)
        await session.hardwareSettled()
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations.last == .startSilence)
        session.releaseContinuity(keeper)
        await session.hardwareSettled()
    }

    @Test func failedIdleMixTransitionReleasesDuckingAndNormalPlaybackCanRecover() async throws {
        let output = Output(), keeper = UUID(), word = UUID()
        let hardware = SessionHardware(perform: output.perform)
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        try await session.activate(word)
        output.fail(.category(ducking: false))
        session.release(word)
        await session.hardwareSettled()
        #expect(output.operations.suffix(2) == [.stopSilence, .deactivate])
        output.render(seconds: 600)
        #expect(output.frames == 0)
        try await session.activate(word)
        session.release(word); session.releaseContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations.last == .deactivate)
    }

    @Test func failedSilenceRetriesForNewGenerationBeforeOldOffSnapshot() async throws {
        let output = Output()
        // Deliver snapshots in the order the worker must tolerate: new on,
        // then the old off, whose lower revision must remain ineffective.
        let worker = SessionHardware(perform: output.perform)
        output.fail(.startSilence)
        try await worker.set(audible: false, continuity: true, revision: 1, lifetime: 1)
        await worker.settled()
        try await worker.set(audible: false, continuity: true, revision: 2, lifetime: 1)
        await worker.settled()
        #expect(output.operations.filter { $0 == .startSilence }.count == 1)
        try await worker.set(audible: false, continuity: true, revision: 4, lifetime: 2)
        await worker.settled()
        let restarted = output.operations
        #expect(restarted.filter { $0 == .startSilence }.count == 2)
        try await worker.set(audible: false, continuity: false, revision: 3, lifetime: 1)
        await worker.settled()
        #expect(output.operations == restarted)
        try await worker.set(audible: false, continuity: false, revision: 5, lifetime: 2)
    }

    @Test func failedSilenceIsSharedUntilLastReviewOwnerReleases() async throws {
        let output = Output(), first = UUID(), second = UUID()
        let hardware = SessionHardware(perform: output.perform)
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { true })
        output.fail(.startSilence)
        session.maintainContinuity(first)
        await session.hardwareSettled()
        session.maintainContinuity(second)
        session.releaseContinuity(first)
        await session.hardwareSettled()
        #expect(output.operations.filter { $0 == .startSilence }.count == 1)
        // No hardware turn between the last release and a fresh review owner.
        session.releaseContinuity(second)
        session.maintainContinuity(first)
        await session.hardwareSettled()
        #expect(output.operations.filter { $0 == .startSilence }.count == 2)
        session.releaseContinuity(first)
        await session.hardwareSettled()
    }

    @Test(arguments: [false, true])
    func failedActivationAndCleanupRequireFreshActivation(audible: Bool) async throws {
        let output = Output()
        let cleanupAttempts = audible ? 1 : 2
        let failures = Mutex([SessionHardware.Operation.activate]
            + Array(repeating: .deactivate, count: cleanupAttempts))
        let hardware = SessionHardware(perform: { operation in
            try output.perform(operation)
            try failures.withLock {
                if $0.first == operation {
                    $0.removeFirst()
                    throw Output.Failure.synthetic
                }
            }
        })
        await #expect(throws: Output.Failure.self) {
            try await hardware.set(audible: audible, continuity: !audible, revision: 1)
        }
        // Keep the existing cleanup attempts, including the idle-continuity catch.
        #expect(output.operations.filter { $0 == .activate }.count == 1)
        #expect(output.operations.filter { $0 == .deactivate }.count == cleanupAttempts)
        try await hardware.set(audible: true, continuity: false, revision: 2)
        #expect(output.operations.last == .activate)
        #expect(output.operations.filter { $0 == .activate }.count == 2)
        try await hardware.set(audible: false, continuity: false, revision: 3)
        #expect(output.operations.filter { $0 == .deactivate }.count == cleanupAttempts + 1)
    }

    @Test func partialActivationAndFailedReleaseAreReconciled() async throws {
        let output = Output()
        let hardware = SessionHardware(perform: output.perform)
        output.fail(.activate)
        do {
            try await hardware.set(audible: false, continuity: true, revision: 1)
            Issue.record("Synthetic activation must fail")
        } catch {}
        #expect(output.operations.filter { $0 == .deactivate }.count == 1)
        try await hardware.set(audible: true, continuity: false, revision: 2)
        #expect(output.operations.last == .activate)
        output.fail(.deactivate)
        do {
            try await hardware.set(audible: false, continuity: false, revision: 3)
            Issue.record("Synthetic deactivation must fail")
        } catch {}
        #expect(output.operations.filter { $0 == .deactivate }.count == 2)
        try await hardware.set(audible: false, continuity: false, revision: 4)
        #expect(output.operations.last == .deactivate && output.operations.filter { $0 == .deactivate }.count == 3)
        let count = output.operations.count
        try await hardware.set(audible: true, continuity: true, revision: 2)
        #expect(output.operations.count == count) // Stale callback cannot reopen output.
    }

    #if os(iOS)
    @Test func busyNativeDeactivationRequiresReactivationButBusyActivationStillFails() async throws {
        let output = Output()
        let busy = NSError(domain: NSOSStatusErrorDomain, code: AVAudioSession.ErrorCode.isBusy.rawValue)
        let hardware = SessionHardware(perform: { operation in
            try output.perform(operation)
            if operation == .deactivate { throw busy }
        })
        try await hardware.set(audible: true, continuity: false, revision: 1)
        try await hardware.set(audible: false, continuity: false, revision: 2)
        try await hardware.set(audible: false, continuity: false, revision: 3)
        try await hardware.set(audible: true, continuity: false, revision: 4)
        #expect(output.operations.filter { $0 == .activate }.count == 2)
        #expect(output.operations.filter { $0 == .deactivate }.count == 1)
        try await hardware.set(audible: false, continuity: false, revision: 5)
        let failing = SessionHardware(perform: { operation in
            if operation == .activate { throw busy }
        })
        await #expect(throws: NSError.self) {
            try await failing.set(audible: true, continuity: false, revision: 1)
        }
    }
    #endif

    @Test func ineligibleRouteDoesNotOpenOutputAndContinuityReleasePreservesHTML() async throws {
        let output = Output(), keeper = UUID(), html = UUID()
        let hardware = SessionHardware(perform: output.perform)
        var bluetooth = false
        let session = AudioSessionCoordinator(hardware: hardware, continuityEligible: { bluetooth })
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations.isEmpty)
        try await session.activate(html)
        #expect(!output.operations.contains(.startSilence))
        bluetooth = true
        session.maintainContinuity(keeper)
        await session.hardwareSettled()
        session.releaseContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations.last == .stopSilence && !output.operations.contains(.deactivate))
        session.release(html)
        await session.hardwareSettled()
        #expect(output.operations.last == .deactivate)
    }
}
