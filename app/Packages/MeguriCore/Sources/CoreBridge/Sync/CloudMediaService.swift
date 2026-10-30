import Foundation
import Instrumentation
import Synchronization
internal import MeguriCoreBindings

public enum CloudMediaFailure: Error, Sendable {
    case closed, busy, queueRejected, missingReply, requiresSyncEngine, invalidAsset, assetTooLarge, noSpace, stagingLimit, metadataScope, failedRead
}
public enum CloudMediaRequest: Sendable, Equatable {
    case root, read(name: String), reads(names: [String]), download(name: String)
    case upload(name: String, envelope: Data, stagedPath: String)
    var downloadsAsset: Bool { if case .download = self { true } else { false } }
    init(_ raw: MeguriCoreBindings.CloudMediaRequest) {
        switch raw {
        case .root: self = .root
        case .read(let name): self = .read(name: name)
        case .reads(let names): self = .reads(names: names)
        case .download(let name): self = .download(name: name)
        case .upload(let name, let envelope, let path): self = .upload(name: name, envelope: envelope, stagedPath: path)
        }
    }
}
public struct CloudMediaWork: Sendable { let raw: MeguriCoreBindings.CloudMediaWork }
public struct CloudMediaIssuedWork: Sendable {
    public let handle: CloudMediaWork
    public let request: CloudMediaRequest
    init(_ raw: MeguriCoreBindings.CloudMediaIssuedWork) { handle = .init(raw: raw.handle); request = .init(raw.request) }
}
public enum CloudMediaBlobReadResult: Sendable {
    case present(name: String, envelope: Data), unknownItem(name: String), zoneMissing(name: String), failed(name: String, code: Int64)
    var raw: MeguriCoreBindings.CloudMediaBlobRead {
        switch self {
        case .present(let name, let envelope): .init(name: name, result: .present(envelope: envelope))
        case .unknownItem(let name): .init(name: name, result: .unknownItem)
        case .zoneMissing(let name): .init(name: name, result: .zoneMissing)
        case .failed(let name, let code): .init(name: name, result: .failed(code: code))
        }
    }
    init(name: String, result: CloudMediaReadResult) throws {
        switch result {
        case .record(let actual, let type, let envelope, nil) where actual == name && type == "MGBlob": self = .present(name: name, envelope: envelope)
        case .unknownItem: self = .unknownItem(name: name)
        case .zoneMissing: self = .zoneMissing(name: name)
        case .failed(let code): self = .failed(name: name, code: code)
        default: throw CloudMediaFailure.invalidAsset
        }
    }
}
public enum CloudMediaReadResult: Sendable {
    case blobReads([CloudMediaBlobReadResult])
    case record(name: String, recordType: String, envelope: Data, asset: CloudMediaStagedAsset?)
    case unknownItem, zoneMissing, failed(code: Int64)
    var failureCode: Int64? {
        switch self {
        case .failed(let code): return code
        case .blobReads(let reads):
            return reads.compactMap { if case .failed(_, let code) = $0 { code } else { nil } }.first
        default: return nil
        }
    }
    var raw: MeguriCoreBindings.CloudMediaReply {
        switch self {
        case .blobReads(let results): .blobReads(results: results.map(\.raw))
        case .record(let name, let type, let envelope, let asset): .record(name: name, recordType: type, envelope: envelope, assetPath: asset?.url.path)
        case .unknownItem: .unknownItem
        case .zoneMissing: .zoneMissing
        case .failed(let code): .failed(code: code)
        }
    }
}
public struct CloudMediaTransportReply: Sendable {
    public let result: CloudMediaReadResult
    public let retryAfterMilliseconds: UInt64?
    public let operationFailureCode: Int64?
    public init(result: CloudMediaReadResult, retryAfterMilliseconds: UInt64? = nil, operationFailureCode: Int64? = nil) {
        self.result = result; self.retryAfterMilliseconds = retryAfterMilliseconds
        self.operationFailureCode = operationFailureCode
    }
    func failure(underlying: any Error = CloudMediaFailure.failedRead) -> CloudKitSDKOperationFailure? {
        guard let code = result.failureCode ?? operationFailureCode else { return nil }
        return .init(underlying: underlying, code: code,
                     retryAfterMilliseconds: retryAfterMilliseconds.flatMap { Int64(exactly: $0) })
    }
}
public protocol CloudMediaQueue: Sendable { func enqueue() -> Bool }
final class ForeignMediaQueue: MeguriCoreBindings.CloudMediaQueue {
    let queue: any CloudMediaQueue
    init(_ queue: any CloudMediaQueue) { self.queue = queue }
    func enqueue() -> Bool { queue.enqueue() }
}
public actor CoreCloudMediaService {
    private let raw: MeguriCoreBindings.CloudMedia
    init(_ raw: MeguriCoreBindings.CloudMedia) { self.raw = raw }
    static let selectors: [BridgeCall] = [.cloudWorkflowMedia, .cloudWorkflowSubmitMediaBatch, .cloudMediaDownloadsBlocked, .cloudMediaNextWork, .cloudMediaSubmitRead, .cloudMediaQueueUpload, .cloudMediaReply]
    nonisolated static func call<T: Sendable>(_ name: BridgeCall, _ body: @Sendable @escaping () throws -> T) async throws -> T {
        try await MaintenanceLane.shared.call(name, body)
    }
    public func nextWork(sdk: String) async throws -> CloudMediaIssuedWork? {
        let raw = self.raw
        return try await Self.call(.cloudMediaNextWork) { try raw.nextWork(sdk: sdk).map(CloudMediaIssuedWork.init) }
    }
    public func downloadsBlocked(sdk: String) async throws -> Bool {
        let raw = self.raw
        return try await Self.call(.cloudMediaDownloadsBlocked) { try raw.downloadsBlocked(sdk: sdk) }
    }
    public func submitRead(_ work: CloudMediaWork, queue: any CloudMediaQueue) async throws -> Bool {
        let raw = self.raw
        return try await Self.call(.cloudMediaSubmitRead) { try raw.submitRead(work: work.raw, queue: ForeignMediaQueue(queue)) }
    }
    public func queueUpload(_ work: CloudMediaWork) async throws {
        let raw = self.raw
        try await Self.call(.cloudMediaQueueUpload) { try raw.queueUpload(work: work.raw) }
    }
    public func reply(_ work: CloudMediaWork, result: CloudMediaReadResult) async throws {
        let raw = self.raw
        try await Self.call(.cloudMediaReply) { try raw.reply(work: work.raw, reply: result.raw) }
    }
}

