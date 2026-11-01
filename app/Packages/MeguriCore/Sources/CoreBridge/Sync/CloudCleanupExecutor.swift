import Foundation
import Instrumentation
import Synchronization
internal import MeguriCoreBindings

public protocol CloudCleanupQueue: Sendable { func enqueue() -> Bool }
private final class ForeignCleanupQueue: MeguriCoreBindings.CloudCleanupQueue {
    let queue: any CloudCleanupQueue
    init(_ queue: any CloudCleanupQueue) { self.queue = queue }
    func enqueue() -> Bool { queue.enqueue() }
}
/// Retains the original command, Host, Runtime, Collection and SDK in Rust.
public struct CoreCloudCleanupPass: Sendable {
    private let raw: MeguriCoreBindings.CloudCleanupPass
    init(_ raw: MeguriCoreBindings.CloudCleanupPass) { self.raw = raw }
    public func submitInventory(queue: any CloudCleanupQueue) async throws -> Bool {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudCleanupPassSubmitInventory) {
            try raw.submitInventory(queue: ForeignCleanupQueue(queue))
        }
    }
    @discardableResult public func completeInventory(zones: [String]?) async throws -> Bool {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudCleanupPassCompleteInventory) {
            try raw.completeInventory(zones: zones)
        }
    }
    public func finish() async throws {
        let raw = self.raw
        try await MaintenanceLane.shared.call(.cloudCleanupPassFinish) { try raw.finish() }
    }
}
extension CoreCloudPolicyExecution {
    /// Computes P10.3 once from the issued drain, then retains its listing due.
    func beginCleanupPass() async throws -> CoreCloudCleanupPass {
        let raw = self.raw
        return try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginCleanupPass) {
            CoreCloudCleanupPass(try raw.beginCleanupPass())
        }
    }
}
private final class CleanupQueueReceipt: CloudCleanupQueue {
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
/// Keeps bounded error classifications for cleanup diagnostics.
public enum CloudCleanupDiagnostics {
    private static let failures = Mutex<[CloudZoneDeletionFailure]>([])
    public static func localFailures() -> [CloudZoneDeletionFailure] { failures.withLock { $0 } }
    static func record(_ error: any Error) {
        let facts = CloudZoneDeletionFailure.evidence(error)
        failures.withLock {
            if $0.count == 64 { $0.removeFirst() }
            $0.append(facts)
        }
    }
}
/// The optional A21-false observation after the required local computation.
/// Each accepted enqueue joins its original terminal; failure never retries.
public actor CloudCleanupExecutor {
    private let transport: any CloudOwnTransport
    private var active: Task<Void, any Error>?
    private var prepared: (any CloudPreparedOwnOperation)?
    private var closed = false
    public init(transport: any CloudOwnTransport) { self.transport = transport }
    public func run(_ pass: CoreCloudCleanupPass) async throws {
        guard !closed else {
            CloudCleanupDiagnostics.record(CloudOwnTransportFailure.closed)
            try await pass.finish(); throw CloudOwnTransportFailure.closed
        }
        guard active == nil else {
            CloudCleanupDiagnostics.record(CloudOwnTransportFailure.busy)
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
    private func perform(_ pass: CoreCloudCleanupPass) async throws {
        var failure: (any Error)?
        do { try await inventory(pass) }
        catch { CloudCleanupDiagnostics.record(error); failure = error }
        do { try await pass.finish() }
        catch { CloudCleanupDiagnostics.record(error); throw error }
        if let failure { throw failure }
    }
    private func inventory(_ pass: CoreCloudCleanupPass) async throws {
        try check()
        let operation = try transport.prepare(.zoneInventory)
        prepared = operation
        defer { prepared = nil }
        let receipt = CleanupQueueReceipt(operation)
        try await withTaskCancellationHandler {
            var submissionError: (any Error)?
            do {
                try check()
                guard try await pass.submitInventory(queue: receipt) else { throw CloudOwnTransportFailure.queueRejected }
            } catch { submissionError = error }
            guard receipt.accepted else {
                operation.cancel()
                throw submissionError ?? CloudOwnTransportFailure.queueRejected
            }
            if submissionError != nil || Task.isCancelled || closed { operation.cancel() }
            let reply: CloudOwnTransportReply
            do { reply = try await operation.reply() }
            catch {
                // Transport owns the actual per-zone/terminal error. None is
                // an unsuccessful listing, never an invented empty inventory.
                CloudCleanupDiagnostics.record(error)
                do { try await pass.completeInventory(zones: nil) }
                catch { CloudCleanupDiagnostics.record(error) }
                throw submissionError ?? error
            }
            if let submissionError {
                try await pass.completeInventory(zones: nil)
                throw submissionError
            }
            if Task.isCancelled || closed {
                try await pass.completeInventory(zones: nil)
                try check()
            }
            guard case .zoneInventory(let zones) = reply else {
                try await pass.completeInventory(zones: nil)
                throw CloudOwnTransportFailure.unexpectedReply
            }
            try await pass.completeInventory(zones: zones)
        } onCancel: { operation.cancel() }
    }
    public func interruptAndDrain() async {
        let original = active
        original?.cancel(); prepared?.cancel()
        _ = await original?.result
    }
    public func close() async { closed = true; await interruptAndDrain() }
}
