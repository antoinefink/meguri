import Foundation
import Synchronization

/// Records acceptance independently of a Rust return or a callback that may
/// arrive during enqueue. No receipt lock spans the native queue callback.
private final class ZoneDeletionQueueReceipt: CloudZoneDeletionQueue {
    private let state = Mutex((called: false, accepted: false))
    let operation: any CloudPreparedOwnOperation
    init(_ operation: any CloudPreparedOwnOperation) { self.operation = operation }
    func enqueue() -> Bool {
        guard state.withLock({ s in if s.called { return false }; s.called = true; return true }) else { return false }
        let accepted = operation.enqueue()
        state.withLock { $0.accepted = accepted }
        return accepted
    }
    var accepted: Bool { state.withLock { $0.accepted } }
}

/// One Core-issued read and, if allowed, one immediately admitted zone batch.
/// No retries, CKSyncEngine work, activity cycles or replacement operations.
public actor CloudZoneDeletionExecutor {
    private let transport: any CloudOwnTransport
    private let freshAccountRead: (@Sendable () async throws -> Void)?
    private var active: Task<Void, any Error>?
    private var prepared: (any CloudPreparedOwnOperation)?
    private var closed = false
    private var failureCategory: CloudZoneDeletionLocalFailure = .preparation
    private var recordedFailure = false
    public init(transport: any CloudOwnTransport, freshAccountRead: (@Sendable () async throws -> Void)? = nil) {
        self.transport = transport; self.freshAccountRead = freshAccountRead
    }
    private func refreshForReset(_ facts: [CloudZoneDeletionFailure?], rootMissing: Bool = false) async throws {
        guard rootMissing || facts.contains(where: { $0?.code == 26 || $0?.code == 28 }) else { return }
        guard let freshAccountRead else { throw BridgeError.unavailable(reason: "Fresh cloud account reader unavailable") }
        try await freshAccountRead(); try check()
    }

    public func run(_ pass: CoreCloudZoneDeletionPass) async throws {
        guard !closed else {
            CloudZoneDeletionDiagnostics.record(.closed, error: CloudOwnTransportFailure.closed)
            try await pass.finish(); throw CloudOwnTransportFailure.closed
        }
        guard active == nil else {
            CloudZoneDeletionDiagnostics.record(.busy, error: CloudOwnTransportFailure.busy)
            throw CloudOwnTransportFailure.busy
        }
        let task = Task { try await self.perform(pass) }
        active = task
        defer { active = nil; prepared = nil }
        try await withTaskCancellationHandler {
            if Task.isCancelled { task.cancel() }
            try await task.value
        } onCancel: { task.cancel() }
    }
    private func check() throws {
        guard !closed else { throw CloudOwnTransportFailure.closed }
        try Task.checkCancellation()
    }
    private func perform(_ pass: CoreCloudZoneDeletionPass) async throws {
        var failure: (any Error)?
        failureCategory = .preparation
        recordedFailure = false
        do {
            try check()
            let target = try await pass.activeTarget()
            let read = try await operation(.zoneDeletionActiveRead(target), pass: pass, deletion: false)
            let batch: CloudZoneDeletionBatch?
            failureCategory = .completion
            switch read {
            case .zoneDeletionActiveRead(let result):
                try await refreshForReset([result.failure, result.terminal], rootMissing: result.read == .missing || result.read == .zoneMissing)
                batch = try await pass.completeRead(result)
            default:
                failureCategory = .invalidReply
                _ = try await pass.completeRead(.init(read: .failed, failure: .init(code: nil)))
                throw CloudOwnTransportFailure.unexpectedReply
            }
            try check()
            if let batch {
                let reply = try await operation(.deleteZones(batch), pass: pass, deletion: true)
                failureCategory = .completion
                switch reply {
                case .deletedZones(let result):
                    try await refreshForReset(result.results.map(\.failure) + [result.terminal])
                    try await pass.completeDeletion(result)
                default:
                    failureCategory = .invalidReply
                    try await pass.completeDeletion(.init(results: batch.zones.map { .init(zone: $0, failure: .init(code: nil)) }))
                    throw CloudOwnTransportFailure.unexpectedReply
                }
            }
            try check()
        } catch {
            if !recordedFailure { CloudZoneDeletionDiagnostics.record(failureCategory, error: error) }
            failure = error
        }
        do { try await pass.finish() }
        catch { CloudZoneDeletionDiagnostics.record(.completion, error: error); throw error }
        if let failure { throw failure }
    }
    private func operation(_ command: CloudOwnTransportCommand, pass: CoreCloudZoneDeletionPass, deletion: Bool) async throws -> CloudOwnTransportReply {
        try check()
        failureCategory = .preparation
        let operation = try transport.prepare(command)
        prepared = operation
        defer { prepared = nil }
        let receipt = ZoneDeletionQueueReceipt(operation)
        return try await withTaskCancellationHandler {
            var submissionError: (any Error)?
            var submissionCategory = CloudZoneDeletionLocalFailure.admission
            do {
                try check()
                failureCategory = .admission
                let admitted: Bool
                if deletion { admitted = try await pass.submitDeletion(queue: receipt) }
                else { admitted = try await pass.submitRead(queue: receipt) }
                if !admitted { failureCategory = .queueRejected; throw CloudOwnTransportFailure.queueRejected }
            } catch { submissionError = error; submissionCategory = failureCategory }
            guard receipt.accepted else {
                operation.cancel()
                throw submissionError ?? CloudOwnTransportFailure.queueRejected
            }
            if submissionError != nil || Task.isCancelled || closed { operation.cancel() }
            // Native reply waits for the original terminal even in a cancelled
            // task. Every accepted read/batch is consumed exactly once below.
            let reply: CloudOwnTransportReply
            failureCategory = .transport
            do { reply = try await operation.reply() }
            catch {
                // A throwing transport has no per-item reply. Keep its actual
                // terminal facts and a fixed local category before Core follow-up.
                CloudZoneDeletionDiagnostics.record(.transport, error: error)
                recordedFailure = true
                let terminal = CloudZoneDeletionFailure.evidence(error)
                do {
                    if case .deleteZones(let batch) = command {
                        try await pass.completeDeletion(.init(results: batch.zones.map { .init(zone: $0, failure: .init(code: nil)) }, terminal: terminal))
                    } else { _ = try await pass.completeRead(.init(read: .failed, terminal: terminal)) }
                } catch { CloudZoneDeletionDiagnostics.record(.completion, error: error) }
                if let submissionError { CloudZoneDeletionDiagnostics.record(submissionCategory, error: submissionError) }
                throw submissionError ?? error
            }
            if let submissionError {
                CloudZoneDeletionDiagnostics.record(submissionCategory, error: submissionError)
                recordedFailure = true
                switch reply {
                case .zoneDeletionActiveRead(let result): _ = try await pass.completeRead(result)
                case .deletedZones(let result): try await pass.completeDeletion(result)
                default: break
                }
                throw submissionError
            }
            return reply
        } onCancel: { operation.cancel() }
    }
    public func interruptAndDrain() async {
        let original = active
        original?.cancel(); prepared?.cancel()
        _ = await original?.result
    }
    public func close() async { closed = true; await interruptAndDrain() }
}
