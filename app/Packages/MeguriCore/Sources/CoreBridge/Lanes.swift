import Dispatch
import Foundation
import Instrumentation

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
