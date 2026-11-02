import Foundation
import Synchronization

/// Commands contain only native transport inputs projected by Rust. Recovery
/// decisions and confirmations remain in the retained core handles.
public enum CloudOwnTransportCommand: Sendable {
    case publication(CloudOwnOperation)
    case read(CloudControlTarget)
    /// The core supplies the work for this read.
    /// Its submit guards must pass before the operation can start.
    case assetRead(zone: String, name: String)
    case zoneDeletionActiveRead(CloudControlTarget)
    case deleteZones(CloudZoneDeletionBatch)
    /// Fetch every zone in the private database, without a guessed zone list.
    case zoneInventory
    case adoptionPage(zone: String, cursor: Data?)
}
public enum CloudOwnTransportReply: Sendable {
    case publication(CloudPublicationResult)
    case read(CloudControlRead)
    case assetRead(CloudAssetReadResult)
    case zoneDeletionActiveRead(CloudZoneDeletionRead)
    case deletedZones(CloudZoneDeletionReply)
    /// Complete, validated private-owner names from the original successful
    /// all-zones terminal callback, sorted deterministically. Empty is success.
    case zoneInventory(zones: [String])
    case page(records: [CloudStagingRecord], deleted: [String], next: Data?)
}
public enum CloudOwnTransportFailure: Error, Sendable {
    case closed, busy, queueRejected, missingReply, unexpectedReply, invalidCursor, privateDatabaseRequired
}
public protocol CloudPreparedOwnOperation: CloudPublicationQueue, CloudAssetReadQueue, CloudZoneDeletionQueue {
    /// The original operation's terminal callback, buffered even if it arrives
    /// during Rust submission. Cancellation must drain this same callback.
    func reply() async throws -> CloudOwnTransportReply
    func cancel()
    /// Original all-fields control-read errors, without retaining SDK objects.
    func controlReadEvidence() -> CloudZoneDeletionRead?
}
extension CloudPreparedOwnOperation {
    public func controlReadEvidence() -> CloudZoneDeletionRead? { nil }
}
public protocol CloudOwnTransport: Sendable {
    /// Construct the complete native operation and all reply handlers before
    /// entering Rust's guarded synchronous enqueue callback.
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation
}
public enum CloudConnectionProgress: Sendable { case progressed, sendEntities, idle }

extension CoreCloudWorkflowService {
    /// Uses the existing guarded reconciliation before the app projects policy.
    public func prepareRuntimeBeforePolicy() async throws { try await prepareRuntime() }
}

/// App lifecycle admission only. Core retains every request and binding authority.
public final class CloudOwnWorkAdmission: Sendable {
    private let open = Mutex(true)
    public init() {}
    public func setOpen(_ value: Bool) { open.withLock { $0 = value } }
    public func check() throws { if !open.withLock({ $0 }) { throw CancellationError() } }
    /// Linearize final queue/result admission against lifecycle closure.
    func withPermission<T>(_ body: (Bool) throws -> T) rethrows -> T {
        try open.withLock { try body($0) }
    }
    fileprivate func enqueue(_ operation: any CloudPublicationQueue) -> Bool {
        open.withLock { $0 && operation.enqueue() }
    }
}
private struct AdmittedCloudOwnQueue: CloudPublicationQueue {
    let admission: CloudOwnWorkAdmission
    let operation: any CloudPublicationQueue
    func enqueue() -> Bool { admission.enqueue(operation) }
}

