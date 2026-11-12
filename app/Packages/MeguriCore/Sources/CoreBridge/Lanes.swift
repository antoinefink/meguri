import Dispatch
import Foundation
import Instrumentation
#if DEBUG
import Observation
import Synchronization

/// Observational only: the current registered call on the one shared
/// maintenance lane, never proof that it caused a particular wait. UI data
/// must never enter this state; it retains only a call name, without history.
public final class MaintenanceCallObservation: Observable, Sendable {
    public static let shared = MaintenanceCallObservation()
    // Passive capture at actual lane entry; contains no arguments or results.
    @TaskLocal public static var capture: (@Sendable (BridgeCall) -> Void)?
    private let changes = ObservationRegistrar()
    private let current = Mutex<BridgeCall?>(nil)
    private init() {}

    public var currentCall: BridgeCall? {
        changes.access(self, keyPath: \.currentCall)
        return current.withLock { $0 }
    }

    func withCall<T>(_ name: BridgeCall, _ body: () throws -> T) rethrows -> T {
        dispatchPrecondition(condition: .onQueue(MaintenanceLane.shared.queue))
        let previous = replace(with: name)
        Self.capture?(name)
        defer { _ = replace(with: previous) }
        return try body()
    }

    private func replace(with call: BridgeCall?) -> BridgeCall? {
        // Synchronous observers may read currentCall. Notify outside the
        // mutex; only the owner swap is locked, never the body or FFI.
        changes.withMutation(of: self, keyPath: \.currentCall) {
            current.withLock {
                let previous = $0
                $0 = call
                return previous
            }
        }
    }
}
#endif

/// The four core lanes (KTD9). Each is an actor whose serial executor is its
/// own `DispatchSerialQueue`, so blocking FFI calls run there and never on the
/// main actor or the cooperative pool. Lanes carry only short calls; long work
/// runs as core jobs (`Jobs`).
public enum Lane: String, Sendable, CaseIterable {
    /// Review, render, undo, edits.
    case interactive
    /// Reads, pages, stats, `qa_cells` with cancel.
    case query
    /// Job start and control, listings, config.
    case maintenance
    /// Lifecycle, background flush/suspend/resume, widget flush, cancels.
    case control

    /// The label of the lane's dispatch queue.
    public var queueLabel: String { "com.antoinefink.meguri.core.\(rawValue)" }

    var qos: DispatchQoS {
        switch self {
        case .interactive: .userInteractive
        case .query, .maintenance, .control: .userInitiated
        }
    }
}

/// A lane actor. Area services either call `call` on a lane, or are actors
/// themselves that share a lane's executor.
public protocol CoreLane: Actor {
    static var lane: Lane { get }
    static var shared: Self { get }
    nonisolated var queue: DispatchSerialQueue { get }
}

extension CoreLane {
    /// Runs one blocking core call on this lane, inside its os_signpost
    /// interval in the CoreBridge category (R1861), and maps whatever it
    /// throws to `BridgeError`. `name` comes from U12's registry
    /// (`BridgeCall`, Instrumentation/IntervalNames.swift).
    func call<T: Sendable>(_ name: BridgeCall, _ body: () throws -> T) throws(BridgeError) -> T {
        dispatchPrecondition(condition: .onQueue(queue))
        do {
            #if DEBUG
            if self === MaintenanceLane.shared {
                return try MaintenanceCallObservation.shared.withCall(name) {
                    try Signposts.bridge(name, lane: Self.lane.rawValue, body)
                }
            }
            #endif
            return try Signposts.bridge(name, lane: Self.lane.rawValue, body)
        } catch {
            throw BridgeError(error)
        }
    }
}

@globalActor public actor InteractiveLane: CoreLane {
    public static let lane = Lane.interactive
    public static let shared = InteractiveLane()
    public nonisolated let queue = DispatchSerialQueue(label: Lane.interactive.queueLabel, qos: Lane.interactive.qos)
    public nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
}

@globalActor public actor QueryLane: CoreLane {
    public static let lane = Lane.query
    public static let shared = QueryLane()
    public nonisolated let queue = DispatchSerialQueue(label: Lane.query.queueLabel, qos: Lane.query.qos)
    public nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
}

@globalActor public actor MaintenanceLane: CoreLane {
    public static let lane = Lane.maintenance
    public static let shared = MaintenanceLane()
    public nonisolated let queue = DispatchSerialQueue(label: Lane.maintenance.queueLabel, qos: Lane.maintenance.qos)
    public nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
}

@globalActor public actor ControlLane: CoreLane {
    public static let lane = Lane.control
    public static let shared = ControlLane()
    public nonisolated let queue = DispatchSerialQueue(label: Lane.control.queueLabel, qos: Lane.control.qos)
    public nonisolated var unownedExecutor: UnownedSerialExecutor { queue.asUnownedSerialExecutor() }
}
