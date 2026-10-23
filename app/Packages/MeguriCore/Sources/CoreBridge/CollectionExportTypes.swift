import Foundation
import Synchronization
internal import MeguriCoreBindings

public struct CollectionExportMetadata: Sendable, Equatable {
    public let operationID: UInt64
    public let sourceGeneration: UInt64
    public let admissionSequence: UInt64
    public let snapshotModifiedMilliseconds: Int64
    public let byteLength: UInt64
    public let suggestedName: String
    public let sha256: [UInt8]
    init(_ raw: MeguriCoreBindings.CollectionExportMetadata) {
        operationID = raw.operationId; sourceGeneration = raw.sourceGeneration; admissionSequence = raw.admissionSeq
        snapshotModifiedMilliseconds = raw.snapshotModifiedMs; byteLength = raw.byteLength
        suggestedName = raw.suggestedName; sha256 = Array(raw.sha256)
    }
}
public struct CollectionExportFailure: Error, Sendable, Equatable {
    public let cause: BridgeError
    public let publicationUncertain: Bool
    public let destination: URL?
    init(cause: BridgeError, destination: URL) {
        self.cause = cause; publicationUncertain = false; self.destination = destination
    }
    init(_ raw: MeguriCoreBindings.CollectionExportFailure) {
        cause = BridgeError(raw.cause); publicationUncertain = raw.publicationUncertain
        destination = raw.destination.map { URL(fileURLWithPath: $0) }
    }
}
/// Immutable completed Rust file, independent of the live collection's reader.
public final class ExportedCollection: Sendable {
    let raw: MeguriCoreBindings.CollectionExportArtifact
    public let metadata: CollectionExportMetadata
    init(raw: MeguriCoreBindings.CollectionExportArtifact, metadata: CollectionExportMetadata) {
        self.raw = raw; self.metadata = metadata
    }
    public func discard() async throws(BridgeError) {
        try await CoreEventStream.shared.start()
        let cleanup = try await MaintenanceLane.shared.call(.collectionExportArtifactDiscard) { try self.raw.discard() }
        if let cleanup {
            let job = try await Jobs.trackDraining(cleanup)
            try await job.value()
        }
    }
    public func acquire() async throws(BridgeError) -> CollectionExportReadLease {
        try await CoreEventStream.shared.start()
        let started = try await MaintenanceLane.shared.call(.collectionExportArtifactAcquire) { try self.raw.acquire() }
        do {
            let job = try await Jobs.trackDraining(started.job)
            try await job.value()
            let path = try await MaintenanceLane.shared.call(.collectionExportLeasePath) { try started.lease.path() }
            return CollectionExportReadLease(raw: started.lease, url: URL(fileURLWithPath: path))
        } catch {
            // No URL escaped. Drain first, then record that native never owned it.
            if let cleanup = try? await MaintenanceLane.shared.call(.collectionExportLeaseFinish, { try started.lease.finish() }),
               let job = try? await Jobs.trackDraining(cleanup) { _ = await job.outcome() }
            throw error
        }
    }
}
/// Keep this object until native provider completion, regardless of sheet lifetime.
public final class CollectionExportReadLease: Sendable {
    private let raw: MeguriCoreBindings.CollectionExportLease
    public let url: URL
    init(raw: MeguriCoreBindings.CollectionExportLease, url: URL) { self.raw = raw; self.url = url }
    /// Only actual native cancellation/copy completion permits restart recovery.
    public func finish() async throws(BridgeError) {
        let cleanup = try await MaintenanceLane.shared.call(.collectionExportLeaseFinish) { try self.raw.finish() }
        let job = try await Jobs.trackDraining(cleanup)
        try await job.value()
    }
}
public final class CollectionExportOperation: Sendable {
    private struct State { var cancelled = false; var job: CoreJob? }
    private let state = Mutex(State())
    public init() {}
    public func cancel() async {
        let job = state.withLock { s in s.cancelled = true; return s.job }
        try? await job?.cancel()
    }
    func attach(_ job: CoreJob) async {
        let cancelled = state.withLock { s in s.job = job; return s.cancelled }
        if cancelled { try? await job.cancel() }
    }
    func check() throws(BridgeError) { if state.withLock({ $0.cancelled }) { throw .cancelled } }
}
