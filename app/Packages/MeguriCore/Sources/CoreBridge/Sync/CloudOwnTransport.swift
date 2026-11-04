import Foundation
import Synchronization
#if DEBUG
import Observation
#endif
#if DEBUG && canImport(CloudKit)
import CloudKit
#endif

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
#if DEBUG
/// Temporary, memory-only diagnosis. Each instance stores one attempt and bounded safe tags.
public final class DebugCloudPreviewProbe: Observable, Sendable {
    enum WorkStage: CaseIterable, Equatable, Sendable {
        case nextPublicationWork, performLocal
        case adoptionControlRead, adoptionControlResult
        case adoptionPageReply, adoptionPageResult, adoptionPageFailureResult
        case adoptionValidation, beforeActivation, adoptionActivation, afterActivation

        var tag: String {
            switch self {
            case .nextPublicationWork: "nextPublicationWork"
            case .performLocal: "performLocal"
            case .adoptionControlRead: "adoptionControlRead"
            case .adoptionControlResult: "adoptionControlResult"
            case .adoptionPageReply: "adoptionPageReply"
            case .adoptionPageResult: "adoptionPageResult"
            case .adoptionPageFailureResult: "adoptionPageFailureResult"
            case .adoptionValidation: "adoptionValidation"
            case .beforeActivation: "beforeActivation"
            case .adoptionActivation: "adoptionActivation"
            case .afterActivation: "afterActivation"
            }
        }
    }

    public static let shared = DebugCloudPreviewProbe()
    public static let ownWork = DebugCloudPreviewProbe()
    private let changes = ObservationRegistrar()
    private let latest = Mutex<(attempt: UUID, tag: String, stage: WorkStage?)?>(nil)

    init() {}

    public var latestTag: String {
        changes.access(self, keyPath: \.latestTag)
        return latest.withLock { $0?.tag ?? "unobserved" }
    }

    /// Last entered stage for this attempt, not evidence of completion or cause.
    public var lastEnteredWorkStageTag: String? {
        changes.access(self, keyPath: \.lastEnteredWorkStageTag)
        return latest.withLock { $0?.stage?.tag }
    }

    func begin() -> UUID {
        let attempt = UUID()
        changes.withMutation(of: self, keyPath: \.latestTag) {
            changes.withMutation(of: self, keyPath: \.lastEnteredWorkStageTag) {
                latest.withLock { $0 = (attempt, "running", nil) }
            }
        }
        return attempt
    }

    func finish(_ attempt: UUID) { update(attempt, tag: "finished") }

    /// The tag marks the last entered await boundary, not its completion.
    func enter(_ stage: WorkStage, attempt: UUID) {
        changes.withMutation(of: self, keyPath: \.latestTag) {
            changes.withMutation(of: self, keyPath: \.lastEnteredWorkStageTag) {
                latest.withLock { if $0?.attempt == attempt { $0 = (attempt, stage.tag, stage) } }
            }
        }
    }

    func finish(_ attempt: UUID, progress: CloudConnectionProgress) {
        let tag: String
        switch progress {
        case .progressed: tag = "progressed"
        case .sendEntities: tag = "sendEntities"
        case .idle: tag = "idle"
        }
        update(attempt, tag: tag)
    }

    func fail(_ attempt: UUID, error: any Error) {
        let tag = Self.classify(error)
        update(attempt, tag: tag)
    }

    private func update(_ attempt: UUID, tag: String) {
        // Native observation notifies outside the mutex, including synchronous
        // observers that read latestTag. Recheck the attempt at the state update.
        changes.withMutation(of: self, keyPath: \.latestTag) {
            latest.withLock { if $0?.attempt == attempt { $0?.tag = tag } }
        }
    }

