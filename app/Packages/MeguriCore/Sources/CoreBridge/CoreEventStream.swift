internal import MeguriCoreBindings
import Synchronization

/// One event from the core (KTD9), in `seq` order.
public struct CoreEvent: Sendable, Hashable {
    /// Strictly increasing for the process's lifetime.
    public let seq: UInt64
    /// The collection generation; 0 before a collection is open.
    public let generation: UInt64
    /// For derived events, the `seq` of the change that caused them.
    public let causeSeq: UInt64?
    public let kind: Kind

    public enum Kind: Sendable, Hashable {
        case jobFinished(JobID, JobOutcome)
    }

    init(_ event: MeguriCoreBindings.CoreEvent) {
        seq = event.seq
        generation = event.generation
        causeSeq = event.causeSeq
        switch event.kind {
        case .jobFinished(let id, let outcome):
            kind = .jobFinished(JobID(rawValue: id), JobOutcome(outcome))
        }
    }
}

/// The core event channel on the Swift side. The core's single dispatcher
/// thread calls the relay, which only yields into an unbounded stream; one
/// task then resolves job outcomes and republishes every event, in order, on
/// `events`.
public final class CoreEventStream: Sendable {
    public static let shared = CoreEventStream()

    /// Every core event in `seq` order. Single consumer: the app's change
    /// fan-out (U151). Events wait in the stream until it is read.
    public let events: AsyncStream<CoreEvent>

    private let relay: EventRelay
    private let started = Mutex(false)

    private init() {
        let (raw, rawContinuation) = AsyncStream.makeStream(
            of: MeguriCoreBindings.CoreEvent.self, bufferingPolicy: .unbounded)
        let (events, output) = AsyncStream.makeStream(of: CoreEvent.self, bufferingPolicy: .unbounded)
        self.events = events
        relay = EventRelay(rawContinuation)
        Task.detached(name: "CoreEventStream router") {
            for await raw in raw {
                let event = CoreEvent(raw)
                if case .jobFinished(let id, let outcome) = event.kind {
                    JobRegistry.shared.finish(id, outcome)
                }
                output.yield(event)
            }
        }
    }

    /// Registers the relay with the core once (control lane). Call it at
    /// launch; `Jobs.start` calls it too.
    public func start() async throws(BridgeError) {
        guard !started.withLock({ $0 }) else { return }
        try await install()
        started.withLock { $0 = true }
    }

    /// Registers the relay with the core, replacing any other listener.
    func install() async throws(BridgeError) {
        let relay = relay
        try await ControlLane.shared.call("set_event_listener") { try setEventListener(listener: relay) }
    }
}

/// The Swift implementation of the core's event callback. It runs on the
/// core's dispatcher thread and only yields (KTD9).
final class EventRelay: MeguriCoreBindings.CoreEventListener {
    private let continuation: AsyncStream<MeguriCoreBindings.CoreEvent>.Continuation

    init(_ continuation: AsyncStream<MeguriCoreBindings.CoreEvent>.Continuation) {
        self.continuation = continuation
    }

    func onEvent(event: MeguriCoreBindings.CoreEvent) {
        continuation.yield(event)
    }
}
