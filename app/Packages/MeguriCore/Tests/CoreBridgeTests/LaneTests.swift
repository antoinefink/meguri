import Dispatch
import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing

@testable import CoreBridge
#if DEBUG
import Observation

@Sendable private func exerciseMaintenanceCallObservation() {
    let observation = MaintenanceCallObservation.shared
    #expect(observation.currentCall == .coreVersion)
    observation.withCall(.startSession) {
        #expect(observation.currentCall == .startSession)
    }
    #expect(observation.currentCall == .coreVersion)
    do {
        try observation.withCall(.reviewSessionJobResult) {
            #expect(observation.currentCall == .reviewSessionJobResult)
            throw BridgeError.cancelled
        }
        Issue.record("Expected BridgeError.cancelled from the nested maintenance call")
    } catch BridgeError.cancelled {
    } catch {
        Issue.record("Expected BridgeError.cancelled from the nested maintenance call")
    }
    #expect(observation.currentCall == .coreVersion)
}

private extension MaintenanceLane {
    // A plain actor method orders this read after other calls without setting an owner.
    func expectMaintenanceCallObservationIdle() {
        #expect(MaintenanceCallObservation.shared.currentCall == nil)
    }
}

extension EventTests {
    @Test func maintenanceCallObservationTracksOnlyExecutingMaintenanceBody() async throws {
        let observation = MaintenanceCallObservation.shared
        let gate = MaintenanceOccupancy()
        let changed = Mutex(false)
        await MaintenanceLane.shared.expectMaintenanceCallObservationIdle()
        let occupying = Task { try await MaintenanceLane.shared.occupy(gate, call: .startSession) }
        defer { gate.release.signal() }
        for await _ in gate.entered { break }
        #expect(observation.currentCall == .startSession)
        withObservationTracking {
            _ = observation.currentCall
        } onChange: {
            // Synchronous read: no mutex may span this callback.
            _ = observation.currentCall
            changed.withLock { $0 = true }
        }
        let otherOwners = [
            try await InteractiveLane.shared.call(.coreVersion) { observation.currentCall },
            try await QueryLane.shared.call(.coreVersion) { observation.currentCall },
            try await ControlLane.shared.call(.coreVersion) { observation.currentCall },
        ]
        #expect(otherOwners == [.startSession, .startSession, .startSession])
        #expect(!changed.withLock { $0 }, "other lanes must not mutate the maintenance owner")
        #expect(!gate.watchdogFired.withLock { $0 })
        gate.release.signal()
        try await occupying.value
        #expect(changed.withLock { $0 }, "the nested observable getter must invalidate on release")
        #expect(!gate.watchdogFired.withLock { $0 })
        await MaintenanceLane.shared.expectMaintenanceCallObservationIdle()
        try await MaintenanceLane.shared.call(.coreVersion, exerciseMaintenanceCallObservation)
        await MaintenanceLane.shared.expectMaintenanceCallObservationIdle()
        do {
            try await MaintenanceLane.shared.call(.startSession) {
                #expect(observation.currentCall == .startSession)
                throw BridgeError.cancelled
            }
            Issue.record("Expected BridgeError.cancelled from the maintenance call")
        } catch BridgeError.cancelled {
        } catch {
            Issue.record("Expected BridgeError.cancelled from the maintenance call")
        }
        await MaintenanceLane.shared.expectMaintenanceCallObservationIdle()
    }
}
#endif

/// Where a lane call's body ran.
struct Probe: Sendable {
    let queueLabel: String
    let isMainThread: Bool
    let version: String
}

/// Runs in the lane call's body: records the current queue and thread, and
/// makes one real FFI call.
@Sendable func probe() throws -> Probe {
    Probe(
        queueLabel: String(cString: __dispatch_queue_get_label(nil)),
        isMainThread: Thread.isMainThread,
        version: try coreVersion()
    )
}

@Suite struct LaneTests {
    @Test @MainActor func aCallFromTheMainActorRunsOnTheInteractiveLaneQueue() async throws {
        #expect(pthread_main_np() == 1)  // the test body itself runs on the main thread
        let ran = try await InteractiveLane.shared.call("test.probe", probe)
        #expect(ran.queueLabel == Lane.interactive.queueLabel)
        #expect(!ran.isMainThread)
        #expect(ran.version == (try VersionTests.workspaceVersion()))
    }

    @Test func eachLaneRunsOnItsOwnSerialQueue() async throws {
        let labels = [
            try await InteractiveLane.shared.call("test.probe", probe).queueLabel,
            try await QueryLane.shared.call("test.probe", probe).queueLabel,
            try await MaintenanceLane.shared.call("test.probe", probe).queueLabel,
            try await ControlLane.shared.call("test.probe", probe).queueLabel,
        ]
        #expect(labels == Lane.allCases.map(\.queueLabel))
        #expect(Set(labels).count == 4)
    }

    @Test func aLaneRunsOneCallAtATime() async throws {
        let inFlight = Counter()
        try await withThrowingTaskGroup(of: Int.self) { group in
            for _ in 0..<8 {
                group.addTask {
                    try await QueryLane.shared.call("test.serial") {
                        let now = inFlight.add(1)
                        Thread.sleep(forTimeInterval: 0.005)
                        inFlight.add(-1)
                        return now
                    }
                }
            }
            for try await concurrent in group {
                #expect(concurrent == 1)
            }
        }
    }
}

/// A thread-safe counter for test bookkeeping.
final class Counter: Sendable {
    private let value = Mutex(0)

    @discardableResult func add(_ delta: Int) -> Int {
        value.withLock {
            $0 += delta
            return $0
        }
    }
}

