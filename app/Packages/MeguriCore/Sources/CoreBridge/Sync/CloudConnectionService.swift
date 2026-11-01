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
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionBeginConnectionPreview) { [raw] in CloudConnectionPreviewWork(try raw.beginConnectionPreview()) }
    }
    public func connectionActiveRead(_ preview: CloudConnectionPreview, read: CloudControlRead) async throws -> CloudConnectionPreviewWork {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionConnectionActiveRead) { [raw] in CloudConnectionPreviewWork(try raw.connectionActiveRead(preview: preview.raw, read: read.raw)) }
    }
    public func connectionEpochRead(_ preview: CloudConnectionPreview, read: CloudControlRead) async throws -> CloudConnectionPreviewWork {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionConnectionEpochRead) { [raw] in CloudConnectionPreviewWork(try raw.connectionEpochRead(preview: preview.raw, read: read.raw)) }
    }
    public func cancelPreview(_ preview: CloudConnectionPreview) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionCancelPreview) { [raw] in try raw.cancelPreview(preview: preview.raw) }
    }
    public func confirmPublication(_ preview: CloudConnectionPreview) async throws -> CloudRecoveryOperation {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionConfirmPublication) { [raw] in CloudRecoveryOperation(raw: try raw.confirmPublication(preview: preview.raw)) }
    }
    public func confirmAdoption(_ preview: CloudConnectionPreview) async throws -> CloudRecoveryOperation {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionConfirmAdoption) { [raw] in CloudRecoveryOperation(raw: try raw.confirmAdoption(preview: preview.raw)) }
    }
    /// Rust consumes and revalidates the retained preview, commits activation,
    /// removes the cursor and settles/reconciles before this returns.
    public func resumeBinding(_ preview: CloudConnectionPreview) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionResumeBinding) { [raw] in try raw.resumeBinding(preview: preview.raw) }
    }
    public func nextPublicationWork() async throws -> CloudPublicationWork {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionNextPublicationWork) { [raw] in CloudPublicationWork(try raw.nextPublicationWork()) }
    }
    public func performLocal(_ handle: CloudPublicationLocal) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionPerformLocal) { [raw] in try raw.performLocal(handle: handle.raw) }
    }
    /// All suspension happens before Rust revalidates the retained OwnRequest.
    /// The callback enqueues synchronously, and its original reply is awaited
    /// only after this call has returned from Rust.
    public func submitPublicationRequest(_ request: CloudPublicationRequest, queue: any CloudPublicationQueue) async throws -> Bool {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionSubmitPublicationRequest) { [raw] in try raw.submitPublicationRequest(request: request.raw, queue: ForeignPublicationQueue(queue)) }
    }
    public func publicationResult(_ request: CloudPublicationRequest, result: CloudPublicationResult) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionPublicationResult) { [raw] in try raw.publicationResult(request: request.raw, result: result.raw) }
    }
    public func adoptionResult(_ handle: CloudAdoptionWork, result: CloudAdoptionResult) async throws {
        let raw = self.raw
        try await result.withRaw { value in
            try await MaintenanceLane.shared.call(.cloudConnectionAdoptionResult) { [raw] in try raw.adoptionResult(handle: handle.raw, result: value) }
        }
    }
    public func performAdoptionLocal(_ handle: CloudAdoptionWork) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionPerformAdoptionLocal) { [raw] in try raw.performAdoptionLocal(handle: handle.raw) }
    }
    public func cancelOperation(_ operation: CloudRecoveryOperation) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionCancelOperation) { [raw] in try raw.cancelOperation(operation: operation.raw) }
    }
    public func trigger() async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionTrigger) { [raw] in try raw.trigger() }
    }
    public func recoveryStatus() async throws -> CloudRecoveryStatus? {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudConnectionRecoveryStatus) { [raw] in try raw.recoveryStatus().map(CloudRecoveryStatus.init) }
    }
}
