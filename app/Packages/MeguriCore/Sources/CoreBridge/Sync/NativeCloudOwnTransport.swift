#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization

/// Shared opaque SDK archive conversion. Restoring fields validates the entire
/// native identity before setting encrypted bytes or admitting an operation.
enum NativeCloudKitSystemFields {
    static func encode(_ record: CKRecord) throws -> Data {
        let archive = NSKeyedArchiver(requiringSecureCoding: true)
        record.encodeSystemFields(with: archive); archive.finishEncoding()
        return archive.encodedData
    }
    static func restore(_ fields: Data, target: CloudControlTarget) throws -> CKRecord {
        guard !fields.isEmpty else { throw NativeCloudKitFailure.invalidSystemFields }
        let decoder = try NSKeyedUnarchiver(forReadingFrom: fields)
        decoder.requiresSecureCoding = true
        decoder.decodingFailurePolicy = .setErrorAndReturn
        defer { decoder.finishDecoding() }
        guard let record = CKRecord(coder: decoder), decoder.error == nil, record.recordID == NativeCloudOwnRecords.id(target), record.recordType == target.recordType else { throw NativeCloudKitFailure.invalidSystemFields }
        try NativeCloudKitIdentityValidation.requireCurrentOwner(record.recordID.zoneID)
        return record
    }
}
enum NativeCloudOwnRecords {
    static func zone(_ name: String) -> CKRecordZone.ID { .init(zoneName: name, ownerName: CKCurrentUserDefaultName) }
    static func id(_ target: CloudControlTarget) -> CKRecord.ID { .init(recordName: target.name, zoneID: zone(target.zone)) }
    static func requireNoAssets(_ record: CKRecord) throws {
        guard record["pa"] == nil, record["ma"] == nil, record["a"] == nil,
              !record.allKeys().contains(where: { record[$0] is CKAsset }) else { throw NativeCloudKitFailure.unsupportedAsset }
    }
    static func control(_ record: CKRecord, target: CloudControlTarget) throws -> CloudControlRecord {
        try NativeCloudKitIdentityValidation.requireCurrentOwner(record.recordID.zoneID)
        guard record.recordID == id(target), record.recordType == target.recordType else { throw NativeCloudKitFailure.unsupportedRecord }
        try requireNoAssets(record)
        let value = record.encryptedValues["e"]
        if let value, !(value is Data) { throw NativeCloudKitFailure.unsupportedRecord }
        return CloudControlRecord(encrypted: value as? Data, systemFields: try NativeCloudKitSystemFields.encode(record))
    }
    static func save(target: CloudControlTarget, encrypted: Data, systemFields: Data?) throws -> CKRecord {
        let record: CKRecord
        if let systemFields { record = try NativeCloudKitSystemFields.restore(systemFields, target: target) }
        else { record = CKRecord(recordType: target.recordType, recordID: id(target)) }
        record.encryptedValues["e"] = encrypted as CKRecordValue
        return record
    }
    /// CKError reports seconds. Preserve a duration only; Rust owns the clock,
    /// persisted deadline and later trigger admission. Partial errors can carry
    /// a longer delay than their enclosing operation.
    static func retryDuration(_ error: any Error) -> UInt64? {
        let error = error as NSError
        guard error.domain == CKErrorDomain else { return nil }
        var duration: UInt64?
        if let seconds = (error.userInfo[CKErrorRetryAfterKey] as? NSNumber)?.doubleValue, seconds.isFinite, seconds >= 0 {
            let milliseconds = (seconds * 1_000).rounded(.up)
            duration = milliseconds >= Double(UInt64.max) ? UInt64.max : UInt64(milliseconds)
        }
        // SDK callbacks bridge errors through NSError. Read the documented
        // dictionary without requiring its values to retain a Swift CKError
        // dictionary type; each nested error still needs the CloudKit domain.
        if let partials = error.userInfo[CKPartialErrorsByItemIDKey] as? [AnyHashable: Any] {
            for partial in partials.values {
                guard let partial = partial as? any Error else { continue }
                if let delay = retryDuration(partial) { duration = max(duration ?? 0, delay) }
            }
        }
        return duration
    }
    static func retryDuration<Success, Failure: Error>(from result: Result<Success, Failure>) -> UInt64? {
        if case .failure(let error) = result { return retryDuration(error) }
        return nil
    }
    static func maximumDelay(_ first: UInt64?, _ second: UInt64?) -> UInt64? {
        switch (first, second) { case (nil, nil): nil; default: max(first ?? 0, second ?? 0) }
    }
    static func assetReadFailure(_ error: any Error, perRecord: CloudAssetReadFailureFacts? = nil) -> CloudAssetReadResult {
        let native = error as NSError
        // A single-record partial wrapper describes the per-record cause.
        // Other terminal errors retain their own actual SDK code.
        let code = native.domain == CKErrorDomain && native.code == CKError.partialFailure.rawValue && perRecord != nil
            ? perRecord?.code : (native.domain == CKErrorDomain ? Int64(native.code) : nil)
        let delay = maximumDelay(perRecord?.retryAfterMilliseconds.flatMap { UInt64(exactly: $0) }, retryDuration(error))
        return .nativeFailure(code: code, retryAfterMilliseconds: delay.map { Int64(clamping: $0) })
    }
    static func readError(_ error: any Error) -> CloudControlRead {
        guard let error = error as? CKError else { return .failed }
        switch error.code { case .unknownItem: return .missing; case .zoneNotFound: return .zoneMissing; default: return .failed }
    }
    static func readResult(_ result: Result<CKRecord, any Error>, target: CloudControlTarget) throws -> CloudControlRead {
        switch result { case .success(let r): return .found(try control(r, target: target)); case .failure(let e): return readError(e) }
    }
    static func saveResult(_ result: Result<CKRecord, any Error>, target: CloudControlTarget) throws -> CloudPublicationResult {
        switch result {
        case .success(let r): return .saved(try control(r, target: target))
        case .failure(let e):
            if let error = e as? CKError, error.code == .serverRecordChanged, let record = error.serverRecord {
                return .conflict(try control(record, target: target))
            }
            if let duration = retryDuration(e) { return .retryAfter(durationMilliseconds: duration) }
            return .failed(retryAfterMilliseconds: nil)
        }
    }
}

