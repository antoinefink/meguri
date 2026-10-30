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
    func prepare(_ request: CloudMediaRequest) throws -> any CloudPreparedMediaRead {
        ObservedMediaRead(native: try native.prepare(request), cancelled: cancelled)
    }
}
private struct ObservedMediaRead: CloudPreparedMediaRead {
    let native: any CloudPreparedMediaRead
    let cancelled: AsyncStream<Void>.Continuation
    func enqueue() -> Bool { native.enqueue() }
    func reply() async throws -> CloudMediaTransportReply { try await native.reply() }
    func cancel() { native.cancel(); cancelled.yield(()) }
}

extension EventTests {
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
            let running = Task { let result = await executor.execute(resume); queue.finish(); return result }
            var reads = queue.issued.makeAsyncIterator()
            let operation = try #require(await reads.next())
            #expect(operation.desiredKeys == ["e"])
            defer { operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled))) }
            let stopping = Task {
                defer { cancelled.finish() }
                if stop { try await fixture.adapter.stop() }
                else {
                    let disabled = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
                    guard case .commands(let commands) = disabled else { throw CloudMediaFailure.missingReply }
                    let pause = try #require(commands.first { if case .pauseMedia = $0.action { true } else { false } })
                    let result = await executor.execute(pause)
                    if let error = result.error { throw error }
                }
            }
            var cancelledReads = cancellations.makeAsyncIterator()
            _ = await cancelledReads.next()
            #expect(operation.isCancelled)
            #expect(await fixture.adapter.retainedInstanceIDs().contains(original.instanceID))
            #expect(!(await original.recordedTrace()).contains(.released))
            if stop { try await fixture.core.validateSDK(original.instanceID) }
            operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled)))
            try await stopping.value
            #expect(await running.value.error != nil)
            if stop { #expect(await original.recordedTrace().contains(.released)) }
        }
    }
}
#endif