public protocol CloudPreparedMediaRead: CloudMediaQueue {
    func reply() async throws -> CloudMediaTransportReply
    func cancel()
}
public protocol CloudMediaTransport: Sendable {
    func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead
}
public enum CloudMediaProgress: Sendable {
    case idle
    case engineSendRequired
    case waitingForCollectionDrain
    case read(CloudMediaTransportReply)
}
public actor CloudMediaExecutor {
    private let core: CoreCloudMediaService
    private let sdk: String
    private let transport: any CloudMediaTransport
    private var active: Task<CloudMediaProgress, any Error>?
    private var prepared: (any CloudPreparedMediaRead)?
    private var stopping = false
    private var queued: Set<String> = []
    public func beginRound() { queued.removeAll() }

    public init(core: CoreCloudMediaService, sdk: String, transport: any CloudMediaTransport) {
        self.core = core; self.sdk = sdk; self.transport = transport
    }
    public func runNext(metadataOnly: Bool = false) async throws -> CloudMediaProgress {
        guard !stopping else { throw CloudMediaFailure.closed }
        guard active == nil else { throw CloudMediaFailure.busy }
        let task = Task { try await self.execute(metadataOnly: metadataOnly) }
        active = task
        defer { active = nil; prepared = nil }
        return try await withTaskCancellationHandler { try await task.value } onCancel: { task.cancel() }
    }
    private func execute(metadataOnly: Bool) async throws -> CloudMediaProgress {
        guard let work = try await core.nextWork(sdk: sdk) else {
            if metadataOnly { return .idle }
            return try await core.downloadsBlocked(sdk: sdk) ? .waitingForCollectionDrain : .idle
        }
        try Task.checkCancellation()
        guard !stopping else { throw CloudMediaFailure.closed }
        if metadataOnly {
            switch work.request {
            case .read, .reads: break
            default: throw CloudMediaFailure.metadataScope
            }
        }
        if case .upload(let name, _, _) = work.request {
            guard queued.insert(name).inserted else { return .idle }
            try await core.queueUpload(work.handle)
            return .engineSendRequired
        }
        if case .download = work.request, try await core.downloadsBlocked(sdk: sdk) { return .waitingForCollectionDrain }
        let operation = try transport.prepare(work.request)
        prepared = operation
        do {
            guard try await core.submitRead(work.handle, queue: operation) else {
                operation.cancel()
                if case .download = work.request, try await core.downloadsBlocked(sdk: sdk) { return .waitingForCollectionDrain }
                throw CloudMediaFailure.queueRejected
            }
        } catch { operation.cancel(); throw error }
        let reply = try await withTaskCancellationHandler { try await operation.reply() } onCancel: { operation.cancel() }
        try Task.checkCancellation()
        do { try await core.reply(work.handle, result: reply.result) }
        catch {
            if let failure = reply.failure(underlying: error) { throw failure }
            throw error
        }
        if let failure = reply.failure() { throw failure }
        let outcome: CloudMediaReadResult
        if case .record(let name, let type, let envelope, _) = reply.result {
            outcome = .record(name: name, recordType: type, envelope: envelope, asset: nil)
        } else { outcome = reply.result }
        return .read(.init(result: outcome, retryAfterMilliseconds: reply.retryAfterMilliseconds))
    }
    public func close() async {
        stopping = true
        prepared?.cancel()
        active?.cancel()
        if let active { _ = await active.result }
    }
}

