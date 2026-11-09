#if canImport(CloudKit)
import CloudKit
import Foundation
import Testing
import Synchronization
@testable import CoreBridge

// Capture before CloudKit's getter normalizes aggregate partial failures.
private final class NativeMediaTerminalCapture: Sendable {
    private let handler = Mutex<(@Sendable (Result<Void, any Error>) -> Void)?>(nil)
    func installed(_ callback: @escaping @Sendable (Result<Void, any Error>) -> Void) {
        handler.withLock { $0 = callback }
    }
    func complete(_ result: Result<Void, any Error>) throws {
        let callback = try #require(handler.withLock { $0 })
        callback(result)
    }
}

struct NativeCloudMediaTests {
    let name = "baaaaaaaaaaaaaaaaaaaaaaaaaa"

    #if DEBUG
    enum FailureScenario: CaseIterable, Sendable {
        case cloudKit, nonCloudKit, missingAsset, missingSource, invalidRecord
        case foreignCallback, duplicateCallback, missingCallback, unknownItem, zoneMissing
    }

    @Test(arguments: FailureScenario.allCases, [false, true])
    func debugMediaFailureKeepsOnlyFixedEvidence(scenario: FailureScenario, batch: Bool) async throws {
        let recorder = DebugCloudDiagnostics.Recorder()
        try await DebugCloudDiagnostics.$mediaRecorder.withValue(recorder) {
            let sentinel = "SYNTHETIC_PRIVATE_SENTINEL"
            let queue = NativeOwnCapturingQueue()
            let terminal = NativeMediaTerminalCapture()
            let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: root) }
            let transport = NativeCloudMediaTransport(queue: queue,
                staging: try .init(directory: root, available: { UInt64.max }),
                terminalHandlerInstalled: { terminal.installed($0) })
            let download = scenario == .missingAsset || scenario == .missingSource
            if batch && download { return }
            let request: CloudMediaRequest = download ? .download(name: name) : batch ? .reads(names: [name]) : .read(name: name)
            let prepared = try transport.prepare(request)
            #expect(prepared.enqueue())
            let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
            let id = try #require(operation.recordIDs?.first)
            let record = CKRecord(recordType: "MGBlob", recordID: id)
            record.encryptedValues["e"] = Data([1]) as CKRecordValue
            let expected: String?
            switch scenario {
            case .cloudKit:
                operation.perRecordResultBlock?(id, .failure(CKError(.serviceUnavailable,
                    userInfo: [NSLocalizedDescriptionKey: sentinel, CKErrorRetryAfterKey: 45.0])))
                expected = "mediaRequest=read mediaStage=recordCallback mediaCategory=cloudKit"
            case .nonCloudKit:
                operation.perRecordResultBlock?(id, .failure(NSError(domain: sentinel, code: 987,
                    userInfo: [NSLocalizedDescriptionKey: sentinel, NSFilePathErrorKey: sentinel])))
                expected = "mediaRequest=read mediaStage=recordCallback mediaCategory=nonCloudKit"
            case .missingAsset:
                operation.perRecordResultBlock?(id, .success(record))
                expected = "mediaRequest=download mediaStage=recordValidation mediaCategory=missingAsset"
            case .missingSource:
                record["a"] = CKAsset(fileURL: root.appendingPathComponent(sentinel))
                operation.perRecordResultBlock?(id, .success(record))
                expected = "mediaRequest=download mediaStage=assetStaging mediaCategory=sourceUnavailable"
            case .invalidRecord:
                record.encryptedValues["e"] = nil
                operation.perRecordResultBlock?(id, .success(record))
                expected = "mediaRequest=read mediaStage=recordValidation mediaCategory=invalidRecord"
            case .foreignCallback:
                operation.perRecordResultBlock?(.init(recordName: sentinel, zoneID: id.zoneID), .success(record))
                expected = "mediaRequest=read mediaStage=recordCallback mediaCategory=invalidRecord"
            case .duplicateCallback:
                operation.perRecordResultBlock?(id, .success(record))
                operation.perRecordResultBlock?(id, .success(record))
                expected = "mediaRequest=read mediaStage=recordCallback mediaCategory=duplicateCallback"
            case .missingCallback:
                expected = "mediaRequest=read mediaStage=terminalCallback mediaCategory=missingCallback"
            case .unknownItem:
                operation.perRecordResultBlock?(id, .failure(CKError(.unknownItem)))
                expected = nil
            case .zoneMissing:
                operation.perRecordResultBlock?(id, .failure(CKError(.zoneNotFound)))
                expected = nil
            }
            try terminal.complete(.success(()))
            let reply = try await prepared.reply()
            let expectedTuple = batch ? expected?.replacingOccurrences(of: "mediaRequest=read ", with: "mediaRequest=reads ") : expected
            #expect(recorder.latestValue == expectedTuple)
            #expect(!(recorder.latestValue ?? "").contains(sentinel))
            switch scenario {
            case .unknownItem:
                if batch {
                    guard case .blobReads(let results) = reply.result, let first = results.first, case .unknownItem = first else { Issue.record("absence changed"); return }
                } else {
                    guard case .unknownItem = reply.result else { Issue.record("absence changed"); return }
                }
                #expect(reply.failure() == nil)
            case .zoneMissing:
                if batch {
                    guard case .blobReads(let results) = reply.result, let first = results.first, case .zoneMissing = first else { Issue.record("absence changed"); return }
                } else {
                    guard case .zoneMissing = reply.result else { Issue.record("absence changed"); return }
                }
                #expect(reply.failure() == nil)
            case .cloudKit:
                #expect(reply.failure()?.code == Int64(CKError.Code.serviceUnavailable.rawValue))
                #expect(reply.failure()?.retryAfterMilliseconds == 45_000)
            default: #expect(reply.failure()?.code == 0)
            }
            let status = DebugCloudDiagnostics.Value.status(.other, zone: .media, kind: .read, code: 0)
            recorder.emitStatus(status) { _ in }
            #expect(recorder.latestValue == [status.formatted, expectedTuple].compactMap { $0 }.joined(separator: " "))
        }
    }
    #endif

    @Test func recordIDReadsNameExactlyTheRequiredFields() throws {
        for request in [CloudMediaRequest.root, .read(name: name), .download(name: name)] {
            let queue = NativeOwnCapturingQueue()
            let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
            defer { try? FileManager.default.removeItem(at: root) }
            let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root, available: { UInt64.max }))
            let prepared = try transport.prepare(request)
            #expect(prepared.enqueue())
            let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
            #expect(operation.desiredKeys == (request.downloadsAsset ? ["e", "a"] : ["e"]))
            #expect(operation.recordIDs == [try NativeCloudMediaRecords.id(request)])
            #expect(operation.recordIDs?.first?.zoneID.ownerName == CKCurrentUserDefaultName)
            #expect(operation.recordIDs?.first?.zoneID.zoneName == "meguri-media")
        }
    }

    @Test func metadataRoundUsesOneBoundedRecordIDOperation() async throws {
        let queue = NativeOwnCapturingQueue()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root, available: { UInt64.max }))
        let names = [name, "bbbbbbbbbbbbbbbbbbbbbbbbbbb"]
        let prepared = try transport.prepare(.reads(names: names))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
        #expect(operation.desiredKeys == ["e"] && operation.recordIDs?.count == 2)
        for id in try #require(operation.recordIDs) { operation.perRecordResultBlock?(id, .failure(CKError(.unknownItem))) }
        operation.fetchRecordsResultBlock?(.failure(CKError(.partialFailure)))
        guard case .blobReads(let results) = try await prepared.reply().result else { Issue.record("round lost"); return }
        #expect(results.count == 2)
        #expect(throws: NativeCloudKitFailure.self) { try transport.prepare(.reads(names: Array(repeating: name, count: 201))) }
    }

    @Test func callbackStagesBeforeOriginalAssetDisappears() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let source = root.appendingPathComponent("temporary")
        let bytes = Data([7, 1, 5, 3, 9])
        try bytes.write(to: source)
        let queue = NativeOwnCapturingQueue()
        let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root.appendingPathComponent("runs"), available: { UInt64.max }))
        let request = CloudMediaRequest.download(name: name)
        let prepared = try transport.prepare(request)
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
        let record = CKRecord(recordType: "MGBlob", recordID: try NativeCloudMediaRecords.id(request))
        record.encryptedValues["e"] = Data([1]) as CKRecordValue
        record["a"] = CKAsset(fileURL: source)
        operation.perRecordResultBlock?(record.recordID, .success(record))
        try FileManager.default.removeItem(at: source)
        operation.fetchRecordsResultBlock?(.success(()))
        guard case .record(_, _, _, let asset) = try await prepared.reply().result else { Issue.record("missing actual record"); return }
        #expect(try Data(contentsOf: #require(asset?.url)) == bytes)
    }

    @Test func metadataValidationRejectsForeignOwnerWrongZoneNameTypeAndAssets() throws {
        let request = CloudMediaRequest.read(name: name)
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let staging = try CloudMediaStaging(directory: root, available: { UInt64.max })
        for id in [
            CKRecord.ID(recordName: name, zoneID: .init(zoneName: "meguri-media", ownerName: "foreign")),
            CKRecord.ID(recordName: name, zoneID: .init(zoneName: "other", ownerName: CKCurrentUserDefaultName)),
            CKRecord.ID(recordName: "other", zoneID: NativeCloudOwnRecords.zone("meguri-media"))
        ] {
            let record = CKRecord(recordType: "MGBlob", recordID: id)
            record.encryptedValues["e"] = Data([1]) as CKRecordValue
            #expect(throws: NativeCloudKitFailure.self) { try NativeCloudMediaRecords.read(record, request: request, staging: staging, cancelled: { false }) }
        }
        let record = CKRecord(recordType: "MGEntity", recordID: try NativeCloudMediaRecords.id(request))
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudMediaRecords.read(record, request: request, staging: staging, cancelled: { false }) }
    }

    @Test func readErrorsKeepCodeAndMaximumRetryDuration() throws {
        let error = CKError(.partialFailure, userInfo: [CKPartialErrorsByItemIDKey: [try NativeCloudMediaRecords.id(.read(name: name)): CKError(.serviceUnavailable, userInfo: [CKErrorRetryAfterKey: 120.0])]])
        let reply = NativeCloudMediaRecords.failure(error)
        #expect(reply.retryAfterMilliseconds == 120_000)
        guard case .failed(let code) = reply.result else { Issue.record("partial failure became absence"); return }
        #expect(code == Int64(CKError.Code.partialFailure.rawValue))
        guard case .unknownItem = NativeCloudMediaRecords.failure(CKError(.unknownItem)).result else { Issue.record("unknown item lost"); return }
        guard case .zoneMissing = NativeCloudMediaRecords.failure(CKError(.zoneNotFound)).result else { Issue.record("zone missing lost"); return }
    }

    @Test func partialMetadataRetainsRecordsAndActualFailureWithLongestDelay() async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = NativeMediaTerminalCapture()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root, available: { UInt64.max }),
            terminalHandlerInstalled: { terminal.installed($0) })
        let names = [name, "bbbbbbbbbbbbbbbbbbbbbbbbbbb"]
        let prepared = try transport.prepare(.reads(names: names))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
        let ids = try #require(operation.recordIDs)
        let record = CKRecord(recordType: "MGBlob", recordID: ids[0])
        record.encryptedValues["e"] = Data([1, 2]) as CKRecordValue
        operation.perRecordResultBlock?(ids[0], .success(record))
        let itemError = CKError(.serviceUnavailable, userInfo: [CKErrorRetryAfterKey: 30.0])
        operation.perRecordResultBlock?(ids[1], .failure(itemError))
        try terminal.complete(.failure(CKError(.partialFailure, userInfo: [
            CKErrorRetryAfterKey: 120.0, CKPartialErrorsByItemIDKey: [ids[1]: itemError]
        ])))
        let reply = try await prepared.reply()
        guard case .blobReads(let results) = reply.result, results.count == 2,
              case .present(let savedName, let envelope) = results[0],
              case .failed(let failedName, let code) = results[1] else {
            Issue.record("metadata results were lost"); return
        }
        #expect(savedName == names[0] && envelope == Data([1, 2]))
        #expect(failedName == names[1] && code == Int64(CKError.Code.serviceUnavailable.rawValue))
        let failure = try #require(reply.failure())
        #expect(failure.code == code && failure.retryAfterMilliseconds == 120_000)
    }

    @Test func downloadFailureRetainsActualCodeAndDelay() async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = NativeMediaTerminalCapture()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root, available: { UInt64.max }),
            terminalHandlerInstalled: { terminal.installed($0) })
        let prepared = try transport.prepare(.download(name: name))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
        let id = try #require(operation.recordIDs?.first)
        operation.perRecordResultBlock?(id, .failure(CKError(.zoneBusy, userInfo: [CKErrorRetryAfterKey: 45.0])))
        try terminal.complete(.failure(CKError(.partialFailure, userInfo: [CKErrorRetryAfterKey: 90.0])))
        let reply = try await prepared.reply()
        let failure = try #require(reply.failure())
        #expect(failure.code == Int64(CKError.Code.zoneBusy.rawValue))
        #expect(failure.retryAfterMilliseconds == 90_000)
    }

    @Test func terminalPartialErrorRetainsMissingItemsActualCode() async throws {
        let queue = NativeOwnCapturingQueue()
        let terminal = NativeMediaTerminalCapture()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root, available: { UInt64.max }),
            terminalHandlerInstalled: { terminal.installed($0) })
        let prepared = try transport.prepare(.reads(names: [name]))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
        let id = try #require(operation.recordIDs?.first)
        try terminal.complete(.failure(CKError(.partialFailure, userInfo: [
            CKPartialErrorsByItemIDKey: [id: CKError(.requestRateLimited, userInfo: [CKErrorRetryAfterKey: 60.0])]
        ])))
        let reply = try await prepared.reply()
        #expect(reply.failure()?.code == Int64(CKError.Code.requestRateLimited.rawValue))
        #expect(reply.failure()?.retryAfterMilliseconds == 60_000)
    }

    @Test func terminalFailureDoesNotEraseSuccessfulMetadata() async throws {
        let queue = NativeOwnCapturingQueue()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root, available: { UInt64.max }))
        let prepared = try transport.prepare(.read(name: name))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
        let id = try #require(operation.recordIDs?.first)
        let record = CKRecord(recordType: "MGBlob", recordID: id)
        record.encryptedValues["e"] = Data([3]) as CKRecordValue
        operation.perRecordResultBlock?(id, .success(record))
        operation.fetchRecordsResultBlock?(.failure(CKError(.networkFailure)))
        let reply = try await prepared.reply()
        guard case .record(let actual, _, let envelope, nil) = reply.result else {
            Issue.record("successful item was erased"); return
        }
        #expect(actual == name && envelope == Data([3]))
        #expect(reply.failure()?.code == Int64(CKError.Code.networkFailure.rawValue))
    }

    @Test func cancellationWaitsForOriginalTerminalCallback() async throws {
        let queue = NativeOwnCapturingQueue()
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: root) }
        let transport = NativeCloudMediaTransport(queue: queue, staging: try .init(directory: root, available: { UInt64.max }))
        let prepared = try transport.prepare(.read(name: name))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first } as? CKFetchRecordsOperation)
        let joined = Task { try await prepared.reply() }
        joined.cancel(); prepared.cancel()
        // The actual callback, including its original error, must win over
        // cancellation of the task that waits for it.
        operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled)))
        guard case .failed(let code) = try await joined.value.result else { Issue.record("cancel became success"); return }
        #expect(code == Int64(CKError.Code.operationCancelled.rawValue))
    }

    @Test func realMediaBatchEncodesAssetAndSavedRoutesToOriginalBatch() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("original-rust-stage")
        try Data([7, 5, 3, 1]).write(to: file)
        let record = CloudRecord(name: name, envelope: Data([1]), systemFields: nil, recordType: "MGBlob", zone: "meguri-media", assetURL: file)
        let batch = CloudEngineBatch(id: 71, saves: [record], deletes: [])
        try NativeCloudMediaBatch.validate(batch, zone: "meguri-media")
        let encoded = try NativeCloudRecordConversion.encode(record, zone: "meguri-media")
        #expect((encoded["a"] as? CKAsset)?.fileURL == file)
        #expect(encoded.recordType == "MGBlob")
        let saved = try NativeCloudRecordConversion.decode(encoded)
        #expect(saved.envelope == record.envelope && saved.zone == "meguri-media" && saved.assetURL == nil)
        let collection = NativeCloudOwnRecords.zone("c-synthetic")
        #expect(try NativeCloudKitRecordRouting.sentEngineBatchID(saved: [encoded.recordID], failed: [], deleted: [], collectionZoneID: collection, batches: [name: 71], pendingBatch: 99) == 71)
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudKitRecordRouting.sentEngineBatchID(saved: [encoded.recordID], failed: [], deleted: [], collectionZoneID: collection, batches: [:], pendingBatch: 71) }
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudMediaBatch.validate(batch, zone: "c-synthetic") }
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.encode(record, zone: "c-synthetic") }
        let collectionRecord = CloudRecord(name: "entity", envelope: Data([1]), systemFields: nil)
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.encode(collectionRecord, zone: "meguri-media") }
        let control = CKRecord(recordType: "MGActive", recordID: NativeCloudOwnRecords.id(.init(zone: "meguri-root", name: "active", recordType: "MGActive")))
        control["a"] = CKAsset(fileURL: file)
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.decode(control) }
    }

    @Test func ordinaryEntityUploadUsesPaAndKeepsItsOriginalFileThroughSavedConversion() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("rust-owned-pa")
        let bytes = Data(repeating: 0x70, count: 800_000)
        try bytes.write(to: file)
        let record = CloudRecord(name: "ordinary-note", envelope: Data([1]), systemFields: nil,
            recordType: "MGEntity", zone: "c-synthetic", assetURL: file)
        // This is the same conversion the native collection batch calls.
        let encoded = try NativeCloudRecordConversion.encode(record, zone: "c-synthetic")
        #expect(encoded.recordType == "MGEntity")
        #expect((encoded["pa"] as? CKAsset)?.fileURL == file)
        #expect(encoded["a"] == nil && encoded["ma"] == nil)
        #expect(encoded.encryptedValues["e"] as? Data == record.envelope)
        try NativeCloudMediaBatch.validate(.init(id: 72, saves: [record], deletes: []), zone: "c-synthetic")
        let saved = try NativeCloudRecordConversion.decode(encoded)
        #expect(saved.name == record.name && saved.envelope == record.envelope)
        #expect(saved.systemFields?.isEmpty == false)
        // Saved conversion cannot consume or remove Rust's original upload.
        #expect(try Data(contentsOf: file) == bytes)
        let foreign = CKRecord(recordType: "MGEntity", recordID: .init(recordName: record.name,
            zoneID: .init(zoneName: "c-synthetic", ownerName: "foreign-owner")))
        foreign.encryptedValues["e"] = record.envelope as CKRecordValue
        foreign["pa"] = CKAsset(fileURL: file)
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudRecordConversion.decode(foreign) }
    }
}
#endif

