#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization

enum NativeCloudMediaRecords {
    static func id(_ request: CloudMediaRequest) throws -> CKRecord.ID {
        let name: String
        switch request { case .root: name = "root"; case .reads: throw NativeCloudKitFailure.unsupportedRecord; case .read(let n), .download(let n), .delete(let n), .upload(let n, _, _): name = n }
        return .init(recordName: name, zoneID: NativeCloudOwnRecords.zone("meguri-media"))
    }
    static func isBlobName(_ name: String) -> Bool {
        let bytes = Array(name.utf8)
        return bytes.count == 27 && bytes.first == 98 && bytes.dropFirst().allSatisfy { (97...122).contains($0) || (50...55).contains($0) }
    }
    static func read(_ record: CKRecord, request: CloudMediaRequest, staging: CloudMediaStaging, cancelled: @Sendable () -> Bool) throws -> CloudMediaReadResult {
        try NativeCloudKitIdentityValidation.requireCurrentOwner(record.recordID.zoneID)
        let type: String
        switch request { case .root: type = "MGMediaRoot"; case .read, .download: type = "MGBlob"; case .upload, .delete: throw CloudMediaFailure.requiresSyncEngine; case .reads: throw NativeCloudKitFailure.unsupportedRecord }
        guard record.recordID == (try id(request)), record.recordType == type,
              record["pa"] == nil, record["ma"] == nil,
              let envelope = record.encryptedValues["e"] as? Data,
              envelope.count <= 1_000_000 else { throw NativeCloudKitFailure.unsupportedRecord }
        guard !record.allKeys().contains(where: { $0 != "a" && record[$0] is CKAsset }) else { throw NativeCloudKitFailure.unsupportedAsset }
        let asset: CloudMediaStagedAsset?
        if request.downloadsAsset {
            guard let file = (record["a"] as? CKAsset)?.fileURL else { throw NativeCloudKitFailure.unsupportedAsset }
            // CKAsset's URL is valid only during this original SDK callback.
            asset = try staging.copy(file, cancelled: cancelled)
        } else {
            guard record["a"] == nil else { throw NativeCloudKitFailure.unsupportedAsset }
            asset = nil
        }
        return .record(name: record.recordID.recordName, recordType: type, envelope: envelope, asset: asset)
    }
    static func missingItem(_ id: CKRecord.ID, terminal: Result<Void, any Error>) -> CloudMediaTransportReply {
        guard case .failure(let error) = terminal else { return .init(result: .failed(code: 0)) }
        let native = error as NSError
        let entries = native.userInfo[CKPartialErrorsByItemIDKey] as? [AnyHashable: Any]
        let item = entries?[AnyHashable(id)] as? any Error
        let reply = failure(item ?? error)
        return .init(result: reply.result, retryAfterMilliseconds: NativeCloudOwnRecords.maximumDelay(
            reply.retryAfterMilliseconds, NativeCloudOwnRecords.retryDuration(error)))
    }
    static func unresolvedOperationCode(_ terminal: Result<Void, any Error>) -> Int64? {
        guard case .failure(let error) = terminal else { return nil }
        let native = error as NSError
        if native.domain == CKErrorDomain && native.code == CKError.Code.partialFailure.rawValue { return nil }
        return failure(error).result.failureCode
    }
    static func failure(_ error: any Error) -> CloudMediaTransportReply {
        let native = error as NSError
        let result: CloudMediaReadResult
        if native.domain == CKErrorDomain, native.code == CKError.Code.unknownItem.rawValue { result = .unknownItem }
        else if native.domain == CKErrorDomain, native.code == CKError.Code.zoneNotFound.rawValue { result = .zoneMissing }
        else { result = .failed(code: native.domain == CKErrorDomain ? Int64(native.code) : 0) }
        return .init(result: result, retryAfterMilliseconds: NativeCloudOwnRecords.retryDuration(error))
    }
}

