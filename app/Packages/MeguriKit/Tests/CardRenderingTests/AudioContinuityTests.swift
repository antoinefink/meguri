@testable import CardRendering
import Dispatch
import Foundation
import Synchronization
import Testing

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
        let hardware = SessionHardware(perform: output.perform, outputIdentity: identity,
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
        session.configurationChanged(engine: ObjectIdentifier(staleEngine))
        await session.hardwareSettled()
        #expect(output.operations == initial)
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
        await session.hardwareSettled()
        let ended = output.operations
        route = "a2dp:headset-A"
        session.configurationChanged(engine: identity)
        session.emit(.available)
        await session.hardwareSettled()
        #expect(output.operations == ended && ended.last == .deactivate)
        withExtendedLifetime((engine, staleEngine)) {}
    }

    @Test func failedConfigurationRepairFallsBackWithoutRepeatedAttempts() async throws {
        let output = Output(), engine = NSObject(), keeper = UUID(), word = UUID()
        let identity = ObjectIdentifier(engine)
        let hardware = SessionHardware(perform: output.perform, outputIdentity: identity,
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
        gate.signal()
        await session.hardwareSettled()
        output.render(seconds: 600)
        #expect(output.frames == 0 && output.operations.last == .deactivate)
        let stopped = output.operations
        session.release(word); session.releaseContinuity(keeper)
        await session.hardwareSettled()
        #expect(output.operations == stopped)
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

    @Test func partialActivationAndFailedReleaseAreReconciled() async throws {
        let output = Output()
        let hardware = SessionHardware(perform: output.perform)
        output.fail(.activate)
        do {
            try await hardware.set(audible: false, continuity: true, revision: 1)
            Issue.record("Synthetic activation must fail")
        } catch {}
        #expect(output.operations.contains(.deactivate))
        try await hardware.set(audible: true, continuity: false, revision: 2)
        #expect(output.operations.last == .activate)
        output.fail(.deactivate)
        do {
            try await hardware.set(audible: false, continuity: false, revision: 3)
            Issue.record("Synthetic deactivation must fail")
        } catch {}
        try await hardware.set(audible: false, continuity: false, revision: 4)
        #expect(output.operations.last == .deactivate)
        let count = output.operations.count
        try await hardware.set(audible: true, continuity: true, revision: 2)
        #expect(output.operations.count == count) // Stale callback cannot reopen output.
    }

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