#if canImport(CloudKit)
import Synchronization

private final class HeldNativeMediaQueue: NativeCloudDatabaseQueue, Sendable {
    let issued: AsyncStream<CKFetchRecordsOperation>
    private let continuation: AsyncStream<CKFetchRecordsOperation>.Continuation
    init() { (issued, continuation) = AsyncStream.makeStream() }
    func finish() { continuation.finish() }
    func add(_ operation: CKDatabaseOperation) -> Bool {
        guard let read = operation as? CKFetchRecordsOperation else { return false }
        continuation.yield(read)
        return true
    }
}
private struct ObservedMediaTransport: CloudMediaTransport {
    let native: NativeCloudMediaTransport
    let cancelled: AsyncStream<Void>.Continuation
    var failure: (any Error)? = nil
    func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead {
        ObservedMediaRead(native: try native.prepare(request), cancelled: cancelled, failure: failure)
    }
}
private struct ObservedMediaRead: CloudPreparedMediaRead {
    let native: any CloudPreparedMediaRead
    let cancelled: AsyncStream<Void>.Continuation
    let failure: (any Error)?
    func enqueue() -> Bool { native.enqueue() }
    func reply() async throws -> CloudMediaTransportReply {
        let reply = try await native.reply()
        if let failure { throw failure }
        return reply
    }
    func cancel() { native.cancel(); cancelled.yield(()) }
}
private final class NativeMediaReadFailure: Error {}

#if DEBUG
private final class NativeMediaPendingUpdateFailure: Error {}

private struct NativeMediaPreparationProbe: CloudMediaTransport {
    let native: NativeCloudMediaTransport
    let observe: @Sendable () -> Void
    func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead {
        observe()
        return try native.prepare(request)
    }
}