final class CloudMediaBatchAdmission: Sendable {
    private let submit: @Sendable (any CloudMediaQueue) async throws -> Bool
    init(_ submit: @escaping @Sendable (any CloudMediaQueue) async throws -> Bool) { self.submit = submit }
    func deliver<Value: Sendable>(_ value: Value, commit: @escaping @Sendable (@Sendable () -> Bool) throws -> Bool = { $0() }) async throws -> Value? {
        try await withCheckedThrowingContinuation { continuation in
            let delivery = CloudMediaBatchDelivery(value: value, continuation: continuation, commit: commit)
            Task {
                do {
                    if try await submit(delivery) == false { delivery.finish(.success(nil)) }
                } catch { delivery.finish(.failure(error)) }
            }
        }
    }
}
private final class CloudMediaBatchDelivery<Value: Sendable>: CloudMediaQueue {
    private let continuation: Mutex<CheckedContinuation<Value?, any Error>?>
    private let value: Value
    private let commit: @Sendable (@Sendable () -> Bool) throws -> Bool
    init(value: Value, continuation: CheckedContinuation<Value?, any Error>, commit: @escaping @Sendable (@Sendable () -> Bool) throws -> Bool) {
        self.value = value; self.continuation = Mutex(continuation); self.commit = commit
    }
    func enqueue() -> Bool {
        do { return try commit { self.finish(.success(self.value)) } }
        catch { finish(.failure(error)); return false }
    }
    @discardableResult func finish(_ result: Result<Value?, any Error>) -> Bool {
        let original = continuation.withLock { value in let original = value; value = nil; return original }
        guard let original else { return false }
        original.resume(with: result)
        return true
    }
}