/// Runs outside CKSyncEngine event delivery. Own requests always cross Rust's
/// submit guard after their last suspension and immediately before queue add.
public actor CloudOwnExecutor {
    private let core: CoreCloudConnectionService
    private let transport: any CloudOwnTransport
    private let observe: (@Sendable (CloudOwnObservation) -> Void)?
    private var active: (id: UUID, task: Task<CloudConnectionProgress, any Error>)?
    private var prepared: (any CloudPreparedOwnOperation)?
    private var previewTask: Task<CloudConnectionPreviewWork, any Error>?
    private var stopping = false
    private var closed = false
    public init(core: CoreCloudConnectionService, transport: any CloudOwnTransport,
                observe: (@Sendable (CloudOwnObservation) -> Void)? = nil) {
        self.core = core; self.transport = transport; self.observe = observe
    }

    public func runNext(admission: CloudOwnWorkAdmission? = nil,
                        beforeActivation: (@Sendable () async throws -> Void)? = nil,
                        afterActivation: (@Sendable () async -> Void)? = nil) async throws -> CloudConnectionProgress {
        try requireIdle()
        return try await run { try await self.executeNext(admission: admission, beforeActivation: beforeActivation, afterActivation: afterActivation) }
    }
    /// Reads the actual native records for the preview. Returning ready grants
    /// no publication; the caller must keep and confirm the returned handle.
    public func preview() async throws -> CloudConnectionPreviewWork {
        try requireIdle()
        // Use the same active task slot as recovery operations. The separate
        // result task retains its typed preview while cancel/close join it.
        let result = Task { try await self.executePreview() }
        previewTask = result
        let id = UUID()
        let join = Task<CloudConnectionProgress, any Error> { _ = try await result.value; return .progressed }
        active = (id, join)
        defer { if active?.id == id { active = nil; prepared = nil; previewTask = nil } }
        return try await withTaskCancellationHandler { try await result.value }
            onCancel: { Task { await self.cancelPreview(id: id) } }
    }
    public func interruptAndDrain() async {
        guard !closed else { return }
        stopping = true
        previewTask?.cancel()
        active?.task.cancel()
        prepared?.cancel()
        if let active { _ = await active.task.result }
        if !closed { stopping = false }
    }
    public func cancelPreview() async {
        guard let id = active?.id else { return }
        await cancelPreview(id: id)
    }
    private func cancelPreview(id: UUID) async {
        guard active?.id == id, let original = previewTask else { return }
        let originalJoin = active?.task
        stopping = true
        original.cancel()
        prepared?.cancel()
        _ = await original.result
        _ = await originalJoin?.result
        if active?.id == id { previewTask = nil }
        if !closed { stopping = false }
    }
    private func requireIdle() throws {
        if closed || stopping { throw CloudOwnTransportFailure.closed }
        if active != nil { throw CloudOwnTransportFailure.busy }
    }
    private func run(_ body: @escaping @Sendable () async throws -> CloudConnectionProgress) async throws -> CloudConnectionProgress {
        let id = UUID()
        let task = Task { try await body() }
        active = (id, task)
        defer { if active?.id == id { active = nil; prepared = nil } }
        return try await task.value
    }
    private func checkStopping() throws {
        if stopping || closed { throw CloudOwnTransportFailure.closed }
        try Task.checkCancellation()
    }
    private func read(_ target: CloudControlTarget, admission: CloudOwnWorkAdmission? = nil) async throws -> CloudControlRead {
        try checkStopping()
        let op = try transport.prepare(.read(target)); prepared = op
        try checkStopping()
        guard admission.map({ $0.enqueue(op) }) ?? op.enqueue() else { throw CloudOwnTransportFailure.queueRejected }
        let result = try await op.reply()
        prepared = nil
        guard case .read(let read) = result else { throw CloudOwnTransportFailure.unexpectedReply }
        return read
    }
    private func executePreview() async throws -> CloudConnectionPreviewWork {
        var work = try await core.beginConnectionPreview()
        do {
            while true {
                switch work {
                case .readActive(let p, let target):
                    let reply = try await read(target)
                    try checkStopping()
                    work = try await core.connectionActiveRead(p, read: reply)
                case .readEpoch(let p, let target):
                    let reply = try await read(target)
                    try checkStopping()
                    work = try await core.connectionEpochRead(p, read: reply)
                case .ready:
                    try checkStopping()
                    return work
                }
            }
        } catch {
            let handle: CloudConnectionPreview
            switch work { case .readActive(let p, _), .readEpoch(let p, _), .ready(let p, _): handle = p }
            try? await core.cancelPreview(handle)
            throw error
        }
    }
    private func executeNext(admission: CloudOwnWorkAdmission?,
                             beforeActivation: (@Sendable () async throws -> Void)?,
                             afterActivation: (@Sendable () async -> Void)?) async throws -> CloudConnectionProgress {
        try admission?.check()
        let work = try await core.nextPublicationWork()
        try checkStopping()
        try admission?.check()
        switch work {
        case .local(let handle, _, _), .scan(let handle):
            try checkStopping()
            try await core.performLocal(handle)
        case .own(let request, let command, _, let metadata):
            let identity = UUID()
            let observation = observe
            let emit: @Sendable (CloudOwnObservationPhase) -> Void = { phase in
                observation?(.init(metadata: metadata, identity: identity, phase: phase))
            }
            let op: any CloudPreparedOwnOperation
            do {
                try checkStopping()
                op = try transport.prepare(.publication(command)); prepared = op
                emit(.prepared)
            } catch {
                // No enqueue occurred. Runtime owns the exact attempt and
                // decides whether this not-started report is still admissible.
                try? await core.publicationResult(request, result: .notStarted)
                throw error
            }
            let accepted: Bool
            do {
                let queue: any CloudPublicationQueue
                let observed: any CloudPublicationQueue
                if observation != nil {
                    observed = ObservedCloudPublicationQueue(original: op, observe: { emit(.enqueued($0)) })
                } else { observed = op }
                if let admission { queue = AdmittedCloudOwnQueue(admission: admission, operation: observed) }
                else { queue = observed }
                accepted = try await core.submitPublicationRequest(request, queue: queue)
            } catch {
                op.cancel(); prepared = nil
                try? await core.publicationResult(request, result: .notStarted)
                throw error
            }
            guard accepted else {
                prepared = nil
                // Runtime consumed the actual synchronous refusal and restored
                // any prior durable evidence. There is no native reply to drain.
                throw CloudOwnTransportFailure.queueRejected
            }
            do {
                let reply = try await op.reply() // Rust has returned.
                prepared = nil
                guard case .publication(let result) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                emit(.replied(result))
                try await core.publicationResult(request, result: result)
                emit(.committed)
            } catch {
                prepared = nil
                // An absent reply remains durably uncertain. A terminal native
                // validation error is an actual failure, never NotStarted or a
                // fabricated successful save/tag.
                if let failure = error as? CloudOwnTransportFailure, case .missingReply = failure {
                    throw error
                }
                try? await core.publicationResult(request, result: .failed(retryAfterMilliseconds: nil))
                throw error
            }
        case .adoption(let handle, let action, _):
            try checkStopping()
            switch action {
            case .read(let target):
                try await core.adoptionResult(handle, result: .read(try await read(target, admission: admission)))
            case .fetch(let zone, let cursor):
                let op = try transport.prepare(.adoptionPage(zone: zone, cursor: cursor)); prepared = op
                try checkStopping()
                guard admission.map({ $0.enqueue(op) }) ?? op.enqueue() else { throw CloudOwnTransportFailure.queueRejected }
                do {
                    let reply = try await op.reply(); prepared = nil
                    guard case .page(let records, let deleted, let next) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                    try await core.adoptionResult(handle, result: .page(records: records, deleted: deleted, next: next))
                } catch {
                    prepared = nil
                    // A failed page is an actual failed read, never an empty
                    // final page. Host retains or aborts staging by its policy.
                    try? await core.adoptionResult(handle, result: .read(.failed))
                    throw error
                }
            case .validate: try await core.performAdoptionLocal(handle)
            case .activate:
                try await beforeActivation?()
                do {
                    try checkStopping()
                    try admission?.check()
                    try await core.performAdoptionLocal(handle)
                } catch {
                    await afterActivation?()
                    throw error
                }
                await afterActivation?()
            }
        case .sendEntities: return .sendEntities
        case .idle: return .idle
        }
        return .progressed
    }
    /// Stop queue admission first, cancel the current operation, then await its
    /// original reply/delivery before asking Rust to cancel the exact recovery.
    public func cancel(_ operation: CloudRecoveryOperation) async throws {
        guard !closed, !stopping else { throw CloudOwnTransportFailure.closed }
        stopping = true
        defer { stopping = false }
        prepared?.cancel()
        if let active { _ = await active.task.result }
        try await core.cancelOperation(operation)
    }
    /// The owner must join this before closing its CoreHost. An accepted reply
    /// lost by the SDK remains a durable recovery question in Rust.
    public func close() async {
        closed = true; stopping = true
        previewTask?.cancel()
        prepared?.cancel()
        if let active { _ = await active.task.result }
    }
}
