import CoreBridge

@MainActor
protocol ChangeFanoutSink: AnyObject {
    func receive(_ event: CoreEvent)
}

/// U151's process-lifetime consumer. Collection owners detach without ending
/// CoreBridge's single output stream; each sink keeps its existing filtering
/// and refresh coalescing after synchronous, ordered delivery.
@MainActor
final class ChangeFanout {
    static let shared = ChangeFanout()

    private final class Sink {
        weak var value: (any ChangeFanoutSink)?
        init(_ value: any ChangeFanoutSink) { self.value = value }
    }
    private var sinks: [ObjectIdentifier: Sink] = [:]
    private var consumer: Task<Void, Never>?
    private var startup: Task<Void, any Error>?

    private init() {}

    func subscribe(_ sink: any ChangeFanoutSink) async throws {
        // Register before starting either task: even a pre-listener backlog
        // must reach the first owner. MainActor serializes all registrations.
        sinks[ObjectIdentifier(sink)] = Sink(sink)
        if consumer == nil {
            consumer = Task { [weak self] in
                for await event in CoreEventStream.shared.events {
                    self?.deliver(event)
                }
            }
        }
        let task: Task<Void, any Error>
        if let startup { task = startup }
        else {
            task = Task { [weak self] in
                do { try await CoreEventStream.shared.start() }
                catch { self?.startup = nil; throw error }
            }
            startup = task
        }
        do { try await task.value }
        catch { unsubscribe(sink); throw error }
    }

    func unsubscribe(_ sink: any ChangeFanoutSink) {
        sinks.removeValue(forKey: ObjectIdentifier(sink))
    }

    // Read-only ownership observation for real-owner regressions.
    func isRegistered(_ sink: any ChangeFanoutSink) -> Bool {
        sinks[ObjectIdentifier(sink)]?.value != nil
    }

    private func deliver(_ event: CoreEvent) {
        // No await, buffering or merging here: Session, CommitFailed and
        // CollectionReplaced retain their exact positions and envelope fields.
        for id in Array(sinks.keys) {
            if let sink = sinks[id]?.value { sink.receive(event) }
            else { sinks.removeValue(forKey: id) }
        }
    }
}