/// Adoption's MGEntity projection owns downloaded pa bytes. Control operations
/// keep using the asset-free projection above; Swift never parses the payload.
enum NativeCloudStagingRecords {
    // A page owns every pa until the original operation replies. Size the SDK
    // request for the worst case: one maximum-size asset per changed record.
    static let pageLimit = Int(CloudMediaStaging.stagedLimit / CloudMediaStaging.assetLimit)

    static func project(_ record: CKRecord, target: CloudControlTarget, staging: CloudMediaStaging,
                        cancelled: @Sendable () -> Bool) throws -> CloudStagingRecord {
        try NativeCloudKitIdentityValidation.requireCurrentOwner(record.recordID.zoneID)
        guard record.recordID == NativeCloudOwnRecords.id(target), record.recordType == target.recordType else { throw NativeCloudKitFailure.unsupportedRecord }
        if record.recordType == "MGEpoch", target.name == "epoch", target.zone.hasPrefix("c-") {
            return .init(name: target.name, recordType: target.recordType, record: try NativeCloudOwnRecords.control(record, target: target))
        }
        guard record.recordType == "MGEntity", target.zone.hasPrefix("c-") else { throw NativeCloudKitFailure.unsupportedRecord }
        let metadata = try NativeCloudRecordConversion.decode(record)
        var lease: CloudMediaStagedAsset?
        if let asset = record["pa"] as? CKAsset {
            guard let source = asset.fileURL, source.isFileURL else { throw CloudMediaSourceFailure.invalidShape }
            lease = try staging.copyPayload(source, cancelled: cancelled, retainedAssetLimit: Int.max)
        }
        return .init(name: target.name, recordType: target.recordType,
                     record: .init(encrypted: metadata.envelope, systemFields: try NativeCloudKitSystemFields.encode(record)),
                     payloadLease: lease)
    }
}

