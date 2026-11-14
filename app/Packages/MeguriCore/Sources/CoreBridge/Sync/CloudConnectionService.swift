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

private struct CloudConnectionJobValue<Value: Sendable>: Sendable {
    let handle: MeguriCoreBindings.JobHandle
    let read: @Sendable () throws -> Value?
    let adoptionResult: MeguriCoreBindings.CloudAdoptionAppliedResult?
    #if DEBUG
    var debugAdoptionSession: UUID?
    #endif
    init(handle: MeguriCoreBindings.JobHandle,
         adoptionResult: MeguriCoreBindings.CloudAdoptionAppliedResult? = nil,
         read: @escaping @Sendable () throws -> Value?) {
        self.handle = handle; self.read = read; self.adoptionResult = adoptionResult
    }
}

public actor CoreCloudConnectionService {
    // Two moved calls on this retained connection shared Maintenance ordering.
    // Keep that order through terminal drain without sequencing policy jobs or
    // nesting a permit around the synchronous calls they may themselves use.
    private let order = CloudPolicyOrder()
    private let onJobQueued: @Sendable () -> Void
    private let raw: MeguriCoreBindings.CloudConnection
    private let track: @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob
    #if DEBUG
    nonisolated let debugAdoptionSession = DebugCloudPreviewProbe.AdoptionSession()

    /// Only successful confirmation or validated recovery attachment starts a
    /// diagnostic session. Ordinary work/status reads must never reset it.
    public nonisolated func debugResetAdoptionProgress() {
        debugAdoptionSession.reset(in: .ownWork)
    }
    #endif
    init(_ raw: MeguriCoreBindings.CloudConnection,
         onJobQueued: @escaping @Sendable () -> Void = {},
         track: @escaping @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob = { try Jobs.track($0) }) {
        self.raw = raw; self.track = track; self.onJobQueued = onJobQueued
    }

    private func run<Value: Sendable>(
        _ startName: BridgeCall, result resultName: BridgeCall,
        _ start: @escaping @Sendable () throws -> CloudConnectionJobValue<Value>
    ) async throws -> Value {
        await order.acquire(queued: onJobQueued)
        do {
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(startName, start)
            let job: CoreJob
            do {
                job = try await Jobs.trackDraining(started.handle, setup: track)
            } catch {
                // trackDraining has already drained on listener setup failure.
                #if DEBUG
                await recordAdoptionFailure(started.adoptionResult, session: started.debugAdoptionSession)
                #endif
                throw error
            }
            let outcome = await job.outcome()
            #if DEBUG
            await recordAdoptionFailure(started.adoptionResult, session: started.debugAdoptionSession)
            #endif
            // The permit and payload leases survive setup failure and terminal
            // drain. Cancellation of a waiting Task cannot drop a predecessor.
            if let value = try await MaintenanceLane.shared.call(resultName, started.read) {
                await order.release()
                return value
            }
            switch outcome {
            case .cancelled: throw BridgeError.cancelled
            case .failed(let error): throw error
            case .succeeded: throw BridgeError.internal("Connection job completed without its typed result")
            }
        } catch {
            await order.release()
            throw error
        }
    }

    #if DEBUG
    private func recordAdoptionFailure(_ result: MeguriCoreBindings.CloudAdoptionAppliedResult?, session: UUID?) async {
        guard let result else { return }
        // Best effort: a diagnostic read must never replace the original error.
        if let diagnostic = try? await MaintenanceLane.shared.call(.cloudAdoptionFailureDiagnostic, {
            try result.failureDiagnostic().map(CloudAdoptionFailureDiagnostic.init)
        }) {
            DebugCloudPreviewProbe.ownWork.recordAdoptionFailure(diagnostic, session: session)
            DebugCloudDiagnostics.recordAdoptionFailure(diagnostic)
        }
    }
    #endif

    public func localPublicationInformation(generation: UInt64) async throws(BridgeError) -> LocalPublicationInformation {
        try await MaintenanceLane.shared.call(.cloudConnectionLocalPublicationInformation) { [raw] in
            LocalPublicationInformation(try raw.localPublicationInformation(generation: generation))
        }
    }
    public func keepLocalOnly(generation: UInt64, preview: CloudConnectionPreview?) async throws {
        try await MaintenanceLane.shared.call(.cloudConnectionKeepLocalOnly) { [raw] in
            try raw.keepLocalOnly(generation: generation, preview: preview?.raw)
        }
    }
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
        #if DEBUG
        let session = debugAdoptionSession
        #endif
        return try await MaintenanceLane.shared.call(.cloudConnectionConfirmAdoption) { [raw] in
            let operation = CloudRecoveryOperation(raw: try raw.confirmAdoption(preview: preview.raw))
            #if DEBUG
            session.reset(in: .ownWork)
            #endif
            return operation
        }
    }
    /// Rust consumes and revalidates the retained preview, commits activation,
    /// removes the cursor and settles/reconciles before this returns.
    public func resumeBinding(_ preview: CloudConnectionPreview) async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudConnectionResumeBinding) { [raw] in try raw.resumeBinding(preview: preview.raw) }
    }
    /// Resume starts after the caller joins earlier commands and result ACKs.
    /// Issue the proof yield once the retained Resume invocation returns.
    public func resumeBindingForAccountProof(_ preview: CloudConnectionPreview) async throws -> CloudOriginalCommandProofYield {
        try await resumeBinding(preview)
        return CloudOriginalCommandProofYield()
    }
    public func nextPublicationWork() async throws -> CloudPublicationWork {
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPublicationWork = try await run(.cloudConnectionStartNextPublicationWork, result: .cloudPublicationWorkResult) { [raw] in
            let started = try raw.startNextPublicationWork()
            return CloudConnectionJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPublicationWork(value)
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
        #if DEBUG
        // Bind this exact result job before lease preparation or order awaits.
        let adoptionSession = debugAdoptionSession.capture()
        #endif
        try await result.withRaw { value in
            // Keep every native payload/staging lease through admission,
            // listener setup (including failure drain), terminal and typed read.
            let _: MeguriCoreBindings.CloudAdoptionApplied = try await run(.cloudConnectionStartAdoptionResult, result: .cloudAdoptionAppliedResult) { [raw] in
                let started = try raw.startAdoptionResult(handle: handle.raw, result: value)
                let job = CloudConnectionJobValue(handle: started.handle, adoptionResult: started.result, read: { try started.result.result() })
                #if DEBUG
                var diagnosticJob = job
                diagnosticJob.debugAdoptionSession = adoptionSession
                return diagnosticJob
                #else
                return job
                #endif
            }
        }
    }
    public func resumeAdoption(_ operation: CloudRecoveryOperation) async throws -> Bool {
        let raw = self.raw
        let value: MeguriCoreBindings.CloudAdoptionResumed = try await run(
            .cloudConnectionStartResumeAdoption, result: .cloudAdoptionResumeResult
        ) { [raw] in
            let started = try raw.startResumeAdoption(operation: operation.raw)
            return CloudConnectionJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return value.resumed
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
