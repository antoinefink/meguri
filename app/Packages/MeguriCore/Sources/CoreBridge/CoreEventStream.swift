internal import MeguriCoreBindings
import Instrumentation
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
        /// An op committed these changes; the event's `seq` is the op's.
        case changes(ChangeSet)
        /// The collection file was replaced and `generation` is open now.
        /// Exactly once per replacement.
        case collectionReplaced(generation: UInt64)
        case jobFinished(JobID, JobOutcome)
        /// The scheduler day changed, either way, after the core's rollover
        /// op committed: queues and counts of the old day are stale.
        case rolledOver(oldDay: UInt32, newDay: UInt32)
        /// The device's time zone differs from the one the day clock last
        /// saw, even when the day did not change.
        case timeZoneChanged(zoneID: String)
        /// The next day boundary moved, in epoch seconds: re-arm the day
        /// clock's timer.
        case nextDayAtChanged(nextDayAt: Int64)
    }

    init(_ event: MeguriCoreBindings.CoreEvent) {
        seq = event.seq
        generation = event.generation
        causeSeq = event.causeSeq
        switch event.kind {
        case .changes(let changes):
            kind = .changes(ChangeSet(changes))
        case .collectionReplaced(let generation):
            kind = .collectionReplaced(generation: generation)
        case .jobFinished(let id, let outcome):
            kind = .jobFinished(JobID(rawValue: id), JobOutcome(outcome))
        case .rolledOver(let oldDay, let newDay):
            kind = .rolledOver(oldDay: oldDay, newDay: newDay)
        case .timeZoneChanged(let zoneId):
            kind = .timeZoneChanged(zoneID: zoneId)
        case .nextDayAtChanged(let nextDayAt):
            kind = .nextDayAtChanged(nextDayAt: nextDayAt)
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
        try await ControlLane.shared.call(.setEventListener) { try setEventListener(listener: relay) }
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