protocol NativeCloudDatabaseQueue: Sendable { func add(_ operation: CKDatabaseOperation) -> Bool }
private struct PrivateCloudDatabaseQueue: NativeCloudDatabaseQueue {
    let database: CKDatabase
    func add(_ operation: CKDatabaseOperation) -> Bool { database.add(operation); return true }
}

/// The original CK callback is buffered until the guarded Rust call returns.
/// A cancelled pre-enqueue operation refuses queue acceptance synchronously.
final class NativePreparedOwnOperation: CloudPreparedOwnOperation, Sendable {
    private struct State { var queued = false; var cancelled = false; var finished = false }
    private let state = Mutex(State())
    private let readEvidence = Mutex<CloudZoneDeletionRead?>(nil)
    func controlReadEvidence() -> CloudZoneDeletionRead? { readEvidence.withLock { $0 } }
    func recordControlReadEvidence(_ value: CloudZoneDeletionRead) { readEvidence.withLock { $0 = value } }
    let operation: CKDatabaseOperation
    private let queue: any NativeCloudDatabaseQueue
    private let stream: AsyncStream<Result<CloudOwnTransportReply, any Error>>
    private let continuation: AsyncStream<Result<CloudOwnTransportReply, any Error>>.Continuation
    private let originalReplyWaiting: (@Sendable () -> Void)?
    init(operation: CKDatabaseOperation, queue: any NativeCloudDatabaseQueue,
         originalReplyWaiting: (@Sendable () -> Void)? = nil) {
        self.operation = operation; self.queue = queue
        self.originalReplyWaiting = originalReplyWaiting
        (stream, continuation) = AsyncStream.makeStream(bufferingPolicy: .bufferingNewest(1))
    }
    var cancelled: Bool { state.withLock { $0.cancelled } }
    func enqueue() -> Bool {
        let allowed = state.withLock { state in
            guard !state.queued, !state.cancelled else { return false }
            state.queued = true
            return true
        }
        guard allowed else { return false }
        return queue.add(operation)
    }
    func cancel() {
        state.withLock { $0.cancelled = true }
        operation.cancel()
    }
    func finish(_ reply: Result<CloudOwnTransportReply, any Error>) {
        let first = state.withLock { state in
            guard !state.finished else { return false }
            state.finished = true; return true
        }
        if first { continuation.yield(reply); continuation.finish() }
    }
    func reply() async throws -> CloudOwnTransportReply {
        let stream = self.stream
        let originalReplyWaiting = self.originalReplyWaiting
        // Join the original callback even when the waiting task is cancelled.
        return try await Task.detached {
            var iterator = stream.makeAsyncIterator()
            originalReplyWaiting?()
            guard let reply = await iterator.next() else { throw CloudOwnTransportFailure.missingReply }
            return try reply.get()
        }.value
    }
}