final class NativePreparedMediaRead: CloudPreparedMediaRead, Sendable {
    struct State { var queued = false; var cancelled = false; var finished = false }
    private let state = Mutex(State())
    let operation: CKFetchRecordsOperation
    private let queue: any NativeCloudDatabaseQueue
    private let stream: AsyncStream<CloudMediaTransportReply>
    private let continuation: AsyncStream<CloudMediaTransportReply>.Continuation
    init(operation: CKFetchRecordsOperation, queue: any NativeCloudDatabaseQueue) {
        self.operation = operation; self.queue = queue
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    var cancelled: Bool { state.withLock { $0.cancelled } }
    func enqueue() -> Bool {
        let allowed = state.withLock { value in
            guard !value.queued, !value.cancelled else { return false }
            value.queued = true; return true
        }
        return allowed && queue.add(operation)
    }
    func cancel() { state.withLock { $0.cancelled = true }; operation.cancel() }
    func finish(_ reply: CloudMediaTransportReply) {
        let first = state.withLock { value in
            guard !value.finished else { return false }
            value.finished = true; return true
        }
        if first { continuation.yield(reply); continuation.finish() }
    }
    func reply() async throws -> CloudMediaTransportReply {
        let stream = self.stream
        // AsyncStream's iterator observes task cancellation. A separate join
        // task keeps waiting for the original SDK callback when its caller is
        // cancelled, so the prepared operation and staging cannot escape drain.
        return try await Task.detached {
            var iterator = stream.makeAsyncIterator()
            guard let reply = await iterator.next() else { throw CloudMediaFailure.missingReply }
            return reply
        }.value
    }
}

private struct PrivateMediaDatabaseQueue: NativeCloudDatabaseQueue {
    let database: CKDatabase
    func add(_ operation: CKDatabaseOperation) -> Bool { database.add(operation); return true }
}
public struct NativeCloudMediaTransport: CloudMediaTransport {
    private let queue: any NativeCloudDatabaseQueue
    private let staging: CloudMediaStaging
    private let terminalHandlerInstalled: (@Sendable (@escaping @Sendable (Result<Void, any Error>) -> Void) -> Void)?
    public init(database: CKDatabase, staging: CloudMediaStaging) throws {
        guard database.databaseScope == .private else { throw CloudOwnTransportFailure.privateDatabaseRequired }
        queue = PrivateMediaDatabaseQueue(database: database); self.staging = staging
        terminalHandlerInstalled = nil
    }
    init(queue: any NativeCloudDatabaseQueue, staging: CloudMediaStaging,
         terminalHandlerInstalled: (@Sendable (@escaping @Sendable (Result<Void, any Error>) -> Void) -> Void)? = nil) {
        self.queue = queue; self.staging = staging; self.terminalHandlerInstalled = terminalHandlerInstalled
    }
    public func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead {
        switch request { case .upload, .delete: throw CloudMediaFailure.requiresSyncEngine; default: break }
        if case .reads(let names) = request { return try prepareBlobReads(names) }
        let id = try NativeCloudMediaRecords.id(request)
        if case .root = request {} else {
            guard NativeCloudMediaRecords.isBlobName(id.recordName) else { throw NativeCloudKitFailure.unsupportedRecord }
        }
        let operation = CKFetchRecordsOperation(recordIDs: [id])
        operation.desiredKeys = request.downloadsAsset ? ["e", "a"] : ["e"]
        let prepared = NativePreparedMediaRead(operation: operation, queue: queue)
        let state = Mutex<CloudMediaTransportReply?>(nil)
        let staging = self.staging
        operation.perRecordResultBlock = { [weak prepared] actual, result in
            guard let prepared else { return }
            let reply: CloudMediaTransportReply
            do {
                guard actual == id else { throw NativeCloudKitFailure.unsupportedRecord }
                switch result {
                case .success(let record): reply = .init(result: try NativeCloudMediaRecords.read(record, request: request, staging: staging, cancelled: { prepared.cancelled }))
                case .failure(let error): reply = NativeCloudMediaRecords.failure(error)
                }
            } catch { reply = NativeCloudMediaRecords.failure(error) }
            state.withLock { value in
                // Two callbacks for one requested ID are never an empty/saved
                // receipt; replacing releases only this run's native staging.
                value = value == nil ? reply : .init(result: .failed(code: 0))
            }
        }
        let terminalHandler: @Sendable (Result<Void, any Error>) -> Void = { [weak prepared] terminal in
            guard let prepared else { return }
            let received = state.withLock { value in let result = value; value = nil; return result }
            let terminalDelay = NativeCloudOwnRecords.retryDuration(from: terminal)
            let retry = NativeCloudOwnRecords.maximumDelay(received?.retryAfterMilliseconds, terminalDelay)
            if let received {
                // A per-record unknownItem often accompanies partialFailure.
                // Preserve the actual item result and both retry durations.
                prepared.finish(.init(result: received.result, retryAfterMilliseconds: retry,
                                      operationFailureCode: NativeCloudMediaRecords.unresolvedOperationCode(terminal)))
            } else { prepared.finish(NativeCloudMediaRecords.missingItem(id, terminal: terminal)) }
        }
        terminalHandlerInstalled?(terminalHandler)
        operation.fetchRecordsResultBlock = terminalHandler
        return prepared
    }
    private func prepareBlobReads(_ names: [String]) throws -> any CloudPreparedMediaRead {
        guard !names.isEmpty, names.count <= 200, Set(names).count == names.count,
              names.allSatisfy(NativeCloudMediaRecords.isBlobName) else { throw NativeCloudKitFailure.unsupportedRecord }
        let ids = try names.map { try NativeCloudMediaRecords.id(.read(name: $0)) }
        let expected = Set(ids)
        let operation = CKFetchRecordsOperation(recordIDs: ids)
        operation.desiredKeys = ["e"]
        let prepared = NativePreparedMediaRead(operation: operation, queue: queue)
        let results = Mutex<[String: CloudMediaTransportReply]>([:])
        let invalid = Mutex(false)
        let staging = self.staging
        operation.perRecordResultBlock = { [weak prepared] id, result in
            guard let prepared else { return }
            let reply: CloudMediaTransportReply
            do {
                guard expected.contains(id) else { invalid.withLock { $0 = true }; throw NativeCloudKitFailure.unsupportedRecord }
                switch result {
                case .success(let record): reply = .init(result: try NativeCloudMediaRecords.read(record, request: .read(name: id.recordName), staging: staging, cancelled: { prepared.cancelled }))
                case .failure(let error): reply = NativeCloudMediaRecords.failure(error)
                }
            } catch { reply = NativeCloudMediaRecords.failure(error) }
            results.withLock { values in
                if values[id.recordName] != nil { invalid.withLock { $0 = true } }
                values[id.recordName] = values[id.recordName] == nil ? reply : .init(result: .failed(code: 0))
            }
        }
        let terminalHandler: @Sendable (Result<Void, any Error>) -> Void = { [weak prepared] terminal in
            guard let prepared else { return }
            let received = results.withLock { values in let received = values; values.removeAll(); return received }
            if invalid.withLock({ $0 }) { prepared.finish(.init(result: .failed(code: 0))); return }
            var retry = NativeCloudOwnRecords.retryDuration(from: terminal)
            var replies: [CloudMediaBlobReadResult] = []
            for name in names {
                let reply: CloudMediaTransportReply
                if let item = received[name] { reply = item }
                else { reply = NativeCloudMediaRecords.missingItem(ids[replies.count], terminal: terminal) }
                retry = NativeCloudOwnRecords.maximumDelay(retry, reply.retryAfterMilliseconds)
                do { replies.append(try .init(name: name, result: reply.result)) }
                catch { replies.append(.failed(name: name, code: 0)) }
            }
            prepared.finish(.init(result: .blobReads(replies), retryAfterMilliseconds: retry,
                                  operationFailureCode: NativeCloudMediaRecords.unresolvedOperationCode(terminal)))
        }
        terminalHandlerInstalled?(terminalHandler)
        operation.fetchRecordsResultBlock = terminalHandler
        return prepared
    }
}

extension NativeCloudKitEngineFactory {
    public func makeMediaExecutor(core: CoreCloudMediaService, sdk: String, staging: CloudMediaStaging) throws -> CloudMediaExecutor {
        CloudMediaExecutor(core: core, sdk: sdk, transport: try NativeCloudMediaTransport(database: database, staging: staging))
    }
}
#endif
