import Dispatch
import Foundation
import MeguriCoreBindings
import Synchronization
import Testing

@testable import CoreBridge

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