public struct NativeCloudOwnTransport: CloudOwnTransport {
    let queue: any NativeCloudDatabaseQueue
    private let staging: CloudMediaStaging?
    let zoneDeletionTerminalHandlerInstalled: (@Sendable (@escaping @Sendable (Result<Void, any Error>) -> Void) -> Void)?
    let zoneInventoryPerZoneHandlerInstalled: (@Sendable (@escaping @Sendable (CKRecordZone.ID, Result<CKRecordZone, any Error>) -> Void) -> Void)?
    let zoneInventoryTerminalHandlerInstalled: (@Sendable (@escaping @Sendable (Result<Void, any Error>) -> Void) -> Void)?
    let zoneInventoryOriginalReplyWaiting: (@Sendable () -> Void)?
    public init(database: CKDatabase) throws {
        guard database.databaseScope == .private else { throw CloudOwnTransportFailure.privateDatabaseRequired }
        queue = PrivateCloudDatabaseQueue(database: database)
        staging = try CloudMediaStaging(directory: FileManager.default.temporaryDirectory.appendingPathComponent("meguri-cloud-adoption", isDirectory: true))
        zoneDeletionTerminalHandlerInstalled = nil
        zoneInventoryPerZoneHandlerInstalled = nil
        zoneInventoryTerminalHandlerInstalled = nil
        zoneInventoryOriginalReplyWaiting = nil
    }
    public init(database: CKDatabase, staging: CloudMediaStaging) throws {
        guard database.databaseScope == .private else { throw CloudOwnTransportFailure.privateDatabaseRequired }
        queue = PrivateCloudDatabaseQueue(database: database)
        self.staging = staging
        zoneDeletionTerminalHandlerInstalled = nil
        zoneInventoryPerZoneHandlerInstalled = nil
        zoneInventoryTerminalHandlerInstalled = nil
        zoneInventoryOriginalReplyWaiting = nil
    }
    init(queue: any NativeCloudDatabaseQueue, staging: CloudMediaStaging? = nil,
         zoneDeletionTerminalHandlerInstalled: (@Sendable (@escaping @Sendable (Result<Void, any Error>) -> Void) -> Void)? = nil,
         zoneInventoryPerZoneHandlerInstalled: (@Sendable (@escaping @Sendable (CKRecordZone.ID, Result<CKRecordZone, any Error>) -> Void) -> Void)? = nil,
         zoneInventoryTerminalHandlerInstalled: (@Sendable (@escaping @Sendable (Result<Void, any Error>) -> Void) -> Void)? = nil,
         zoneInventoryOriginalReplyWaiting: (@Sendable () -> Void)? = nil) {
        self.queue = queue; self.staging = staging
        self.zoneDeletionTerminalHandlerInstalled = zoneDeletionTerminalHandlerInstalled
        self.zoneInventoryPerZoneHandlerInstalled = zoneInventoryPerZoneHandlerInstalled
        self.zoneInventoryTerminalHandlerInstalled = zoneInventoryTerminalHandlerInstalled
        self.zoneInventoryOriginalReplyWaiting = zoneInventoryOriginalReplyWaiting
    }
    public func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        switch command {
        case .zoneDeletionActiveRead(let target): return try prepareZoneDeletionRead(target)
        case .deleteZones(let batch): return try prepareZoneDeletion(batch)
        case .zoneInventory: return prepareZoneInventory()
        case .read(let target): return read(target, publication: false)
        case .assetRead(let zone, let name): return try assetRead(zone: zone, name: name)
        case .publication(.read(let target)): return read(target, publication: true)
        case .publication(.createZones(let zones)): return createZones(zones)
        case .publication(.save(let target, let encrypted, let fields)): return try save(target, encrypted: encrypted, fields: fields)
        case .adoptionPage(let zone, let cursor): return try page(zone: zone, cursor: cursor)
        }
    }
    private func read(_ target: CloudControlTarget, publication: Bool) -> NativePreparedOwnOperation {
        let id = NativeCloudOwnRecords.id(target)
        let operation = CKFetchRecordsOperation(recordIDs: [id])
        operation.desiredKeys = nil // Include encrypted e and every asset field.
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue)
        let record = Mutex<Result<CKRecord, any Error>?>(nil)
        operation.perRecordResultBlock = { received, result in
            record.withLock { value in
                if received == id { value = result }
                else { value = .failure(NativeCloudKitFailure.unsupportedRecord) }
            }
        }
        operation.fetchRecordsResultBlock = { [weak prepared] result in
            guard let prepared else { return }
            do {
                let received = record.withLock { $0 }
                let duration = NativeCloudOwnRecords.maximumDelay(received.flatMap { NativeCloudOwnRecords.retryDuration(from: $0) }, NativeCloudOwnRecords.retryDuration(from: result))
                if publication, let duration {
                    prepared.finish(.success(.publication(.retryAfter(durationMilliseconds: duration))))
                    return
                }
                let read: CloudControlRead
                if let received { read = try NativeCloudOwnRecords.readResult(received, target: target) }
                else if case .failure(let error) = result { read = NativeCloudOwnRecords.readError(error) }
                else { throw CloudOwnTransportFailure.missingReply }
                if !publication {
                    let failure: CloudZoneDeletionFailure?
                    if case .failure(let error)? = received { failure = .evidence(error) } else { failure = nil }
                    let terminal: CloudZoneDeletionFailure?
                    if case .failure(let error) = result { terminal = .evidence(error) } else { terminal = nil }
                    prepared.recordControlReadEvidence(.init(read: read, failure: failure, terminal: terminal))
                }
                prepared.finish(.success(publication ? .publication(.read(read)) : .read(read)))
            } catch {
                prepared.recordControlReadEvidence(.init(read: .failed, terminal: .evidence(error)))
                prepared.finish(.failure(error))
            }
        }
        return prepared
    }
    private func assetRead(zone: String, name: String) throws -> NativePreparedOwnOperation {
        guard zone.hasPrefix("c-") else { throw NativeCloudKitFailure.unsupportedRecord }
        let target = CloudControlTarget(zone: zone, name: name, recordType: "MGEntity")
        let id = NativeCloudOwnRecords.id(target)
        let operation = CKFetchRecordsOperation(recordIDs: [id])
        operation.desiredKeys = nil // All fields, including encrypted e and pa.
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue)
        let staging = try self.staging ?? CloudMediaStaging(directory: FileManager.default.temporaryDirectory.appendingPathComponent("meguri-cloud-adoption", isDirectory: true))
        struct Read {
            var received = false
            var result: CloudAssetReadResult?
            var failure: (any Error)?
            var finished = false
        }
        let state = Mutex(Read())
        operation.perRecordResultBlock = { [weak prepared] received, result in
            // Serialize projection with terminal delivery so the SDK file is
            // copied before this callback returns, and no copy escapes a join.
            state.withLock { value in
                guard !value.finished else { return }
                guard !value.received else {
                    value.failure = NativeCloudKitFailure.unsupportedRecord
                    return // Retain the first copy until the original terminal.
                }
                value.received = true
                do {
                    guard let prepared, !prepared.cancelled else { throw CancellationError() }
                    guard received == id else { throw NativeCloudKitFailure.unsupportedRecord }
                    switch result {
                    case .failure(let error): value.result = NativeCloudOwnRecords.assetReadFailure(error)
                    case .success(let native):
                        try NativeCloudKitIdentityValidation.requireCurrentOwner(native.recordID.zoneID)
                        guard native.recordID == id, native.recordType == target.recordType else { throw NativeCloudKitFailure.unsupportedRecord }
                        let metadata = try NativeCloudRecordConversion.decode(native)
                        var lease: CloudMediaStagedAsset?
                        if let asset = native["pa"] as? CKAsset {
                            do {
                                guard let source = asset.fileURL, source.isFileURL else { throw CloudMediaSourceFailure.invalidShape }
                                lease = try staging.copyPayload(source, cancelled: { prepared.cancelled })
                            } catch is CloudMediaSourceFailure {
                                // Core intake still needs this version to retry its payload.
                            } catch CloudMediaFailure.stagingLimit {
                                // Keep metadata when the native copy budget is full.
                            }
                        }
                        let record = CloudRecord(name: metadata.name, envelope: metadata.envelope, systemFields: metadata.systemFields,
                                                 recordType: metadata.recordType, zone: metadata.zone, assetURL: lease?.url)
                        value.result = try CloudAssetReadResult(record: record, asset: lease)
                    }
                } catch { value.failure = error }
            }
        }
        operation.fetchRecordsResultBlock = { [weak prepared] result in
            let read = state.withLock { value in
                let read = value
                value.result = nil; value.finished = true
                return read
            }
            guard let prepared else { return }
            if prepared.cancelled { prepared.finish(.failure(CancellationError())); return }
            if let error = read.failure { prepared.finish(.failure(error)); return }
            if case .failure(let error) = result {
                prepared.finish(.success(.assetRead(NativeCloudOwnRecords.assetReadFailure(error, perRecord: read.result?.failureFacts))))
                return
            }
            guard let result = read.result else { prepared.finish(.failure(CloudOwnTransportFailure.missingReply)); return }
            prepared.finish(.success(.assetRead(result)))
        }
        return prepared
    }
    private func createZones(_ names: [String]) -> NativePreparedOwnOperation {
        let ids = Set(names.map(NativeCloudOwnRecords.zone))
        let operation = CKModifyRecordZonesOperation(recordZonesToSave: names.map { CKRecordZone(zoneID: NativeCloudOwnRecords.zone($0)) }, recordZoneIDsToDelete: nil)
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue)
        struct State { var saved: Set<CKRecordZone.ID> = []; var failed = false; var retryDuration: UInt64? }
        let state = Mutex(State())
        operation.perRecordZoneSaveBlock = { id, result in
            state.withLock { value in
                switch result {
                case .success(let zone):
                    if ids.contains(id), zone.zoneID == id, id.ownerName == CKCurrentUserDefaultName { value.saved.insert(id) }
                    else { value.failed = true }
                case .failure(let error):
                    value.failed = true
                    value.retryDuration = NativeCloudOwnRecords.maximumDelay(value.retryDuration, NativeCloudOwnRecords.retryDuration(error))
                }
            }
        }
        operation.modifyRecordZonesResultBlock = { [weak prepared] result in
            guard let prepared else { return }
            let outcome = state.withLock { $0 }
            if case .success = result, !outcome.failed, outcome.saved == ids { prepared.finish(.success(.publication(.zonesCreated))) }
            else if let duration = NativeCloudOwnRecords.maximumDelay(outcome.retryDuration, NativeCloudOwnRecords.retryDuration(from: result)) {
                prepared.finish(.success(.publication(.retryAfter(durationMilliseconds: duration))))
            } else { prepared.finish(.success(.publication(.failed(retryAfterMilliseconds: nil)))) }
        }
        return prepared
    }
    private func save(_ target: CloudControlTarget, encrypted: Data, fields: Data?) throws -> NativePreparedOwnOperation {
        let native = try NativeCloudOwnRecords.save(target: target, encrypted: encrypted, systemFields: fields)
        let operation = CKModifyRecordsOperation(recordsToSave: [native], recordIDsToDelete: nil)
        operation.savePolicy = .ifServerRecordUnchanged; operation.isAtomic = true
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue)
        let record = Mutex<Result<CKRecord, any Error>?>(nil)
        operation.perRecordSaveBlock = { id, result in
            record.withLock { $0 = id == native.recordID ? result : .failure(NativeCloudKitFailure.unsupportedRecord) }
        }
        operation.modifyRecordsResultBlock = { [weak prepared] result in
            guard let prepared else { return }
            do {
                let received = record.withLock { $0 }
                if let duration = NativeCloudOwnRecords.maximumDelay(received.flatMap { NativeCloudOwnRecords.retryDuration(from: $0) }, NativeCloudOwnRecords.retryDuration(from: result)) {
                    prepared.finish(.success(.publication(.retryAfter(durationMilliseconds: duration))))
                } else if let received {
                    prepared.finish(.success(.publication(try NativeCloudOwnRecords.saveResult(received, target: target))))
                } else if case .failure(let error) = result {
                    prepared.finish(.success(.publication(try NativeCloudOwnRecords.saveResult(.failure(error), target: target))))
                } else { prepared.finish(.failure(CloudOwnTransportFailure.missingReply)) }
            } catch { prepared.finish(.failure(error)) }
        }
        return prepared
    }
    private func page(zone: String, cursor: Data?) throws -> NativePreparedOwnOperation {
        let id = NativeCloudOwnRecords.zone(zone)
        let token: CKServerChangeToken?
        if let cursor {
            do {
                guard let decoded = try NSKeyedUnarchiver.unarchivedObject(ofClass: CKServerChangeToken.self, from: cursor) else { throw CloudOwnTransportFailure.invalidCursor }
                token = decoded
            } catch { throw CloudOwnTransportFailure.invalidCursor }
        } else { token = nil }
        let configuration = CKFetchRecordZoneChangesOperation.ZoneConfiguration()
        configuration.previousServerChangeToken = token; configuration.desiredKeys = nil
        configuration.resultsLimit = NativeCloudStagingRecords.pageLimit
        let operation = CKFetchRecordZoneChangesOperation(recordZoneIDs: [id], configurationsByRecordZoneID: [id: configuration])
        operation.configuration.qualityOfService = .userInitiated
        operation.fetchAllChanges = false
        let prepared = NativePreparedOwnOperation(operation: operation, queue: queue)
        let staging = try self.staging ?? CloudMediaStaging(directory: FileManager.default.temporaryDirectory.appendingPathComponent("meguri-cloud-adoption", isDirectory: true))
        struct Page {
            var records: [CloudStagingRecord] = []
            var deleted: [String] = []
            var next: Data?
            var completed = false
            var failure: (any Error)?
            var finished = false
            var changes = 0
            mutating func acceptChange() -> Bool {
                guard !finished, failure == nil else { return false }
                guard changes < NativeCloudStagingRecords.pageLimit else {
                    failure = NativeCloudKitFailure.unsupportedRecord
                    return false
                }
                changes += 1
                return true
            }
        }
        let state = Mutex(Page())
        operation.recordWasChangedBlock = { [weak prepared] recordID, result in
            guard state.withLock({ $0.acceptChange() }) else { return }
            do {
                guard let prepared, !prepared.cancelled else { throw CancellationError() }
                guard recordID.zoneID == id else { throw NativeCloudKitFailure.unsupportedRecord }
                let record = try result.get()
                guard record.recordID == recordID else { throw NativeCloudKitFailure.unsupportedRecord }
                let target = CloudControlTarget(zone: zone, name: recordID.recordName, recordType: record.recordType)
                let value = try NativeCloudStagingRecords.project(record, target: target, staging: staging, cancelled: { prepared.cancelled })
                state.withLock { if !$0.finished { $0.records.append(value) } }
            } catch { state.withLock { if !$0.finished { $0.failure = error } } }
        }
        operation.recordWithIDWasDeletedBlock = { recordID, _ in
            state.withLock { value in
                guard value.acceptChange() else { return }
                if recordID.zoneID == id { value.deleted.append(recordID.recordName) }
                else { value.failure = NativeCloudKitFailure.unsupportedRecord }
            }
        }
        operation.recordZoneFetchResultBlock = { zoneID, result in
            do {
                guard zoneID == id else { throw NativeCloudKitFailure.unsupportedRecord }
                let page = try result.get()
                let next = page.moreComing ? try NSKeyedArchiver.archivedData(withRootObject: page.serverChangeToken, requiringSecureCoding: true) : nil
                state.withLock { $0.next = next; $0.completed = true }
            } catch { state.withLock { $0.failure = error } }
        }
        operation.fetchRecordZoneChangesResultBlock = { [weak prepared] result in
            let page = state.withLock { value in
                let page = value
                value.records.removeAll(); value.finished = true
                return page
            }
            guard let prepared else { return }
            if prepared.cancelled { prepared.finish(.failure(CancellationError())); return }
            if let error = page.failure { prepared.finish(.failure(error)); return }
            if case .failure(let error) = result { prepared.finish(.failure(error)); return }
            guard page.completed else { prepared.finish(.failure(CloudOwnTransportFailure.missingReply)); return }
            prepared.finish(.success(.page(records: page.records, deleted: page.deleted, next: page.next)))
        }
        return prepared
    }
}

extension NativeCloudKitEngineFactory {
    /// Both paths retain exactly this factory's private CKDatabase. This creates
    /// no CKSyncEngine and adds no pending database changes to an engine.
    public func makeOwnExecutor(core: CoreCloudConnectionService) throws -> CloudOwnExecutor {
        CloudOwnExecutor(core: core, transport: try NativeCloudOwnTransport(database: database))
    }
}
#endif