// Retain only work and bytes issued by the real Core. An original collection
// record in the durable inbox requests the now-present blob for download.
private func prepareDebugMediaDownload(_ fixture: CloudKitBridgeFixture,
                                      policy: CoreCloudPolicyService,
                                      media: CoreCloudMediaService, sdk: String) async throws {
    guard case .commands(let commands) = try await policy.input(.settings(.init(
        enabled: true, paused: false, automatic: true, media: true))) else {
        throw CloudMediaFailure.missingReply
    }
    let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
    let permit = try #require(try await policy.prepareExecution(resume))
    try #require(try await permit.beginMedia())
    let root = try #require(try await media.nextWork(sdk: sdk))
    try #require(root.request == .root)
    try await media.reply(root.handle, result: .record(name: "root", recordType: "MGMediaRoot",
        envelope: Data([0x08, 0x01, 0x12, 0x20]) + Data(repeating: 47, count: 32), asset: nil))

    let before = Set(try await fixture.core.trackingSendables().keys)
    _ = try await CoreMediaService().addMedia(name: "pending.asset", bytes: [7, 1, 5, 3, 9])
    let read = try #require(try await media.nextWork(sdk: sdk))
    switch read.request {
    case .read:
        try await media.reply(read.handle, result: .unknownItem)
    case .reads(let names):
        try #require(names.count == 1)
        try await media.reply(read.handle, result: .blobReads(names.map { .unknownItem(name: $0) }))
    default: throw CloudMediaFailure.missingReply
    }
    let upload = try #require(try await media.nextWork(sdk: sdk))
    guard case .upload(let blob, _, _) = upload.request else { throw CloudMediaFailure.missingReply }
    try await media.queueUpload(upload.handle)
    for update in try await fixture.core.takePendingUpdates(sdk: sdk) {
        try await fixture.core.updateApplied(sdk: sdk, id: update.id, added: Array(update.adds.keys))
    }
    let batch = try #require(try await fixture.core.buildBatch(sdk: sdk, zone: "meguri-media", pending: [blob: .save]))
    try #require(batch.saves.count == 1 && batch.saves.first?.name == blob)
    _ = try await fixture.core.sent(sdk: sdk, batchID: batch.id, saved: batch.saves,
        failed: [], deleted: [], failedDeletes: [], batchCount: 1)
    try await fixture.core.handlerReturned(sdk: sdk)

    let pending = try await fixture.core.trackingSendables().filter { !before.contains($0.key) && $0.key != blob }
    try #require(pending.count == 1)
    let collection = try #require(try await fixture.core.buildBatch(sdk: sdk, zone: fixture.zone, pending: pending))
    try #require(collection.saves.count == 1)
    try await fixture.core.fetched(sdk: sdk, zone: fixture.zone, records: collection.saves, deleted: [])
}

