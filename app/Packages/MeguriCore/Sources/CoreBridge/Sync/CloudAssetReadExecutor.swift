import Foundation
import Synchronization

/// Receipt for the original synchronous queue callback. Rust can throw after
/// enqueue has accepted, so submit's return alone cannot describe ownership.
private final class CloudAssetReadQueueReceipt: CloudAssetReadQueue {
    private enum State { case ready, calling, returned(Bool) }
    private let state = Mutex(State.ready)
    private let operation: any CloudPreparedOwnOperation

    init(_ operation: any CloudPreparedOwnOperation) { self.operation = operation }

    func enqueue() -> Bool {
        let mayCall = state.withLock { state in
            guard case .ready = state else { return false }
            state = .calling
            return true
        }
        guard mayCall else { return false }
        // Never hold the receipt lock across a foreign/native callback.
        let accepted = operation.enqueue()
        state.withLock { $0 = .returned(accepted) }
        return accepted
    }

    var accepted: Bool {
        state.withLock { if case .returned(true) = $0 { true } else { false } }
    }
}

public struct CloudAssetReadPassResult: Sendable, Equatable {
    public enum Outcome: Sendable, Equatable { case succeeded, failed }
    public let readCount: Int
    public let failures: [CloudAssetReadFailureFacts]
    public var outcome: Outcome { failures.isEmpty ? .succeeded : .failed }
}

/// Drains one opaque Rust pass, without refreshing its owner or scheduling a
/// replacement pass. Rust retains selection, retry and writer/WAL authority.
public actor CloudAssetReadExecutor {
    private let transport: any CloudOwnTransport
    private var active: (id: UUID, task: Task<CloudAssetReadPassResult, any Error>)?
    private var prepared: (any CloudPreparedOwnOperation)?
    private var closed = false

    public init(transport: any CloudOwnTransport) { self.transport = transport }

    @discardableResult
    public func run(_ pass: CoreCloudAssetReadPass) async throws -> CloudAssetReadPassResult {
        guard !closed else { throw CloudOwnTransportFailure.closed }
        guard active == nil else { throw CloudOwnTransportFailure.busy }
        let id = UUID()
        let task = Task { try await self.execute(pass) }
        active = (id, task)
        defer {
            if active?.id == id { active = nil; prepared = nil }
        }
        return try await withTaskCancellationHandler {
            // A caller already cancelled before entry must also stop this task.
            if Task.isCancelled { task.cancel() }
            return try await task.value
        } onCancel: { task.cancel() }
    }

    private func checkStopping() throws {
        guard !closed else { throw CloudOwnTransportFailure.closed }
        try Task.checkCancellation()
    }

    private func execute(_ pass: CoreCloudAssetReadPass) async throws -> CloudAssetReadPassResult {
        var failure: (any Error)?
        var failures: [CloudAssetReadFailureFacts] = []
        var readCount = 0
        do {
            while true {
                try checkStopping()
                guard let work = try await pass.next() else { break }
                if let facts = try await execute(work, pass: pass) { failures.append(facts) }
                readCount += 1
            }
        } catch { failure = error }
        // Finish also runs on cancelled tasks, after every accepted operation
        // has delivered its original terminal and complete has returned.
        // A finish/storage error must never be reported as a successful drain.
        try await pass.finish()
        if let failure { throw failure }
        return .init(readCount: readCount, failures: failures)
    }

    private func execute(_ work: CloudAssetReadIssuedWork, pass: CoreCloudAssetReadPass) async throws -> CloudAssetReadFailureFacts? {
        let operation: any CloudPreparedOwnOperation
        do {
            try checkStopping()
            operation = try transport.prepare(.assetRead(zone: work.zone, name: work.name))
            prepared = operation
        } catch {
            _ = try await pass.cancel(work.handle)
            throw error
        }
        defer { prepared = nil }
        let receipt = CloudAssetReadQueueReceipt(operation)
        return try await withTaskCancellationHandler {
            var submissionError: (any Error)?
            do {
                try checkStopping()
                guard try await pass.submit(work.handle, queue: receipt) else {
                    throw CloudOwnTransportFailure.queueRejected
                }
            } catch { submissionError = error }

            guard receipt.accepted else {
                operation.cancel()
                _ = try await pass.cancel(work.handle)
                throw submissionError ?? CloudOwnTransportFailure.queueRejected
            }

            if submissionError != nil || closed || Task.isCancelled { operation.cancel() }
            // No cancellation check may skip completion of accepted work.
            // Native reply joins a detached original terminal and owns its file.
            let reply: Result<CloudOwnTransportReply, any Error>
            do { reply = .success(try await operation.reply()) }
            catch { reply = .failure(error) }
            let facts: CloudAssetReadFailureFacts?
            switch reply {
            case .success(.assetRead(let result)):
                _ = try await pass.complete(work.handle, result: result)
                facts = result.failureFacts
            case .success:
                _ = try await pass.complete(work.handle, result: .failed)
                throw submissionError ?? CloudOwnTransportFailure.unexpectedReply
            case .failure(let error):
                _ = try await pass.complete(work.handle, result: .failed)
                throw submissionError ?? error
            }
            if let submissionError { throw submissionError }
            try checkStopping()
            return facts
        } onCancel: { operation.cancel() }
    }

    /// Permanent admission stop. Concurrent callers join the same original task;
    /// its accepted terminal and core completion drain before any close returns.
    public func close() async {
        closed = true
        let original = active?.task
        original?.cancel()
        prepared?.cancel()
        _ = await original?.result
    }
}
