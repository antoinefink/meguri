import Foundation
import Instrumentation
internal import MeguriCoreBindings

/// The only synchronous foreign action permitted by this port is acknowledged
/// queue admission. Implementations add an already built operation immediately.
public protocol CloudPublicationQueue: Sendable { func enqueue() -> Bool }
private final class ForeignPublicationQueue: MeguriCoreBindings.CloudPublicationQueue {
    let queue: any CloudPublicationQueue
    init(_ queue: any CloudPublicationQueue) { self.queue = queue }
    func enqueue() -> Bool { queue.enqueue() }
}

public actor CoreCloudConnectionService {
    private let raw: MeguriCoreBindings.CloudConnection
    init(_ raw: MeguriCoreBindings.CloudConnection) { self.raw = raw }
    public func beginConnectionPreview() async throws -> CloudConnectionPreviewWork {
        try await MaintenanceLane.shared.call(.cloudConnectionBeginConnectionPreview) { CloudConnectionPreviewWork(try raw.beginConnectionPreview()) }
    }
    public func connectionActiveRead(_ preview: CloudConnectionPreview, read: CloudControlRead) async throws -> CloudConnectionPreviewWork {
        try await MaintenanceLane.shared.call(.cloudConnectionConnectionActiveRead) { CloudConnectionPreviewWork(try raw.connectionActiveRead(preview: preview.raw, read: read.raw)) }
    }
    public func connectionEpochRead(_ preview: CloudConnectionPreview, read: CloudControlRead) async throws -> CloudConnectionPreviewWork {
        try await MaintenanceLane.shared.call(.cloudConnectionConnectionEpochRead) { CloudConnectionPreviewWork(try raw.connectionEpochRead(preview: preview.raw, read: read.raw)) }
    }
    public func cancelPreview(_ preview: CloudConnectionPreview) async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionCancelPreview) { try raw.cancelPreview(preview: preview.raw) }
    }
    public func confirmPublication(_ preview: CloudConnectionPreview) async throws -> CloudRecoveryOperation {
        try await MaintenanceLane.shared.call(.cloudConnectionConfirmPublication) { CloudRecoveryOperation(raw: try raw.confirmPublication(preview: preview.raw)) }
    }
    public func confirmAdoption(_ preview: CloudConnectionPreview) async throws -> CloudRecoveryOperation {
        try await MaintenanceLane.shared.call(.cloudConnectionConfirmAdoption) { CloudRecoveryOperation(raw: try raw.confirmAdoption(preview: preview.raw)) }
    }
    public func nextPublicationWork() async throws -> CloudPublicationWork {
        try await MaintenanceLane.shared.call(.cloudConnectionNextPublicationWork) { CloudPublicationWork(try raw.nextPublicationWork()) }
    }
    public func performLocal(_ handle: CloudPublicationLocal) async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionPerformLocal) { try raw.performLocal(handle: handle.raw) }
    }
    /// All suspension happens before Rust revalidates the retained OwnRequest.
    /// The callback enqueues synchronously, and its original reply is awaited
    /// only after this call has returned from Rust.
    public func submitPublicationRequest(_ request: CloudPublicationRequest, queue: any CloudPublicationQueue) async throws -> Bool {
        try await MaintenanceLane.shared.call(.cloudConnectionSubmitPublicationRequest) { try raw.submitPublicationRequest(request: request.raw, queue: ForeignPublicationQueue(queue)) }
    }
    public func publicationResult(_ request: CloudPublicationRequest, result: CloudPublicationResult) async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionPublicationResult) { try raw.publicationResult(request: request.raw, result: result.raw) }
    }
    public func adoptionResult(_ handle: CloudAdoptionWork, result: CloudAdoptionResult) async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionAdoptionResult) { try raw.adoptionResult(handle: handle.raw, result: result.raw) }
    }
    public func performAdoptionLocal(_ handle: CloudAdoptionWork) async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionPerformAdoptionLocal) { try raw.performAdoptionLocal(handle: handle.raw) }
    }
    public func cancelOperation(_ operation: CloudRecoveryOperation) async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionCancelOperation) { try raw.cancelOperation(operation: operation.raw) }
    }
    public func trigger() async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionTrigger) { try raw.trigger() }
    }
    public func recoveryStatus() async throws -> CloudRecoveryStatus? {
        try await MaintenanceLane.shared.call(.cloudConnectionRecoveryStatus) { try raw.recoveryStatus().map(CloudRecoveryStatus.init) }
    }
}