    static func classify(_ error: any Error) -> String {
        if error is CancellationError { return "taskCancelled" }
        if let failure = error as? CloudOwnTransportFailure {
            switch failure {
            case .closed: return "transportClosed"
            case .busy: return "transportBusy"
            case .queueRejected: return "transportQueueRejected"
            case .missingReply: return "transportMissingReply"
            case .unexpectedReply: return "transportUnexpectedReply"
            case .invalidCursor: return "transportInvalidCursor"
            case .privateDatabaseRequired: return "transportPrivateDatabaseRequired"
            }
        }
        if let failure = error as? BridgeError {
            switch failure {
            case .cancelled: return "coreCancelled"
            case .unavailable: return "coreUnavailable"
            case .internal: return "coreInternal"
            default: return "coreOther"
            }
        }
        // Exact value types only: conditional casts alone can bridge fake domains.
        #if canImport(CloudKit)
        if type(of: error) == CKError.self, let native = error as? CKError {
            guard let code = Int32(exactly: native.code.rawValue) else { return "other" }
            return "CKErrorDomain:\(code)"
        }
        #endif
        if type(of: error) == URLError.self, let native = error as? URLError {
            guard let code = Int32(exactly: native.code.rawValue) else { return "other" }
            return "NSURLErrorDomain:\(code)"
        }
        if type(of: error) == CocoaError.self, let native = error as? CocoaError {
            guard let code = Int32(exactly: native.code.rawValue) else { return "other" }
            return "NSCocoaErrorDomain:\(code)"
        }
        // Reject other Swift errors before bridging can consult their payloads.
        guard type(of: error) is NSError.Type else { return "other" }
        let native = error as NSError
        guard let code = Int32(exactly: native.code) else { return "other" }
        switch native.domain {
        case "CKErrorDomain": return "CKErrorDomain:\(code)"
        case NSURLErrorDomain: return "NSURLErrorDomain:\(code)"
        case NSCocoaErrorDomain: return "NSCocoaErrorDomain:\(code)"
        case NSOSStatusErrorDomain: return "NSOSStatusErrorDomain:\(code)"
        default: return "other"
        }
    }
}
#endif
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
    #if DEBUG
    private var debugWorkAttempt: UUID?
    #endif
    public init(core: CoreCloudConnectionService, transport: any CloudOwnTransport,
                observe: (@Sendable (CloudOwnObservation) -> Void)? = nil) {
        self.core = core; self.transport = transport; self.observe = observe
    }

    public func runNext(admission: CloudOwnWorkAdmission? = nil,
                        beforeActivation: (@Sendable () async throws -> Void)? = nil,
                        afterActivation: (@Sendable () async -> Void)? = nil) async throws -> CloudConnectionProgress {
        #if DEBUG
        let probeAttempt = DebugCloudPreviewProbe.ownWork.begin()
        #endif
        do {
            try requireIdle()
            #if DEBUG
            debugWorkAttempt = probeAttempt
            defer { if debugWorkAttempt == probeAttempt { debugWorkAttempt = nil } }
            #endif
            let progress = try await run { try await self.executeNext(admission: admission, beforeActivation: beforeActivation, afterActivation: afterActivation) }
            #if DEBUG
            DebugCloudPreviewProbe.ownWork.finish(probeAttempt, progress: progress)
            #endif
            return progress
        } catch {
            #if DEBUG
            DebugCloudPreviewProbe.ownWork.fail(probeAttempt, error: error)
            #endif
            throw error
        }
    }
    /// Reads the actual native records for the preview. Returning ready grants
    /// no publication; the caller must keep and confirm the returned handle.
    public func preview() async throws -> CloudConnectionPreviewWork {
        #if DEBUG
        let probeAttempt = DebugCloudPreviewProbe.shared.begin()
        #endif
        do {
            try requireIdle()
            // Use the same active task slot as recovery operations. The separate
            // result task retains its typed preview while cancel/close join it.
            let result = Task { try await self.executePreview() }
            previewTask = result
            let id = UUID()
            let join = Task<CloudConnectionProgress, any Error> { _ = try await result.value; return .progressed }
            active = (id, join)
            defer { if active?.id == id { active = nil; prepared = nil; previewTask = nil } }
            let work = try await withTaskCancellationHandler { try await result.value }
                onCancel: { Task { await self.cancelPreview(id: id) } }
            #if DEBUG
            DebugCloudPreviewProbe.shared.finish(probeAttempt)
            #endif
            return work
        } catch {
            #if DEBUG
            DebugCloudPreviewProbe.shared.fail(probeAttempt, error: error)
            #endif
            throw error
        }
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
    #if DEBUG
    private func recordWorkStage(_ stage: DebugCloudPreviewProbe.WorkStage) {
        guard let attempt = debugWorkAttempt else { return }
        DebugCloudPreviewProbe.ownWork.enter(stage, attempt: attempt)
    }
    #endif
    private func executeNext(admission: CloudOwnWorkAdmission?,
                             beforeActivation: (@Sendable () async throws -> Void)?,
                             afterActivation: (@Sendable () async -> Void)?) async throws -> CloudConnectionProgress {
        try admission?.check()
        #if DEBUG
        recordWorkStage(.nextPublicationWork)
        #endif
        let work = try await core.nextPublicationWork()
        try checkStopping()
        try admission?.check()
        switch work {
        case .local(let handle, _, _), .scan(let handle):
            try checkStopping()
            #if DEBUG
            recordWorkStage(.performLocal)
            #endif
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
                #if DEBUG
                recordWorkStage(.adoptionControlRead)
                let reply = try await read(target, admission: admission)
                recordWorkStage(.adoptionControlResult)
                try await core.adoptionResult(handle, result: .read(reply))
                #else
                try await core.adoptionResult(handle, result: .read(try await read(target, admission: admission)))
                #endif
            case .fetch(let zone, let cursor):
                let op = try transport.prepare(.adoptionPage(zone: zone, cursor: cursor)); prepared = op
                try checkStopping()
                guard admission.map({ $0.enqueue(op) }) ?? op.enqueue() else { throw CloudOwnTransportFailure.queueRejected }
                do {
                    #if DEBUG
                    recordWorkStage(.adoptionPageReply)
                    #endif
                    let reply = try await op.reply(); prepared = nil
                    guard case .page(let records, let deleted, let next) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                    #if DEBUG
                    recordWorkStage(.adoptionPageResult)
                    #endif
                    try await core.adoptionResult(handle, result: .page(records: records, deleted: deleted, next: next))
                } catch {
                    prepared = nil
                    // A failed page is an actual failed read, never an empty
                    // final page. Host retains or aborts staging by its policy.
                    #if DEBUG
                    recordWorkStage(.adoptionPageFailureResult)
                    #endif
                    try? await core.adoptionResult(handle, result: .read(.failed))
                    throw error
                }
            case .validate:
                #if DEBUG
                recordWorkStage(.adoptionValidation)
                #endif
                try await core.performAdoptionLocal(handle)
            case .activate:
                #if DEBUG
                if beforeActivation != nil { recordWorkStage(.beforeActivation) }
                #endif
                try await beforeActivation?()
                do {
                    try checkStopping()
                    try admission?.check()
                    #if DEBUG
                    recordWorkStage(.adoptionActivation)
                    #endif
                    try await core.performAdoptionLocal(handle)
                } catch {
                    #if DEBUG
                    if afterActivation != nil { recordWorkStage(.afterActivation) }
                    #endif
                    await afterActivation?()
                    throw error
                }
                #if DEBUG
                if afterActivation != nil { recordWorkStage(.afterActivation) }
                #endif
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
