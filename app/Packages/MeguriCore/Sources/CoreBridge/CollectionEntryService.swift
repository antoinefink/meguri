import Foundation
internal import MeguriCoreBindings

public protocol CollectionEntryService: Sendable {
    func inspect(source: URL, location: CollectionLocation, operation: CollectionEntryOperation,
                 progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> InspectedCollection
    func activate(_ inspection: InspectedCollection, operation: CollectionEntryOperation,
                  progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> CollectionActivation
}

public final class CoreCollectionEntryService: CollectionEntryService, Sendable {
    private let clock: CollectionClockRelay
    private let diskSpace: DiskSpaceRelay
    private let source: CoordinatedSourceOwner
    public init(clock: any CollectionClock = SystemCollectionClock(), diskSpace: any DiskSpaceProviding = DiskSpaceProvider(),
                source: CoordinatedSourceOwner = CoordinatedSourceOwner()) {
        self.clock = CollectionClockRelay(clock); self.diskSpace = DiskSpaceRelay(diskSpace); self.source = source
    }
    public func inspect(source url: URL, location: CollectionLocation, operation: CollectionEntryOperation,
                        progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> InspectedCollection {
        // Subscribe before dispatching coordination, including a no-file host.
        try await CoreEventStream.shared.start()
        return try await withTaskCancellationHandler {
            try await source.withReadablePackage(at: url) { [self] coordinatedURL in
                try operation.checkCancellation()
                let started = try await MaintenanceLane.shared.call(.startCollectionInspection) {
                    try startCollectionInspection(sourcePath: coordinatedURL.path, location: location.raw,
                        clock: self.clock, diskSpace: self.diskSpace, randomSeed: UInt64.random(in: 0...UInt64.max))
                }
                var trackingFailure: BridgeError?
                do throws(BridgeError) {
                    let job = try await Jobs.trackDraining(started.job)
                    await operation.attach(job)
                    let updates = Task { for await update in job.progress { operation.update(update); progress(update) } }
                    _ = await job.outcome()
                    await updates.value
                } catch { trackingFailure = error }
                // trackDraining also waits for terminal delivery on setup error.
                let outcome = try await MaintenanceLane.shared.call(.collectionInspectResultOutcome) { try started.result.outcome() }
                let inspected: MeguriCoreBindings.CollectionInspection
                switch outcome {
                case .ready(let raw): inspected = raw
                case .failed(let failure): throw CollectionEntryFailure(failure)
                case .pending: throw BridgeError.internal("Inspection result pending after terminal event")
                }
                do {
                    if let trackingFailure { throw trackingFailure }
                    let facts = try await MaintenanceLane.shared.call(.collectionInspectionFacts) { try inspected.facts() }
                    let preview = InspectedCollection(raw: inspected, facts: CollectionInspectionFacts(facts))
                    try operation.checkCancellation()
                    return preview
                } catch {
                    try? await MaintenanceLane.shared.call(.collectionInspectionDiscard) { try inspected.discard() }
                    throw error
                }
            }
        } onCancel: { Task { await operation.cancel() } }
    }
    public func activate(_ inspection: InspectedCollection, operation: CollectionEntryOperation,
                         progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> CollectionActivation {
        try await CoreEventStream.shared.start()
        try operation.checkCancellation()
        return try await withTaskCancellationHandler {
            let started = try await MaintenanceLane.shared.call(.startCollectionActivation) {
                try startCollectionActivation(inspection: inspection.raw)
            }
            var trackingFailure: BridgeError?
            do throws(BridgeError) {
                let job = try await Jobs.trackDraining(started.job)
                await operation.attach(job)
                let updates = Task { for await update in job.progress { operation.update(update); progress(update) } }
                _ = await job.outcome()
                await updates.value
            } catch { trackingFailure = error }
            let outcome = try await MaintenanceLane.shared.call(.collectionActivationResultOutcome) { try started.result.outcome() }
            switch outcome {
            case .activated(let result):
                let activation = CollectionActivation(result)
                if let trackingFailure {
                    throw CollectionEntryFailure(issue: .core(trackingFailure), candidateActive: true,
                        generation: activation.generation, backup: activation.backup)
                }
                return activation
            case .failed(let failure): throw CollectionEntryFailure(failure)
            case .pending: throw BridgeError.internal("Activation result pending after terminal event")
            }
        } onCancel: { Task { await operation.cancel() } }
    }
}
