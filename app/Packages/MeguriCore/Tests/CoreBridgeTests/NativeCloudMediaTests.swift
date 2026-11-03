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

#if DEBUG
private final class NativeMediaPendingUpdateFailure: Error {}

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
    @Test func nativeMediaUploadsResumeAfterUnrelatedPersistedSentHandlerReturns() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let guardBefore = try await fixture.core.currentGuard()
            let held = CloudCallbackGate()
            await fixture.probe.pauseSent(held)
            let (admissions, observedAdmission) = AsyncStream<Bool>.makeStream()
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
                    observedAdmission.finish()
                    try? await fixture.adapter.stop()
                }
            }
            // This call owns its sent event and cannot return before that
            // original event does. It is unrelated to the later media round.
            let original = Task { try await fixture.adapter.requestSend(zones: [fixture.zone]) }
            var media: Task<CloudPolicyNativeResult, Never>?
            var names: [String] = []
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
                admissionEvidence.withLock { $0.handlerReleased = true }
                await held.open()
                try await original.value
                _ = await running.value
            } catch { failure = error }

            // All paths release callbacks before joining every owned task.
            // Deadline assertions come only after these joins, including the
            // watchdog's failure-only stop/drain and the native responder.
            admissionEvidence.withLock { $0.handlerReleased = true }
            await held.open()
            media?.cancel()
            let originalResult = await original.result
            let mediaResult = await media?.value
            await fixture.adapter.setSendAdmissionObserver(nil)
            await fixture.adapter.setSendCallObserver(nil)
            observedAdmission.finish()
            queue.finish()
            await responses.value
            let inTime = deadline.observeReady()
            watchdog.cancel()
            await watchdog.value
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
            // IDs remain internal to the bounded exact-call comparison.
            let diagnostic = "mediaFailure=\(mediaFailure) adapterFailure=\(adapterFailure)"
                + " sdkCallsSinceOriginal=\(engine.acceptedNetworkCalls.value - sdkCallsBeforeOriginal)"
                + " admittedCalls=\(calls.admitted.count) finalizedCalls=\(calls.finalized.count)"
                + " exactCallsMatch=\(exactCallsMatch) admissionOwnersMatch=\(calls.admissionOwnersMatch)"
                + " finalizationOwnersMatch=\(calls.finalizationOwnersMatch) callEvidenceCapped=\(calls.capped)"
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