private struct NativeEventWaitDelegate: CloudKitEngineDelegate {
    var returnedGate: CloudCallbackGate?
    init(returnedGate: CloudCallbackGate? = nil) { self.returnedGate = returnedGate }
    func eventWillDeliver(engine: any CloudKitEngine) async {}
    func handleEvent(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {}
    func eventHandlerReturned(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {
        if let returnedGate { await returnedGate.enter() }
    }
    func nextRecordZoneChangeBatch(pending: [String: CloudPendingChange], zone: String,
                                   engine: any CloudKitEngine) async -> CloudEngineBatch? { nil }
    func nextFetchChangesOptions(_ options: CloudFetchOptions, engine: any CloudKitEngine) async -> CloudFetchOptions { options }
}

// Keep the fake's concrete nonthrowing API used by other witnesses. Only this
// wrapper's protocol application can fail; every SDK operation still delegates.
private actor NativeMediaPendingUpdateEngine: CloudKitEngine {
    nonisolated let instanceID: String
    private let sdk: CloudKitEngineFake
    private var nextFailure: (any Error)?
    private(set) var injectedFailures = 0
    init(_ sdk: CloudKitEngineFake) { self.sdk = sdk; instanceID = sdk.instanceID }
    func failNextPendingUpdate(_ error: any Error) { nextFailure = error }
    func pendingRecordZoneChanges() async -> [String: CloudPendingChange] { await sdk.pendingRecordZoneChanges() }
    func pendingDatabaseChanges() async -> Set<String> { await sdk.pendingDatabaseChanges() }
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws {
        if let error = nextFailure {
            nextFailure = nil
            injectedFailures += 1
            throw error
        }
        await sdk.applyPendingUpdate(update)
    }
    func removePendingDatabaseChanges() async { await sdk.removePendingDatabaseChanges() }
    func waitForEventBookkeeping() async throws { try await sdk.waitForEventBookkeeping() }
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue { try await sdk.prepareSend(zones: zones) }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue { try await sdk.prepareFetch(options: options) }
    func sendChanges(zones: [String]) async throws { try await sdk.sendChanges(zones: zones) }
    func fetchChanges(options: CloudFetchOptions) async throws { try await sdk.fetchChanges(options: options) }
    func cancelOperations() async { await sdk.cancelOperations() }
    func release() async { await sdk.release() }
}
#endif

extension EventTests {
    #if DEBUG
    @Test(arguments: [false, true], [false, true])
    func nativeRetainedOperationEventWaitCancellationAndRelease(stop: Bool, fetch: Bool) async throws {
        let zone = "event-wait-zone"
        let engine = CloudKitEngineFake(configuration: .init(instanceID: "event-wait", automaticallySync: false,
            serialization: nil, collectionZone: zone), delegate: NativeEventWaitDelegate(), a23: true, keepAlive: false)
        let prepared: CloudKitNetworkEnqueue
        if fetch {
            prepared = try await engine.prepareFetch(options: .init(zones: [zone], priorityZones: [], groupID: "event-wait"))
        } else { prepared = try await engine.prepareSend(zones: [zone]) }
        let held = CloudCallbackGate()
        await engine.pauseNextCallback(held)
        let (waits, observedWait) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        await engine.observeNextNetworkEventWait { _ = observedWait.yield(()) }
        let callback = Task { await engine.emitRetained(.willSend(zone: zone), label: "held") }
        let deadline = CloudPolicyReadinessDecision()
        let watchdog = Task {
            do { try await Task.sleep(for: .seconds(10)) } catch { return }
            if deadline.expire() {
                observedWait.finish()
                await held.abort(CancellationError())
                await engine.cancelOperations()
            }
        }
        let validations = Mutex(0)
        var operation: CloudKitNetworkOperation?
        var joined: Task<(any Error)?, Never>?
        var failure: (any Error)?
        do {
            try await held.waitForArrival()
            let retained = try prepared.enqueue(validate: { validations.withLock { $0 += 1 } })
            operation = retained
            let running = Task<(any Error)?, Never> {
                do { try await retained.drain(); return nil } catch { return error }
            }
            joined = running
            var entries = waits.makeAsyncIterator()
            guard await entries.next() != nil else { throw CancellationError() }
            try #require(await engine.networkEventWaiterCount() == 1)
            try #require(engine.acceptedNetworkCalls.value == 0 && validations.withLock { $0 } == 0)
            if stop { await engine.release() } else { running.cancel() }
            let result = await running.value
            try #require(result is CancellationError)
            try #require(await engine.networkEventWaiterCount() == 0)
            let trace = await engine.recordedTrace()
            try #require(trace.contains(.eventEntered("held")) && !trace.contains(.eventReturned("held")))
            try #require(trace.contains(.cancelReturned))
        } catch { failure = error }
        let inTime = deadline.observeReady()
        await held.open()
        await callback.value
        joined?.cancel()
        if let operation { await operation.cancelAndDrain() }
        _ = await joined?.value
        await engine.observeNextNetworkEventWait(nil)
        observedWait.finish()
        await engine.release()
        watchdog.cancel()
        await watchdog.value
        #expect(inTime, "Retained operation event-wait cancellation/drain exceeded ten seconds")
        #expect(await held.pendingWaiterCount() == 0)
        #expect(await engine.networkEventWaiterCount() == 0)
        #expect(engine.acceptedNetworkCalls.value == 0 && validations.withLock { $0 } == 0)
        if let failure { throw failure }
    }

    @Test(arguments: [false, true], [false, true])
    func nativePreparationRejoinsEventDeliveredDuringAwait(fetch: Bool, returnedBookkeeping: Bool) async throws {
        let zone = "prepare-rejoin-zone"
        let preparation = CloudCallbackGate(), event = CloudCallbackGate()
        let returnedWork = CloudCallbackGate()
        let engine = CloudKitEngineFake(configuration: .init(instanceID: "prepare-rejoin", automaticallySync: false,
            serialization: nil, collectionZone: zone),
            delegate: NativeEventWaitDelegate(returnedGate: returnedBookkeeping ? event : nil), a23: true, keepAlive: false)
        let prepared: CloudKitNetworkEnqueue
        if fetch { prepared = try await engine.prepareFetch(options: .init(zones: [zone], priorityZones: [], groupID: "prepare-rejoin")) }
        else { prepared = try await engine.prepareSend(zones: [zone]) }
        let (waits, rejoined) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let validations = CloudAcceptedNetworkCalls()
        let operation = try prepared.enqueue(prepare: { await preparation.enter() }, validate: { validations.record() })
        let deadline = CloudPolicyReadinessDecision()
        let watchdog = Task {
            do { try await Task.sleep(for: .seconds(10)) } catch { return }
            if deadline.expire() {
                await preparation.abort(CancellationError()); await event.abort(CancellationError())
                await returnedWork.abort(CancellationError())
                rejoined.finish(); operation.cancel()
            }
        }
        var callback: Task<Void, Never>?
        var failure: (any Error)?
        do {
            try await preparation.waitForArrival()
            if returnedBookkeeping { await engine.pauseNativeReturnedWork(returnedWork) }
            if !returnedBookkeeping { await engine.pauseNextCallback(event) }
            await engine.observeNextNetworkEventWait { rejoined.yield(()) }
            callback = Task { await engine.emitRetained(.willSend(zone: zone), label: "duringPrepare") }
            try await event.waitForArrival()
            await preparation.open()
            var iterator = waits.makeAsyncIterator()
            try #require(await iterator.next() != nil)
            #expect(engine.acceptedNetworkCalls.value == 0 && validations.value == 0)
            #expect(await engine.networkEventWaiterCount() == 1)
            let trace = await engine.recordedTrace()
            #expect(trace.contains(.eventReturned("duringPrepare")) == returnedBookkeeping)
            await event.open()
            await callback?.value
            if returnedBookkeeping {
                // Delivery has finished and the event is idle; preparation must
                // still join returned work before validation or SDK entry.
                try await returnedWork.waitForArrival()
                #expect(await engine.networkEventWaiterCount() == 0)
                #expect(engine.acceptedNetworkCalls.value == 0 && validations.value == 0)
                await returnedWork.open()
            }
            try await operation.drain()
        } catch { failure = error }
        await preparation.open(); await event.open(); rejoined.finish()
        await returnedWork.open()
        if failure != nil { await operation.cancelAndDrain() }
        await callback?.value
        let calls = engine.acceptedNetworkCalls.value
        let inTime = deadline.observeReady()
        watchdog.cancel(); await watchdog.value
        await engine.observeNextNetworkEventWait(nil)
        await engine.release()
        #expect(inTime && calls == 1 && validations.value == 1)
        #expect(await preparation.pendingWaiterCount() == 0)
        #expect(await event.pendingWaiterCount() == 0)
        #expect(await returnedWork.pendingWaiterCount() == 0)
        #expect(await engine.networkEventWaiterCount() == 0)
        if let failure { throw failure }
    }

    @Test func nativeMediaUploadsResumeAfterUnrelatedPersistedSentHandlerReturns() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            let held = CloudCallbackGate(), afterSent = CloudCallbackGate()
            let mediaTask = CloudCallbackGate(), didSend = CloudCallbackGate()
            await fixture.probe.pauseSent(held)
            await engine.pauseAfterSentHandler(afterSent)
            let (admissions, observedAdmission) = AsyncStream<Bool>.makeStream()
            let (finalEntries, observedFinalEntry) = AsyncStream<CloudKitNetworkLifetime.FinalEntryObservation>
                .makeStream(bufferingPolicy: .bufferingNewest(1))
            let admissionEvidence = Mutex<(handlerReleased: Bool,
                entries: [(admitted: Bool, beforeHandlerRelease: Bool)])>((false, []))
            let callEvidence = Mutex<(admitted: [UInt64], finalized: [UInt64],
                admissionOwnersMatch: Bool, finalizationOwnersMatch: Bool, capped: Bool)>(([], [], true, true, false))
            let queue = HeldNativeMediaQueue()
            let transport = NativeCloudMediaTransport(queue: queue,
                staging: try .init(directory: fixture.directory.appendingPathComponent("busy-send-media"), available: { UInt64.max }))
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: transport)
            let responses = Task {
                for await operation in queue.issued {
                    for id in operation.recordIDs ?? [] {
                        if id.recordName == "root" {
                            let root = CKRecord(recordType: "MGMediaRoot", recordID: id)
                            // Canonical V1 synthetic media root: format 1,
                            // 32-byte shared media key, creation time zero.
                            root.encryptedValues["e"] = (Data([0x08, 0x01, 0x12, 0x20])
                                + Data(repeating: 47, count: 32)) as CKRecordValue
                            operation.perRecordResultBlock?(id, .success(root))
                        } else {
                            // Only original metadata callbacks report absence;
                            // the SDK's own send later supplies actual Saved.
                            operation.perRecordResultBlock?(id, .failure(CKError(.unknownItem)))
                        }
                    }
                    operation.fetchRecordsResultBlock?(.success(()))
                }
            }
            let sdkCallsBeforeOriginal = engine.acceptedNetworkCalls.value
            let expectedOwner = engine.instanceID
            await fixture.adapter.setSendCallObserver { origin, call, finalized in
                callEvidence.withLock { evidence in
                    if finalized {
                        evidence.finalizationOwnersMatch = evidence.finalizationOwnersMatch && origin == expectedOwner
                        if evidence.finalized.count < 8 { evidence.finalized.append(call) }
                        else { evidence.capped = true }
                    } else {
                        evidence.admissionOwnersMatch = evidence.admissionOwnersMatch && origin == expectedOwner
                        if evidence.admitted.count < 8 { evidence.admitted.append(call) }
                        else { evidence.capped = true }
                    }
                }
            }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    admissionEvidence.withLock { $0.handlerReleased = true }
                    await held.abort(CancellationError())
                    await afterSent.abort(CancellationError())
                    await mediaTask.abort(CancellationError())
                    await didSend.abort(CancellationError())
                    observedAdmission.finish()
                    observedFinalEntry.finish()
                    try? await fixture.adapter.stop()
                }
            }
            // This call owns its sent event and cannot return before that
            // original event does. It is unrelated to the later media round.
            let original = Task { try await fixture.adapter.requestSend(zones: [fixture.zone]) }
            var media: Task<CloudPolicyNativeResult, Never>?
            var names: [String] = []
            var finalEntry: CloudKitNetworkLifetime.FinalEntryObservation?
            var failure: (any Error)?
            do {
                try await held.waitForArrival()
                let trace = await engine.recordedTrace()
                let handed = try #require(trace.compactMap { entry -> UInt64? in
                    if case .handed(let id, let names) = entry, names.contains(fixture.names[0]) { id } else { nil }
                }.last)
                let committed = try #require(try await fixture.core.entityFacts(name: fixture.names[0]))
                try #require(committed.ackedRev == committed.localRev && committed.ackedRev > 0 && committed.mark == nil)
                try #require(!(try await fixture.core.observations()).contains(.handlerReturned(requestID: handed)))
                try #require(!trace.contains(.eventReturned("sent")))
                let calls = engine.acceptedNetworkCalls.value
                let bytes: [[UInt8]] = [[7, 1, 5, 3, 9], [2, 4, 6, 8, 10, 12]]
                let files = CoreMediaService()
                for (index, content) in bytes.enumerated() {
                    names.append(try await files.addMedia(name: "busy-send-\(index).asset", bytes: content))
                }
                try #require(Set(names).count == 2)
                let tracked = try await files.trackedNames()
                for (name, content) in zip(names, bytes) {
                    try #require(tracked.names.contains { $0.name == name })
                    try #require(try Data(contentsOf: tracked.root.appendingPathComponent(name)) == Data(content))
                }
                guard case .commands(let commands) = try await policy.input(.settings(.init(
                    enabled: true, paused: false, automatic: true, media: true))) else {
                    throw CloudMediaFailure.missingReply
                }
                let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
                await fixture.adapter.setSendAdmissionObserver { admitted in
                    admissionEvidence.withLock {
                        if $0.entries.count < 8 {
                            $0.entries.append((admitted, !$0.handlerReleased))
                        }
                    }
                    _ = observedAdmission.yield(admitted)
                }
                let running = Task { await executor.execute(resume) }
                media = running
                var entries = admissions.makeAsyncIterator()
                // A valid implementation may wait after its first rejection.
                // Observe that admission without requiring the round to finish.
                guard let admitted = await entries.next() else { throw CancellationError() }
                try #require(!admitted)
                try #require(engine.acceptedNetworkCalls.value == calls)
                try #require(try await fixture.core.currentGuard() == guardBefore)
                try #require(await fixture.adapter.retainedInstanceIDs() == [engine.instanceID])
                try #require(await fixture.factory.engines().count == 1)
                let status = try await policy.status()
                try #require(status.accountStatus == .available && status.accountMatchesBinding && !status.mediaPaused)
                for name in names {
                    let pending = try #require(try await CoreSmokeInspection().cloud(mediaName: name).entities.first)
                    let size = try #require(pending.mediaBytes)
                    try #require(size > 0 && !pending.blobPresent && !pending.acknowledged)
                }
                // Arm only after this round's first actual nil admission;
                // the unrelated original SDK task must never take this gate.
                await engine.pauseNextOperationTask(mediaTask)
                await engine.observeNextSendFinalEntry { entry in
                    _ = observedFinalEntry.yield(entry)
                }
                admissionEvidence.withLock { $0.handlerReleased = true }
                await held.open()
                try await afterSent.waitForArrival()
                try await mediaTask.waitForArrival()
                // Preparation saw the completed sent slot. Independently start
                // the original didSend before the detached task's final entry.
                await engine.pauseNextCallback(didSend)
                await afterSent.open()
                try await didSend.waitForArrival()
                let activeTrace = await engine.recordedTrace()
                try #require(activeTrace.contains(.eventEntered("didSend")))
                try #require(!activeTrace.contains(.eventReturned("didSend")))
                await mediaTask.open()
                var finals = finalEntries.makeAsyncIterator()
                guard let entry = await finals.next() else { throw CancellationError() }
                finalEntry = entry
                // Observe entry, never completion or successful admission under
                // the held callback. A future final-entry event wait can finish
                // as soon as this original didSend is released.
                await didSend.open()
                #expect(!entry.taskCancelled && entry.eventActive && entry.generationMatches && entry.sdkLive,
                    "Final entry must isolate active-event rejection from task cancellation or revoked SDK authority")
                try await original.value
                _ = await running.value
            } catch { failure = error }

            // All paths release callbacks before joining every owned task.
            // Deadline assertions come only after these joins, including the
            // watchdog's failure-only stop/drain and the native responder.
            admissionEvidence.withLock { $0.handlerReleased = true }
            await held.open()
            await afterSent.open()
            await mediaTask.open()
            await didSend.open()
            media?.cancel()
            let originalResult = await original.result
            let mediaResult = await media?.value
            await fixture.adapter.setSendAdmissionObserver(nil)
            await fixture.adapter.setSendCallObserver(nil)
            await engine.observeNextSendFinalEntry(nil)
            observedAdmission.finish()
            observedFinalEntry.finish()
            queue.finish()
            await responses.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            for gate in [held, afterSent, mediaTask, didSend] {
                #expect(await gate.pendingWaiterCount() == 0)
            }
            #expect(await engine.networkEventWaiterCount() == 0)
            let evidence = admissionEvidence.withLock { $0.entries }
            let calls = callEvidence.withLock { $0 }
            let exactCallsMatch = calls.admitted.count == calls.finalized.count
                && Set(calls.admitted).count == calls.admitted.count
                && Set(calls.admitted) == Set(calls.finalized)
            let mediaFailure = (mediaResult?.error).map {
                DebugCloudDiagnostics.Value(.policyExecute, error: $0).failure
            } ?? "none"
            let adapterError = await fixture.adapter.lastFailure
            let adapterFailure = adapterError.map {
                DebugCloudDiagnostics.Value(.policyExecute, error: $0).failure
            } ?? "none"
            let finalEvidence = finalEntry.map {
                "taskCancelled=\($0.taskCancelled) eventActive=\($0.eventActive)"
                    + " generationMatches=\($0.generationMatches) sdkLive=\($0.sdkLive)"
            } ?? "finalEntry=unobserved"
            // IDs remain internal to the bounded exact-call comparison.
            let diagnostic = "mediaFailure=\(mediaFailure) adapterFailure=\(adapterFailure)"
                + " sdkCallsSinceOriginal=\(engine.acceptedNetworkCalls.value - sdkCallsBeforeOriginal)"
                + " admittedCalls=\(calls.admitted.count) finalizedCalls=\(calls.finalized.count)"
                + " exactCallsMatch=\(exactCallsMatch) admissionOwnersMatch=\(calls.admissionOwnersMatch)"
                + " finalizationOwnersMatch=\(calls.finalizationOwnersMatch) callEvidenceCapped=\(calls.capped)"
                + " \(finalEvidence)"
            #expect(inTime, "Media admission or original callback drain exceeded the existing ten-second bound; first eight admissions (admitted, before-handler-release): \(evidence); \(diagnostic)")
            if let failure { throw failure }
            try originalResult.get()
            #expect(mediaResult?.error == nil && mediaResult?.deferred == false,
                "Media round failed; first eight admissions (admitted, before-handler-release): \(evidence); \(diagnostic)")
            for name in names {
                let saved = try #require(try await CoreSmokeInspection().cloud(mediaName: name).entities.first)
                #expect(saved.acknowledged && saved.ackedRevision == saved.revision && saved.blobPresent,
                    "Both generated files must reach real Saved commits without another policy wake; first eight admissions (admitted, before-handler-release): \(evidence); \(diagnostic)")
            }
            #expect(try await fixture.core.currentGuard() == guardBefore)
            #expect(await fixture.factory.engines().count == 1)
            #expect(!(await engine.recordedTrace()).contains(.reentrantNetworkCall))
        }
    }

    @Test(arguments: [false, true]) func nativeMediaBusyAdmissionCancellationAndLostWake(cancel: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            let held = CloudCallbackGate(), declined = CloudCallbackGate()
            await fixture.probe.pauseSent(held)
            let (admissions, observedAdmission) = AsyncStream<Bool>.makeStream()
            let queue = HeldNativeMediaQueue()
            let transport = NativeCloudMediaTransport(queue: queue,
                staging: try .init(directory: fixture.directory.appendingPathComponent("busy-admission-races"), available: { UInt64.max }))
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: transport)
            let responses = Task {
                for await operation in queue.issued {
                    for id in operation.recordIDs ?? [] {
                        if id.recordName == "root" {
                            let root = CKRecord(recordType: "MGMediaRoot", recordID: id)
                            root.encryptedValues["e"] = (Data([0x08, 0x01, 0x12, 0x20])
                                + Data(repeating: 47, count: 32)) as CKRecordValue
                            operation.perRecordResultBlock?(id, .success(root))
                        } else {
                            operation.perRecordResultBlock?(id, .failure(CKError(.unknownItem)))
                        }
                    }
                    operation.fetchRecordsResultBlock?(.success(()))
                }
            }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    await held.abort(CancellationError())
                    await declined.abort(CancellationError())
                    observedAdmission.finish()
                    try? await fixture.adapter.stop()
                }
            }
            let original = Task { try await fixture.adapter.requestSend(zones: [fixture.zone]) }
            var media: Task<CloudPolicyNativeResult, Never>?
            var failure: (any Error)?
            do {
                try await held.waitForArrival()
                let trace = await engine.recordedTrace()
                let handed = try #require(trace.compactMap { entry -> UInt64? in
                    if case .handed(let id, let names) = entry, names.contains(fixture.names[0]) { id } else { nil }
                }.last)
                try #require(!(try await fixture.core.observations()).contains(.handlerReturned(requestID: handed)))
                let calls = engine.acceptedNetworkCalls.value
                let name = try await CoreMediaService().addMedia(name: "busy-admission.asset", bytes: [3, 1, 4, 1, 5, 9])
                guard case .commands(let commands) = try await policy.input(.settings(.init(
                    enabled: true, paused: false, automatic: true, media: true))) else {
                    throw CloudMediaFailure.missingReply
                }
                let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
                await fixture.adapter.setSendAdmissionObserver { admitted in
                    _ = observedAdmission.yield(admitted)
                    // Force handler return BEFORE wait registration in this
                    // variant, using the existing post-Core scheduling seam.
                    if !admitted && !cancel { await declined.enter() }
                }
                let running = Task { await executor.execute(resume) }
                media = running
                var entries = admissions.makeAsyncIterator()
                guard let admitted = await entries.next() else {
                    throw CloudMediaFailure.missingReply
                }
                try #require(!admitted)
                try #require(engine.acceptedNetworkCalls.value == calls)
                if cancel {
                    // This rendezvous observes an installed versioned waiter,
                    // not merely the earlier nil-admission callback.
                    try await fixture.adapter.waitForMediaSendAdmissionWaiter()
                    running.cancel()
                    let result = await running.value
                    try #require(result.error is CancellationError && !result.deferred)
                    try #require(!result.mediaCancellation)
                    try #require(engine.acceptedNetworkCalls.value == calls)
                    try #require(!(try await fixture.core.observations()).contains(.handlerReturned(requestID: handed)))
                    try #require(!(await engine.recordedTrace()).contains(.eventReturned("sent")))
                    let pending = try #require(try await CoreSmokeInspection().cloud(mediaName: name).entities.first)
                    try #require(!pending.acknowledged && !pending.blobPresent)
                }
                await held.open()
                try await original.value
                await declined.open()
                let result = await running.value
                if !cancel {
                    try #require(result.error == nil && !result.deferred)
                    let saved = try #require(try await CoreSmokeInspection().cloud(mediaName: name).entities.first)
                    try #require(saved.acknowledged && saved.ackedRevision == saved.revision && saved.blobPresent)
                    // Exactly collection then media after the declined first
                    // scope. This seam does not force a later-scope wait.
                    try #require(engine.acceptedNetworkCalls.value == calls + 2)
                }
            } catch { failure = error }
            await held.open()
            await declined.open()
            media?.cancel()
            let originalResult = await original.result
            _ = await media?.value
            await fixture.adapter.setSendAdmissionObserver(nil)
            observedAdmission.finish()
            queue.finish()
            await responses.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "Busy media admission cancellation/registration exceeded ten seconds")
            if let failure { throw failure }
            try originalResult.get()
            #expect(try await fixture.core.currentGuard() == guardBefore)
            #expect(await fixture.adapter.retainedInstanceIDs() == [engine.instanceID])
            #expect(await fixture.factory.engines().count == 1)
            #expect(!(await engine.recordedTrace()).contains(.reentrantNetworkCall))
        }
    }

    @Test func nativeMediaCancellationAfterSuccessfulAdmissionFinalizesExactCall() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            guard case .commands(let commands) = try await policy.input(.settings(.init(
                enabled: true, paused: false, automatic: true, media: true))) else {
                throw CloudMediaFailure.missingReply
            }
            let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let permit = try #require(try await policy.prepareExecution(resume))
            let held = CloudCallbackGate()
            let calls = engine.acceptedNetworkCalls.value
            let observations = Mutex<[(sdk: String, id: UInt64, returned: Bool)]>([])
            await fixture.adapter.setSendCallObserver { sdk, id, returned in
                observations.withLock { $0.append((sdk, id, returned)) }
            }
            await fixture.adapter.setSendAdmissionObserver { admitted in
                if admitted { await held.enter() }
            }
            // This is the original media-send task and retained permit, held
            // after Core reserves its first scope but before any SDK call.
            let original = Task<(any Error)?, Never> {
                do { try await fixture.adapter.sendPolicyMedia(execution: permit); return nil }
                catch { return error }
            }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    original.cancel()
                    await held.abort(CancellationError())
                    try? await fixture.adapter.stop()
                }
            }
            var failure: (any Error)?
            do {
                try await held.waitForArrival()
                let before = observations.withLock { $0 }
                try #require(before.count == 1)
                let admission = try #require(before.first)
                try #require(admission.sdk == engine.instanceID && !admission.returned)
                try #require(engine.acceptedNetworkCalls.value == calls)
                original.cancel()
                await held.open()
                let result = await original.value
                try #require(result is CancellationError)
                let after = observations.withLock { $0 }
                try #require(after.count == 2)
                let finished = try #require(after.last)
                // Emitted only after lane-backed Core finishSend returns:
                // Runtime has marked this exact reservation returned.
                try #require(finished.returned && finished.sdk == admission.sdk && finished.id == admission.id)
                try #require(engine.acceptedNetworkCalls.value == calls)
            } catch { failure = error }
            await held.open()
            original.cancel()
            let result = await original.value
            await fixture.adapter.setSendAdmissionObserver(nil)
            await fixture.adapter.setSendCallObserver(nil)
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "Successful media admission cancellation/finalization exceeded ten seconds")
            if let failure { throw failure }
            #expect(result is CancellationError)
            #expect(engine.acceptedNetworkCalls.value == calls)
            #expect(try await fixture.core.currentGuard() == guardBefore)
            #expect(await fixture.adapter.retainedInstanceIDs() == [engine.instanceID])
            #expect(await fixture.factory.engines().count == 1)
            #expect(!(await engine.recordedTrace()).contains(.reentrantNetworkCall))
        }
    }

    @Test func nativeMediaPendingUpdateFailureAfterAdmissionFinalizesExactCall() async throws {
        try await withNativePolicyFixture { fixture in
            let wrapped = Mutex<NativeMediaPendingUpdateEngine?>(nil)
            await fixture.factory.wrapNextEngine { sdk in
                let engine = NativeMediaPendingUpdateEngine(sdk)
                wrapped.withLock { $0 = engine }
                return engine
            }
            let (policy, _) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let pendingEngine = try #require(wrapped.withLock { $0 })
            let guardBefore = try await fixture.core.currentGuard()
            guard case .commands(let commands) = try await policy.input(.settings(.init(
                enabled: true, paused: false, automatic: true, media: true))) else {
                throw CloudMediaFailure.missingReply
            }
            let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let permit = try #require(try await policy.prepareExecution(resume))
            let injected = NativeMediaPendingUpdateFailure()
            let admissions = Mutex(0)
            let observations = Mutex<[(sdk: String, id: UInt64, returned: Bool)]>([])
            let calls = engine.acceptedNetworkCalls.value
            let traceStart = await engine.recordedTrace().count
            await fixture.adapter.setSendCallObserver { sdk, id, returned in
                observations.withLock { $0.append((sdk, id, returned)) }
            }
            await fixture.adapter.setSendAdmissionObserver { admitted in
                guard admitted else { return }
                let count = admissions.withLock { $0 += 1; return $0 }
                // Collection sends normally. Core's second successful beginSend
                // emits the media pending update consumed before the SDK call.
                if count == 2 { await pendingEngine.failNextPendingUpdate(injected) }
            }
            let original = Task<(any Error)?, Never> {
                do { try await fixture.adapter.sendPolicyMedia(execution: permit); return nil }
                catch { return error }
            }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    original.cancel()
                    try? await fixture.adapter.stop()
                }
            }
            // Join the original operation and remove both observers on every
            // outcome before recording the same ten-second cleanup bound.
            let result = await original.value
            await fixture.adapter.setSendAdmissionObserver(nil)
            await fixture.adapter.setSendCallObserver(nil)
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "Post-admission pending-update failure/finalization exceeded ten seconds")
            let keptOriginalError = (result as? NativeMediaPendingUpdateFailure) === injected
            #expect(keptOriginalError)
            #expect(await pendingEngine.injectedFailures == 1)
            #expect(admissions.withLock { $0 } == 2)
            #expect(engine.acceptedNetworkCalls.value == calls + 1)
            let trace = Array((await engine.recordedTrace()).dropFirst(traceStart))
            #expect(trace.filter { if case .sendEntered = $0 { true } else { false } }.count == 1)
            #expect(trace.contains { if case .handed(_, let names) = $0 { names.contains(fixture.names[0]) } else { false } })
            #expect(!trace.contains(.reentrantNetworkCall))
            #expect(try await fixture.core.currentGuard() == guardBefore)
            try await fixture.core.validateSDK(engine.instanceID)
            #expect(await fixture.adapter.retainedInstanceIDs() == [engine.instanceID])
            #expect(await fixture.factory.engines().count == 1)
            let observed = observations.withLock { $0 }
            let admitted = observed.filter { !$0.returned }
            #expect(admitted.count == 2)
            #expect(observed.count == 4)
            for call in admitted {
                let originalOwner = call.sdk == engine.instanceID
                #expect(originalOwner)
                let retirements = observed.filter { $0.returned && $0.sdk == call.sdk && $0.id == call.id }.count
                #expect(retirements == 1, "Each original admitted call must finalize exactly once")
            }
        }
    }

    @Test func nativeMediaNonbusySendDeclineFailsWithoutSDKInvocation() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            guard case .commands(let commands) = try await policy.input(.settings(.init(
                enabled: true, paused: false, automatic: true, media: true))) else {
                throw CloudMediaFailure.missingReply
            }
            let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let permit = try #require(try await policy.prepareExecution(resume))
            // A real Runtime exchange's before-send fetch phase declines even
            // though there is NO retained handler. This tests a nonbusy nil;
            // closed gates and budget exhaustion have no isolated Swift seam.
            let exchange = try await fixture.core.beginExchange(sdk: engine.instanceID)
            let calls = engine.acceptedNetworkCalls.value
            let running = Task<(any Error)?, Never> {
                do { try await fixture.adapter.sendPolicyMedia(execution: permit); return nil }
                catch { return error }
            }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    running.cancel()
                    try? await fixture.adapter.stop()
                }
            }
            let failure = await running.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            _ = try await fixture.core.completeExchange(sdk: engine.instanceID, id: exchange, fetched: false, cancelled: true)
            #expect(inTime, "Nonbusy nil admission must not wait for a handler")
            if case .busy? = failure as? CloudMediaFailure {} else {
                Issue.record("Nonbusy nil did not report explicit busy failure: \(String(describing: failure))")
            }
            #expect(engine.acceptedNetworkCalls.value == calls)
            #expect(await fixture.factory.engines().count == 1)
        }
    }
    #endif

    @Test func nativeMediaFailureReachesPolicyAsReadWithoutSDKCalls() async throws {
        try await withNativePolicyFixture { fixture in
            let queue = HeldNativeMediaQueue()
            let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("media-errors"), available: { UInt64.max })
            let (policy, setup) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
            let runner = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: NativeCloudMediaTransport(queue: queue, staging: staging))
            let enabled = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true)))
            guard case .commands(let commands) = enabled else { throw CloudMediaFailure.missingReply }
            let command = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let engine = try await fixture.engine()
            let calls = engine.acceptedNetworkCalls.value
            let running = Task { await runner.execute(command) }
            var reads = queue.issued.makeAsyncIterator()
            let operation = try #require(await reads.next())
            operation.fetchRecordsResultBlock?(.failure(CKError(.serviceUnavailable, userInfo: [CKErrorRetryAfterKey: 120.0])))
            let result = await running.value
            queue.finish()
            let failure = try #require(result.error as? CloudKitSDKOperationFailure)
            #expect(failure.code == Int64(CKError.Code.serviceUnavailable.rawValue))
            #expect(failure.retryAfterMilliseconds == 120_000)
            #expect(!result.mediaCancellation && !(result.error is CancellationError))
            let status = try await policy.status()
            let detail = try #require(status.error)
            #expect(detail.code == failure.code && detail.retryAfterMilliseconds == 120_000)
            #expect(detail.error == .throttled)
            #expect(detail.operation.zone == .media && detail.operation.kind == .read)
            #expect(engine.acceptedNetworkCalls.value == calls)
            let refreshed = try await policy.input(.refresh(.cloud))
            guard case .commands(let followups) = refreshed else { throw CloudMediaFailure.missingReply }
            for followup in followups {
                switch followup.action {
                case .readBlobs, .resumeMedia, .requestSend: Issue.record("read failure immediately scheduled more media work")
                default: break
                }
            }
        }
    }

    #if DEBUG
    @Test func debugMalformedMediaCallbackKeepsPolicyZeroWithoutSDKCalls() async throws {
        let diagnostics = DebugCloudDiagnostics.Recorder()
        try await DebugCloudDiagnostics.$mediaRecorder.withValue(diagnostics) {
            try await withNativePolicyFixture { fixture in
                let queue = HeldNativeMediaQueue()
                let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("media-errors"), available: { UInt64.max })
                let (policy, setup) = try await nativePolicy(fixture)
                try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
                let runner = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                    mediaService: try await fixture.core.media(), mediaTransport: NativeCloudMediaTransport(queue: queue, staging: staging))
                let enabled = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true)))
                guard case .commands(let commands) = enabled else { throw CloudMediaFailure.missingReply }
                let command = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
                let engine = try await fixture.engine()
                let calls = engine.acceptedNetworkCalls.value
                let running = Task { await runner.execute(command) }
                var reads = queue.issued.makeAsyncIterator()
                let operation = try #require(await reads.next())
                operation.fetchRecordsResultBlock?(.success(()))
                let result = await running.value
                queue.finish()
                let failure = try #require(result.error as? CloudKitSDKOperationFailure)
                #expect(failure.code == 0)
                #expect(failure.retryAfterMilliseconds == nil)
                #expect(!result.mediaCancellation && !(result.error is CancellationError))
                let status = try await policy.status()
                let detail = try #require(status.error)
                #expect(detail.code == failure.code && detail.retryAfterMilliseconds == nil)
                #expect(detail.error == .other)
                #expect(detail.operation.zone == .media && detail.operation.kind == .read)
                let beforeStatus = try #require(diagnostics.latestValue)
                diagnostics.emitStatus(.status(detail.error, zone: detail.operation.zone, kind: detail.operation.kind, code: detail.code)) { _ in }
                #expect(diagnostics.latestValue == "boundary=policyStatus action=none failure=other zone=media kind=read code=0 \(beforeStatus)")
                #expect(beforeStatus.contains("mediaStage=terminalCallback mediaCategory=missingCallback"))
                #expect(engine.acceptedNetworkCalls.value == calls)
                let refreshed = try await policy.input(.refresh(.cloud))
                guard case .commands(let followups) = refreshed else { throw CloudMediaFailure.missingReply }
                for followup in followups {
                    switch followup.action {
                    case .readBlobs, .resumeMedia, .requestSend: Issue.record("read failure immediately scheduled more media work")
                    default: break
                    }
                }
            }
        }
    }
    #endif

    #if DEBUG
    @Test func debugMediaCoreNextRejectionHasFixedEvidenceWithoutSDKCalls() async throws {
        let diagnostics = DebugCloudDiagnostics.Recorder()
        try await DebugCloudDiagnostics.$mediaRecorder.withValue(diagnostics) {
            try await withNativePolicyFixture { fixture in
                let queue = NativeOwnCapturingQueue()
                let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("debug-core"), available: { UInt64.max })
                let executor = CloudMediaExecutor(core: try await fixture.core.media(), sdk: "synthetic-unowned-sdk",
                    transport: NativeCloudMediaTransport(queue: queue, staging: staging))
                do { _ = try await executor.runNext(); Issue.record("Core accepted an unowned SDK") }
                catch { #expect(error is BridgeError) }
                #expect(diagnostics.latestValue == "mediaRequest=next mediaStage=coreNext mediaCategory=coreRejected")
                #expect(queue.operations.withLock { $0.isEmpty })
                #expect(await fixture.factory.engines().isEmpty)
            }
        }
    }

    @Test(arguments: [false, true])
    func debugMediaDownloadsBlockedRejectionHasFixedEvidenceWithoutSDKCalls(download: Bool) async throws {
        let diagnostics = DebugCloudDiagnostics.Recorder()
        try await DebugCloudDiagnostics.$mediaRecorder.withValue(diagnostics) {
            try await withNativePolicyFixture { fixture in
                let (policy, _) = try await nativePolicy(fixture)
                let engine = try await fixture.engine()
                let sdk = engine.instanceID
                let calls = engine.acceptedNetworkCalls.value
                let media = try await fixture.core.media()
                if download { try await prepareDebugMediaDownload(fixture, policy: policy, media: media, sdk: sdk) }

                let queue = NativeOwnCapturingQueue()
                let prepared = Mutex(false)
                let selectedOnce = Mutex(false)
                let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("debug-core"), available: { UInt64.max })
                let transport = NativeMediaPreparationProbe(native: NativeCloudMediaTransport(queue: queue, staging: staging),
                    observe: { prepared.withLock { $0 = true } })
                let executor = CloudMediaExecutor(core: media, sdk: sdk, transport: transport, nextWorkForTesting: {
                    let work = try await media.nextWork(sdk: sdk)
                    if download {
                        let selected = try #require(work)
                        guard case .download = selected.request else { throw CloudMediaFailure.missingReply }
                    } else { try #require(work == nil) }
                    try #require(selectedOnce.withLock { let previous = $0; $0 = true; return !previous })
                    try await fixture.core.retireSDK(sdk, policyStop: false)
                    return work
                })
                // Replace stale evidence as well as populating an empty recorder.
                if download {
                    diagnostics.retainMedia(.init(request: .reads, stage: .recordCallback, category: .duplicateCallback))
                }
                var propagated: BridgeError?
                do { _ = try await executor.runNext(); Issue.record("Core accepted a retired SDK") }
                catch { propagated = try #require(error as? BridgeError) }
                let failure = try #require(propagated)
                // downloads_blocked maps CoreError.Unavailable to this exact
                // BridgeError, including its reason; it is not CloudPolicy.
                guard case .unavailable = failure else { Issue.record("Core error mapping changed"); return }
                do { _ = try await media.downloadsBlocked(sdk: sdk); Issue.record("Direct Core call accepted a retired SDK") }
                catch { #expect(error as? BridgeError == failure) }
                let kind = download ? "download" : "next"
                #expect(diagnostics.latestValue == "mediaRequest=\(kind) mediaStage=coreAdmission mediaCategory=coreRejected")
                #expect(selectedOnce.withLock { $0 })
                #expect(!prepared.withLock { $0 })
                #expect(queue.operations.withLock { $0.isEmpty })
                #expect(engine.acceptedNetworkCalls.value == calls)
                #expect(await fixture.factory.engines().count == 1)
            }
        }
    }
    #endif

    @Test func nativeMediaSDKCancellationWrapperRemainsUnqualifiedWithCoreCancellation() async throws {
        try await withNativePolicyFixture { fixture in
            let queue = HeldNativeMediaQueue()
            defer { queue.finish() }
            let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("media-sdk-cancel"), available: { UInt64.max })
            let (policy, setup) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: NativeCloudMediaTransport(queue: queue, staging: staging))
            guard case .commands(let commands) = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true))) else {
                throw CloudMediaFailure.missingReply
            }
            let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let original = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            let calls = original.acceptedNetworkCalls.value
            let running = Task { await executor.execute(resume) }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    running.cancel()
                    try? await fixture.adapter.stop()
                }
            }
            let responses = Task {
                for await operation in queue.issued {
                    operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled, userInfo: [CKErrorRetryAfterKey: 120.0])))
                }
            }
            let result = await running.value
            queue.finish()
            await responses.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "SDK cancellation wrapper drain exceeded ten seconds")
            let wrapper = try #require(result.error as? CloudKitSDKOperationFailure)
            #expect(wrapper.code == Int64(CKError.Code.operationCancelled.rawValue))
            #expect(wrapper.retryAfterMilliseconds == 120_000)
            #expect(cloudOperationWasCancelled(wrapper) && !(result.error is CancellationError))
            #expect(!result.mediaCancellation && !result.deferred && !result.outputs.isEmpty)
            // Core cancels native activity for SDK 20; it does not publish a
            // throttling error as it does for the serviceUnavailable control.
            #expect(try await policy.status().error == nil)
            #expect(original.acceptedNetworkCalls.value == calls)
            let guardPreserved = try await fixture.core.currentGuard() == guardBefore
            let originalRetained = await fixture.adapter.retainedInstanceIDs() == [original.instanceID]
            #expect(guardPreserved && originalRetained)
            #expect(!(await original.recordedTrace()).contains(.released))
        }
    }

    #if DEBUG
    @Test func nativeMediaEarlierSourceCancellationRemainsUnqualifiedAfterLaterPause() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let original = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            let calls = original.acceptedNetworkCalls.value
            let held = CloudCallbackGate()
            let cancellation = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let evidence = Mutex((sourceCaught: false, cancellationObserved: false, mediaReturned: false, pauseReturned: false))
            let callEvidence = Mutex<(admitted: [UInt64], finalized: [UInt64], ownersMatch: Bool, capped: Bool)>(([], [], true, false))
            let expectedOwner = original.instanceID
            await fixture.adapter.setSendCallObserver { owner, id, finalized in
                callEvidence.withLock {
                    $0.ownersMatch = $0.ownersMatch && owner == expectedOwner
                    if finalized {
                        if $0.finalized.count < 8 { $0.finalized.append(id) } else { $0.capped = true }
                    } else {
                        if $0.admitted.count < 8 { $0.admitted.append(id) } else { $0.capped = true }
                    }
                }
            }
            await fixture.adapter.setSendErrorObserver {
                evidence.withLock { $0.sourceCaught = true }
                await withTaskCancellationHandler {
                    await held.enter()
                } onCancel: {
                    evidence.withLock { $0.cancellationObserved = true }
                    cancellation.continuation.yield(true)
                }
            }
            let queue = HeldNativeMediaQueue()
            let transport = NativeCloudMediaTransport(queue: queue,
                staging: try .init(directory: fixture.directory.appendingPathComponent("earlier-source-media"), available: { UInt64.max }))
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: transport)
            let name = try await CoreMediaService().addMedia(name: "earlier-source.asset", bytes: [3, 1, 4, 1, 5, 9])
            guard case .commands(let commands) = try await policy.input(.settings(.init(
                enabled: true, paused: false, automatic: true, media: true))) else {
                throw CloudMediaFailure.missingReply
            }
            let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            await original.failNextOperation(CancellationError())
            let running = Task {
                let result = await executor.execute(resume)
                evidence.withLock { $0.mediaReturned = true }
                return result
            }
            let deadline = CloudPolicyReadinessDecision()
            // Arm before waiting for any queue arrival or source-catch gate.
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    running.cancel()
                    cancellation.continuation.finish()
                    await held.abort(CancellationError())
                    try? await fixture.adapter.stop()
                }
            }
            let responses = Task {
                for await operation in queue.issued {
                    for id in operation.recordIDs ?? [] {
                        if id.recordName == "root" {
                            let root = CKRecord(recordType: "MGMediaRoot", recordID: id)
                            root.encryptedValues["e"] = (Data([0x08, 0x01, 0x12, 0x20])
                                + Data(repeating: 47, count: 32)) as CKRecordValue
                            operation.perRecordResultBlock?(id, .success(root))
                        } else {
                            operation.perRecordResultBlock?(id, .failure(CKError(.unknownItem)))
                        }
                    }
                    operation.fetchRecordsResultBlock?(.success(()))
                }
            }
            var pause: Task<CloudPolicyNativeResult, any Error>?
            var failure: (any Error)?
            do {
                try await held.waitForArrival()
                try #require(evidence.withLock { $0.sourceCaught && !$0.cancellationObserved && !$0.mediaReturned })
                pause = Task {
                    guard case .commands(let commands) = try await policy.input(.settings(.init(
                        enabled: true, paused: false, automatic: true, media: false))) else {
                        throw CloudMediaFailure.missingReply
                    }
                    let command = try #require(commands.first { if case .pauseMedia = $0.action { true } else { false } })
                    let result = await executor.execute(command)
                    evidence.withLock { $0.pauseReturned = true }
                    return result
                }
                var cancellations = cancellation.stream.makeAsyncIterator()
                try #require(await cancellations.next() == true)
                try #require(evidence.withLock { $0.cancellationObserved && !$0.mediaReturned })
                let originalRetained = await fixture.adapter.retainedInstanceIDs() == [original.instanceID]
                try #require(originalRetained)
                try await fixture.core.validateSDK(original.instanceID)
            } catch { failure = error; running.cancel() }
            // Release before joining original media, Pause and the responder on
            // every path. No assertion can abandon a held original operation.
            await held.open()
            let result = await running.value
            let paused = await pause?.result
            await fixture.adapter.setSendErrorObserver(nil)
            await fixture.adapter.setSendCallObserver(nil)
            cancellation.continuation.finish()
            queue.finish()
            await responses.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "Earlier source error/later Pause drain exceeded ten seconds")
            if let failure { throw failure }
            let pauseResult = try #require(try paused?.get())
            #expect(pauseResult.error == nil && !pauseResult.deferred && !pauseResult.mediaCancellation)
            #expect(result.error is CancellationError)
            #expect(!result.mediaCancellation && !result.deferred && !result.outputs.isEmpty)
            let observed = callEvidence.withLock { $0 }
            let exactCalls = observed.admitted.count == 1 && observed.admitted == observed.finalized
            #expect(exactCalls && observed.ownersMatch && !observed.capped)
            #expect(original.acceptedNetworkCalls.value == calls + 1)
            let pending = try #require(try await CoreSmokeInspection().cloud(mediaName: name).entities.first)
            #expect(!pending.acknowledged && !pending.blobPresent)
            let guardPreserved = try await fixture.core.currentGuard() == guardBefore
            let originalRetained = await fixture.adapter.retainedInstanceIDs() == [original.instanceID]
            #expect(guardPreserved && originalRetained)
            #expect(await fixture.factory.engines().count == 1)
            #expect(!(await original.recordedTrace()).contains(.released))
        }
    }
    #endif

    @Test(arguments: [false, true])
    func nativeMediaUnrequestedErrorsRemainUnqualified(cancellation: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let queue = HeldNativeMediaQueue()
            let (_, cancelled) = AsyncStream<Void>.makeStream()
            defer { cancelled.finish(); queue.finish() }
            let injected = NativeMediaReadFailure()
            let failure: any Error = cancellation ? CancellationError() : injected
            let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("unrequested-media"), available: { UInt64.max })
            let transport = ObservedMediaTransport(native: .init(queue: queue, staging: staging), cancelled: cancelled, failure: failure)
            let (policy, setup) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: transport)
            guard case .commands(let enabled) = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true))) else {
                throw CloudMediaFailure.missingReply
            }
            let resume = try #require(enabled.first { if case .resumeMedia = $0.action { true } else { false } })
            let original = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            let calls = original.acceptedNetworkCalls.value
            let running = Task { await executor.execute(resume) }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    running.cancel()
                    try? await fixture.adapter.stop()
                }
            }
            let responses = Task {
                var first: CKFetchRecordsOperation?
                for await operation in queue.issued {
                    if first == nil { first = operation }
                    // The transport throws only after its original native callback.
                    // Neither the caller nor Pause requested this cancellation.
                    operation.fetchRecordsResultBlock?(.failure(CKError(.serviceUnavailable)))
                }
                return first
            }
            let result = await running.value
            queue.finish()
            let first = await responses.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "Unrequested media failure drain exceeded ten seconds")
            let operation = try #require(first)
            #expect(!operation.isCancelled && !running.isCancelled)
            #expect(!result.mediaCancellation && !result.deferred && !result.outputs.isEmpty)
            if cancellation { #expect(result.error is CancellationError) }
            else { #expect((result.error as? NativeMediaReadFailure) === injected) }
            #expect(original.acceptedNetworkCalls.value == calls)
            let guardPreserved = try await fixture.core.currentGuard() == guardBefore
            let originalRetained = await fixture.adapter.retainedInstanceIDs() == [original.instanceID]
            #expect(guardPreserved && originalRetained)
            #expect(!(await original.recordedTrace()).contains(.released))
        }
    }

    #if DEBUG
    @Test func nativePolicyMediaStopJoinsPauseDrainWithOriginalWitness() async throws {
        // Count the original native work without changing its callback or
        // cancellation semantics. Stop must only join Pause's existing drain.
        struct Counts {
            var prepared = 0, enqueued = 0, replies = 0, cancelled = 0
            var context: CloudMediaCancellationContext?
            var sameContext = true
        }
        final class Observations: Sendable {
            let counts = Mutex(Counts())
        }
        struct Read: CloudPreparedMediaRead {
            let original: any CloudPreparedMediaRead
            let observations: Observations
            func enqueue() -> Bool {
                observations.counts.withLock { $0.enqueued += 1 }
                return original.enqueue()
            }
            func reply() async throws -> CloudMediaTransportReply {
                let context = CloudMediaCancellationContext.current
                observations.counts.withLock {
                    $0.replies += 1
                    $0.sameContext = $0.sameContext && context != nil && context === $0.context
                }
                return try await original.reply()
            }
            func cancel() {
                observations.counts.withLock { $0.cancelled += 1 }
                original.cancel()
            }
        }
        struct Transport: CloudMediaTransport {
            let original: ObservedMediaTransport
            let observations: Observations
            func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead {
                let context = CloudMediaCancellationContext.current
                observations.counts.withLock {
                    $0.prepared += 1
                    if $0.context == nil { $0.context = context }
                    $0.sameContext = $0.sameContext && context != nil && context === $0.context
                }
                return Read(original: try original.prepare(request), observations: observations)
            }
        }
        try await withNativePolicyFixture { fixture in
            let queue = HeldNativeMediaQueue()
            let (cancellations, cancelled) = AsyncStream<Void>.makeStream()
            let held = CloudCallbackGate(), endRead = CloudCallbackGate()
            let (drainJoins, drainJoined) = AsyncStream<String>.makeStream()
            let observations = Observations()
            let callbacks = Mutex(0)
            let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("pause-then-stop-media"), available: { UInt64.max })
            let transport = Transport(original: .init(native: .init(queue: queue, staging: staging), cancelled: cancelled), observations: observations)
            let (policy, setup) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: transport)
            let name = try await CoreMediaService().addMedia(name: "pause-then-stop.asset", bytes: [2, 6, 1, 0, 2])
            guard case .commands(let commands) = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true))) else {
                throw CloudMediaFailure.missingReply
            }
            let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let original = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            let calls = original.acceptedNetworkCalls.value
            let returned = Mutex((media: 0, pause: 0, stop: 0))
            let enginesBefore = await fixture.factory.engines().map(\.instanceID)
            let originalSDK = original.instanceID
            await executor.setExistingMediaDrainObserver { drainJoined.yield($0) }
            await executor.setMediaEndReadObserver { sdk in
                if sdk == originalSDK { await endRead.enter() }
            }
            let running = Task {
                let result = await executor.execute(resume)
                returned.withLock { $0.media += 1 }
                return result
            }
            let deadline = CloudPolicyReadinessDecision()
            // Arm before the first callback arrival or cancellation wait.
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    running.cancel()
                    cancelled.finish()
                    await held.abort(CancellationError())
                    await endRead.abort(CancellationError())
                    drainJoined.finish()
                    try? await fixture.adapter.stop()
                }
            }
            let responses = Task {
                for await operation in queue.issued {
                    callbacks.withLock { $0 += 1 }
                    #expect(operation.desiredKeys == ["e"])
                    await held.enter()
                    #expect(operation.isCancelled)
                    operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled)))
                }
            }
            var pause: Task<CloudPolicyNativeResult, any Error>?
            var stopping: Task<Void, any Error>?
            var failure: (any Error)?
            do {
                try await held.waitForArrival()
                pause = Task {
                    guard case .commands(let commands) = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))) else {
                        throw CloudMediaFailure.missingReply
                    }
                    let command = try #require(commands.first { if case .pauseMedia = $0.action { true } else { false } })
                    let result = await executor.execute(command)
                    returned.withLock { $0.pause += 1 }
                    return result
                }
                var cancelledReads = cancellations.makeAsyncIterator()
                // The run's cancellation handler and worker.close each cancel
                // this same read. The second observation locates Pause inside
                // its already installed drain, waiting on the held callback.
                try #require(await cancelledReads.next() != nil)
                try #require(await cancelledReads.next() != nil)
                try #require(observations.counts.withLock { $0.cancelled == 2 })
                stopping = Task {
                    try await fixture.adapter.stop()
                    returned.withLock { $0.stop += 1 }
                }
                var joins = drainJoins.makeAsyncIterator()
                // This branch exists on RED too: observe Stop joining Pause's
                // actual drain, without requiring a successfully attached witness.
                try #require(await joins.next() == original.instanceID)
                try await fixture.core.validateSDK(original.instanceID)
                try #require(await fixture.adapter.retainedInstanceIDs() == [original.instanceID])
                try #require(!(await original.recordedTrace()).contains(.released))
                try #require(returned.withLock { $0.media == 0 && $0.pause == 0 && $0.stop == 0 })
                try #require(observations.counts.withLock { $0.cancelled == 2 && $0.prepared == 1 && $0.enqueued == 1 && $0.replies == 1 })
                try #require(callbacks.withLock { $0 == 1 })
                try #require(try await fixture.core.currentGuard() == guardBefore)
                await held.open()
                try await endRead.waitForArrival()
                let stop = try #require(stopping)
                try await stop.value
                let guardAfter = try await fixture.core.currentGuard()
                #expect(guardAfter.generation == guardBefore.generation)
                #expect(guardAfter.binding == guardBefore.binding)
                #expect(guardAfter.token > guardBefore.token)
                try #require(await fixture.adapter.retainedInstanceIDs().isEmpty)
                try #require(await original.recordedTrace().filter { $0 == .released }.count == 1)
                await #expect(throws: (any Error).self) { try await fixture.core.validateSDK(originalSDK) }
            } catch { failure = error; running.cancel() }
            // Release every gate before the remaining joins, including assertion
            // failures and watchdog expiry. No original callback escapes.
            await held.open()
            await endRead.open()
            let result = await running.value
            let paused = await pause?.result
            let stopped = await stopping?.result
            await executor.setExistingMediaDrainObserver(nil)
            await executor.setMediaEndReadObserver(nil)
            drainJoined.finish()
            cancelled.finish()
            queue.finish()
            await responses.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "Pause-owned media drain joined by Stop exceeded ten seconds")
            if let failure { throw failure }
            let pauseResult = try #require(try paused?.get())
            _ = try #require(try stopped?.get())
            #expect(pauseResult.error == nil || (pauseResult.error as? BridgeError) == .cloudPolicy(.staleCommand))
            #expect(!pauseResult.mediaCancellation && !pauseResult.deferred && !pauseResult.outputs.isEmpty)
            #expect(result.error is CancellationError)
            #expect(result.mediaCancellation)
            #expect(!result.deferred && !result.outputs.isEmpty)
            #expect(returned.withLock { $0.media == 1 && $0.pause == 1 && $0.stop == 1 })
            #expect(observations.counts.withLock { $0.context != nil && $0.sameContext && $0.prepared == 1 && $0.enqueued == 1 && $0.replies == 1 && $0.cancelled == 2 })
            #expect(callbacks.withLock { $0 == 1 })
            #expect(original.acceptedNetworkCalls.value == calls)
            let pending = try #require(try await CoreSmokeInspection().cloud(mediaName: name).entities.first)
            #expect(!pending.acknowledged && !pending.blobPresent)
            #expect(await fixture.factory.engines().map(\.instanceID) == enginesBefore)
            #expect(await fixture.adapter.retainedInstanceIDs().isEmpty)
            #expect(await original.recordedTrace().filter { $0 == .released }.count == 1)
        }
    }
    #endif

    @Test(arguments: [false, true]) func nativePolicyMediaPauseAndStopDrainOriginalCallback(stop: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let queue = HeldNativeMediaQueue()
            let (cancellations, cancelled) = AsyncStream<Void>.makeStream()
            let staging = try CloudMediaStaging(directory: fixture.directory.appendingPathComponent("media-runs"), available: { UInt64.max })
            let transport = ObservedMediaTransport(native: .init(queue: queue, staging: staging), cancelled: cancelled)
            let (policy, setup) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                mediaService: try await fixture.core.media(), mediaTransport: transport)
            let enabled = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true)))
            guard case .commands(let commands) = enabled else { throw CloudMediaFailure.missingReply }
            let resume = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let original = try await fixture.engine()
            let returned = Mutex((media: false, control: false))
            let running = Task {
                let result = await executor.execute(resume)
                returned.withLock { $0.media = true }
                queue.finish()
                return result
            }
            var reads = queue.issued.makeAsyncIterator()
            let operation = try #require(await reads.next())
            #expect(operation.desiredKeys == ["e"])
            defer { operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled))) }
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() {
                    running.cancel()
                    cancelled.finish()
                    operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled)))
                    try? await fixture.adapter.stop()
                }
            }
            let stopping = Task {
                defer { returned.withLock { $0.control = true }; cancelled.finish() }
                if stop { try await fixture.adapter.stop() }
                else {
                    let disabled = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
                    guard case .commands(let commands) = disabled else { throw CloudMediaFailure.missingReply }
                    let pause = try #require(commands.first { if case .pauseMedia = $0.action { true } else { false } })
                    let result = await executor.execute(pause)
                    if let error = result.error { throw error }
                    #expect(!result.mediaCancellation && !result.deferred)
                }
            }
            var cancelledReads = cancellations.makeAsyncIterator()
            _ = await cancelledReads.next()
            #expect(operation.isCancelled)
            #expect(await fixture.adapter.retainedInstanceIDs().contains(original.instanceID))
            #expect(!(await original.recordedTrace()).contains(.released))
            #expect(!returned.withLock { $0.media || $0.control })
            var validationFailure: (any Error)?
            if stop {
                do { try await fixture.core.validateSDK(original.instanceID) }
                catch { validationFailure = error }
            }
            operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled)))
            let stopped = await stopping.result
            let result = await running.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
            #expect(inTime, "Original media callback/control drain exceeded ten seconds")
            try stopped.get()
            if let validationFailure { throw validationFailure }
            #expect(result.error != nil)
            #expect(result.error is CancellationError)
            #expect(result.mediaCancellation)
            #expect(!result.deferred && !result.outputs.isEmpty)
            if stop { #expect(await original.recordedTrace().contains(.released)) }
        }
    }
}
#endif
