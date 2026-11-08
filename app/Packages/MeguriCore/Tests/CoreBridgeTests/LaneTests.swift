import Dispatch
import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing

@testable import CoreBridge
#if DEBUG
import Observation

extension EventTests {
    @Test func maintenanceCallObservationTracksOnlyExecutingMaintenanceBody() async throws {
        let observation = MaintenanceCallObservation.shared
        let gate = MaintenanceOccupancy()
        let changed = Mutex(false)
        let occupying = Task { @MaintenanceLane in
            #expect(observation.currentCall == nil)
            try MaintenanceLane.shared.call(.coreVersion) {
                #expect(observation.currentCall == .coreVersion)
                try MaintenanceLane.shared.occupy(gate, call: .startSession)
                #expect(observation.currentCall == .coreVersion)
                #expect(throws: BridgeError.cancelled) {
                    try MaintenanceLane.shared.call(.reviewSessionJobResult) {
                        #expect(observation.currentCall == .reviewSessionJobResult)
                        throw BridgeError.cancelled
                    }
                }
                #expect(observation.currentCall == .coreVersion)
            }
            #expect(observation.currentCall == nil)
            #expect(throws: BridgeError.cancelled) {
                try MaintenanceLane.shared.call(.startSession) {
                    #expect(observation.currentCall == .startSession)
                    throw BridgeError.cancelled
                }
            }
            #expect(observation.currentCall == nil)
        }
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
        #expect(observation.currentCall == nil)
        try cloudFinalCoreCall(.cloudWorkflowValidateSdk) {
            #expect(observation.currentCall == .cloudWorkflowValidateSdk)
        }
        #expect(observation.currentCall == nil)
        #expect(throws: BridgeError.cancelled) {
            try cloudFinalCoreCall(.cloudFetchReadValidate) {
                #expect(observation.currentCall == .cloudFetchReadValidate)
                throw BridgeError.cancelled
            }
        }
        #expect(observation.currentCall == nil)
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

