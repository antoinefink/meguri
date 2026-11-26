import Dispatch
import Foundation
import Synchronization
internal import MeguriCoreBindings

public protocol CollectionExportService: Sendable {
    func prepare(deck: DeckExportRequest?, operation: CollectionExportOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ExportedCollection
    func save(_ artifact: ExportedCollection, to: URL, operation: CollectionExportOperation) async throws
}
extension CollectionExportService {
    public func prepare(operation: CollectionExportOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ExportedCollection {
        try await prepare(deck: nil, operation: operation, progress: progress)
    }
}
public final class CoreCollectionExportService: CollectionExportService, Sendable {
    private let destination: CoordinatedExportDestination
    private let publisher = StagedCollectionExportDestination()
    public init(destination: CoordinatedExportDestination = CoordinatedExportDestination()) { self.destination = destination }
    public func prepare(deck: DeckExportRequest?, operation: CollectionExportOperation, progress: @escaping @Sendable (CoreProgress) -> Void) async throws -> ExportedCollection {
        try await CoreEventStream.shared.start()
        try operation.check()
        return try await withTaskCancellationHandler {
            let started: MeguriCoreBindings.CollectionExportJob
            if let deck {
                started = try await MaintenanceLane.shared.call(.startDeckExport) {
                    try startDeckExport(deckId: deck.deckID, options: deck.options.raw)
                }
            } else {
                started = try await MaintenanceLane.shared.call(.startCollectionExport) { try startCollectionExport() }
            }
            var setupFailure: BridgeError?
            do throws(BridgeError) {
                let job = try await Jobs.trackDraining(started.job)
                await operation.attach(job)
                let updates = Task { for await value in job.progress { progress(value) } }
                _ = await job.outcome(); await updates.value
            } catch { setupFailure = error }
            let outcome = try await MaintenanceLane.shared.call(.collectionExportResultOutcome) { try started.result.outcome() }
            switch outcome {
            case .ready(let raw):
                do {
                    if let setupFailure { throw setupFailure }
                    try operation.check()
                    let metadata = try await MaintenanceLane.shared.call(.collectionExportArtifactMetadata) { try raw.metadata() }
                    return ExportedCollection(raw: raw, metadata: CollectionExportMetadata(metadata))
                } catch {
                    // trackDraining has already waited for the real worker.
                    if let cleanup = try? await MaintenanceLane.shared.call(.collectionExportArtifactDiscard, { try raw.discard() }) {
                        if let job = try? await Jobs.trackDraining(cleanup) { _ = await job.outcome() }
                    }
                    throw error
                }
            case .failed(let failure): throw CollectionExportFailure(failure)
            case .pending: throw BridgeError.internal("Export pending after terminal delivery")
            }
        } onCancel: { Task { await operation.cancel() } }
    }
    public func save(_ artifact: ExportedCollection, to url: URL, operation: CollectionExportOperation) async throws {
        try await CoreEventStream.shared.start()
        try operation.check()
        try await withTaskCancellationHandler {
            try await destination.withDestination(url) { coordinatedURL in
                try operation.check()
                try await self.publisher.save(to: coordinatedURL, operation: operation,
                                              deckPackage: artifact.metadata.suggestedName.hasSuffix(".apkg")) { stagedURL in
                    let started = try await MaintenanceLane.shared.call(.startCollectionExportSave) {
                        try startCollectionExportSave(artifact: artifact.raw, destination: stagedURL.path,
                                                      finalValidationDestination: coordinatedURL.path)
                    }
                    do throws(BridgeError) {
                        let job = try await Jobs.trackDraining(started.job)
                        await operation.attach(job)
                        _ = await job.outcome()
                    } catch {
                        // The owned result remains authoritative after setup failure.
                    }
                    switch try await MaintenanceLane.shared.call(.collectionExportSaveResultOutcome, { try started.result.outcome() }) {
                    case .saved: return // Only the staged copy is complete here.
                    case .failed(let failure):
                        // Rust never published the native destination. Even a failed
                        // stage-directory sync is a prepublication destination failure.
                        throw CollectionExportFailure(cause: BridgeError(failure.cause), destination: coordinatedURL)
                    case .pending: throw BridgeError.internal("Save pending after terminal delivery")
                    }
                }
            }
        } onCancel: { Task { await operation.cancel() } }
    }
}

public protocol CollectionExportDestinationAccess: Sendable {
    func write(_ url: URL, accessor: @escaping @Sendable (URL) -> Void) throws
}
public struct FoundationExportDestinationAccess: CollectionExportDestinationAccess {
    public init() {}
    public func write(_ url: URL, accessor: @escaping @Sendable (URL) -> Void) throws {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        var error: NSError?
        NSFileCoordinator().coordinate(writingItemAt: url, options: .forReplacing, error: &error, byAccessor: accessor)
        if let error { throw error }
    }
}
private final class ExportDestinationResult<Value: Sendable>: Sendable {
    let value = Mutex<Result<Value, any Error>?>(nil)
}
/// Synchronous coordination and scope stay alive while the bounded Rust job drains.
public final class CoordinatedExportDestination: Sendable {
    private let queue = DispatchQueue(label: "com.antoinefink.meguri.collection-destination", qos: .userInitiated)
    private let access: any CollectionExportDestinationAccess
    public init(access: any CollectionExportDestinationAccess = FoundationExportDestinationAccess()) { self.access = access }
    public func withDestination<Output: Sendable>(_ url: URL, consume: @escaping @Sendable (URL) async throws -> Output) async throws -> Output {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [access] in
                let result = ExportDestinationResult<Output>()
                do {
                    try access.write(url) { coordinated in
                        let finished = DispatchSemaphore(value: 0)
                        Task {
                            defer { finished.signal() }
                            do { let output = try await consume(coordinated); result.value.withLock { $0 = .success(output) } }
                            catch { result.value.withLock { $0 = .failure(error) } }
                        }
                        finished.wait()
                    }
                    continuation.resume(with: result.value.withLock { $0 } ?? .failure(BridgeError.unavailable(reason: "Destination accessor did not run")))
                } catch {
                    switch result.value.withLock({ $0 }) {
                    case .success?: continuation.resume(throwing: ExportDestinationCompletionUncertain(underlyingError: error))
                    case .failure(let ioError)?: continuation.resume(throwing: ioError)
                    case nil: continuation.resume(throwing: error)
                    }
                }
            }
        }
    }
}
public struct ExportDestinationCompletionUncertain: Error, Sendable {
    public let destination: URL?
    public let recoveryDirectory: URL?
    public let underlyingError: (any Error)?
    public init(destination: URL? = nil, recoveryDirectory: URL? = nil, underlyingError: (any Error)? = nil) {
        self.destination = destination; self.recoveryDirectory = recoveryDirectory; self.underlyingError = underlyingError
    }
}
