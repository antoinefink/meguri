import Foundation
import MeguriCoreBindings
import Synchronization
import Testing

@testable import CoreBridge

/// The name of the calling thread, as the OS reports it.
func currentThreadName() -> String {
    var buffer = [CChar](repeating: 0, count: 64)
    pthread_getname_np(pthread_self(), &buffer, buffer.count)
    return String(decoding: buffer.prefix { $0 != 0 }.map(UInt8.init(bitPattern:)), as: UTF8.self)
}

/// A raw listener that records what the dispatcher delivers and on which
/// thread. Tests that install one restore CoreEventStream's relay afterwards.
final class RecordingListener: MeguriCoreBindings.CoreEventListener {
    struct Delivery: Sendable {
        let seq: UInt64
        let thread: String
    }

    let deliveries = Mutex<[Delivery]>([])
    let reentryErrors = Mutex<[BridgeError]>([])
    let callsBackIntoTheCore: Bool

    init(callsBackIntoTheCore: Bool = false) {
        self.callsBackIntoTheCore = callsBackIntoTheCore
    }

    func onEvent(event: MeguriCoreBindings.CoreEvent) {
        if callsBackIntoTheCore {
            // Forbidden (KTD9): a synchronous call into the core from the callback.
            do {
                _ = try coreVersion()
            } catch {
                reentryErrors.withLock { $0.append(BridgeError(error)) }
            }
        }
        let delivery = Delivery(seq: event.seq, thread: currentThreadName())
        deliveries.withLock { $0.append(delivery) }
    }

    func waitForDeliveries(_ count: Int, timeout: Duration = .seconds(20)) async throws -> [Delivery] {
        let deadline = ContinuousClock.now + timeout
        while ContinuousClock.now < deadline {
            let got = deliveries.withLock { $0 }
            if got.count >= count { return got }
            try await Task.sleep(for: .milliseconds(5))
        }
        return deliveries.withLock { $0 }
    }
}

/// Installs `listener` in place of CoreEventStream's relay for `body`, then
/// puts the relay back.
func withRawListener<T>(_ listener: RecordingListener, _ body: () async throws -> T) async throws -> T {
    try await ControlLane.shared.call("test.setEventListener") { try setEventListener(listener: listener) }
    do {
        let result = try await body()
        try await CoreEventStream.shared.install()
        return result
    } catch {
        try? await CoreEventStream.shared.install()
        throw error
    }
}

/// Everything here shares the process-wide event listener, so it runs serially.
@Suite(.serialized) struct EventTests {
    @Test func tenThousandEventsFromFourProducersArriveInSeqOrderOnTheDispatcherThread() async throws {
        let listener = RecordingListener()
        let deliveries = try await withRawListener(listener) {
            try await MaintenanceLane.shared.call("test.emitEvents") {
                try testEmitEvents(producers: 4, perProducer: 2_500)
            }
            return try await listener.waitForDeliveries(10_000)
        }

        #expect(deliveries.count == 10_000)
        let seqs = deliveries.map(\.seq)
        #expect(zip(seqs, seqs.dropFirst()).allSatisfy { $0 < $1 }, "seq is not strictly increasing")
        let threads = Set(deliveries.map(\.thread))
        #expect(threads == ["meguri-events"])
        #expect(!threads.contains { $0.hasPrefix("meguri-test-producer") })
    }

    @Test func aCallbackThatCallsBackIntoTheCoreTripsTheDebugAssertion() async throws {
        let listener = RecordingListener(callsBackIntoTheCore: true)
        _ = try await withRawListener(listener) {
            try await MaintenanceLane.shared.call("test.emitEvents") { try testEmitEvents(producers: 1, perProducer: 1) }
            return try await listener.waitForDeliveries(1)
        }

        let errors = listener.reentryErrors.withLock { $0 }
        #expect(errors.count == 1)
        guard case .internal(let message)? = errors.first else {
            Issue.record("expected an internal error, got \(errors)")
            return
        }
        #expect(message.contains("core_version"))
        #expect(message.contains("called synchronously from the Swift callback"))
    }

    @Test func coreEventStreamYieldsEventsInSeqOrder() async throws {
        try await CoreEventStream.shared.install()
        let events = CoreEventStream.shared.events
        try await MaintenanceLane.shared.call("test.emitEvents") { try testEmitEvents(producers: 4, perProducer: 50) }

        var seqs: [UInt64] = []
        for await event in events {
            // Test producers use ids from 1,000,000 up; real jobs stay below.
            guard case .jobFinished(let id, let outcome) = event.kind, id.rawValue >= 1_000_000 else { continue }
            #expect(outcome == .succeeded)
            seqs.append(event.seq)
            if seqs.count == 200 { break }
        }
        #expect(zip(seqs, seqs.dropFirst()).allSatisfy { $0 < $1 })
    }

    @Test func progressFiredAThousandTimesASecondReachesSwiftAtMostElevenTimesInAnySecond() async throws {
        // 1 ms sleeps overshoot, so ask for 2,000 a second and check below
        // that Rust really fired at least 1,000.
        let job = try await Jobs.start("test.startJob") {
            try testStartJob(durationMs: 2_500, reportsPerSecond: 2_000, failWith: nil)
        }
        let clock = ContinuousClock()
        let start = clock.now
        var arrivals: [Duration] = []
        var total: UInt64 = 0
        for await update in job.progress {
            arrivals.append(clock.now - start)
            total = update.total ?? 0
        }
        #expect(await job.outcome() == .succeeded)
        let elapsed = clock.now - start
        let seconds = Double(elapsed.components.seconds) + Double(elapsed.components.attoseconds) / 1e18

        let firedPerSecond = Double(total) / seconds
        #expect(firedPerSecond >= 1_000, "Rust fired only \(Int(firedPerSecond)) reports a second")
        for first in arrivals {
            let window = arrivals.filter { $0 >= first && $0 <= first + .seconds(1) }
            #expect(window.count <= 11, "\(window.count) updates in the second after \(first)")
        }
        // About ten updates a second; the throttle must not starve the UI.
        #expect(Double(arrivals.count) >= seconds * 5, "only \(arrivals.count) updates in \(elapsed)")
    }

    @Test func aJobOutcomeArrivesThroughItsJobFinishedEvent() async throws {
        let failing = try await Jobs.start("test.startJob") {
            try testStartJob(durationMs: 20, reportsPerSecond: 100, failWith: .Busy(runningOp: "Check Database"))
        }
        #expect(await failing.outcome() == .failed(.busy(runningOperation: "Check Database")))
        await #expect(throws: BridgeError.busy(runningOperation: "Check Database")) { try await failing.value() }

        let instant = try await Jobs.start("test.startJob") {
            try testStartJob(durationMs: 0, reportsPerSecond: 1, failWith: nil)
        }
        #expect(await instant.outcome() == .succeeded)
    }

    @Test func cancellingAJobEndsItAsCancelled() async throws {
        let job = try await Jobs.start("test.startJob") {
            try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
        }
        try await Task.sleep(for: .milliseconds(50))
        let clock = ContinuousClock()
        let cancelledAt = clock.now
        try await job.cancel()
        #expect(await job.outcome() == .cancelled)
        #expect(clock.now - cancelledAt < .seconds(1))
    }
}
