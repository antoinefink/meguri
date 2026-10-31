#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

private final class NativeAssetReadHandlers: Sendable {
    private let perRecord: Mutex<(CKRecord.ID, Result<CKRecord, any Error>) -> Void>
    private let terminal: Mutex<(Result<Void, any Error>) -> Void>

    init(_ operation: CKFetchRecordsOperation) throws {
        perRecord = Mutex(try #require(operation.perRecordResultBlock))
        terminal = Mutex(try #require(operation.fetchRecordsResultBlock))
    }
    func receive(_ id: CKRecord.ID, _ result: Result<CKRecord, any Error>) {
        perRecord.withLock { $0(id, result) }
    }
    func finish(_ result: Result<Void, any Error>) {
        terminal.withLock { $0(result) }
    }
}

/// Real installed CKFetchRecordsOperation handlers, captured once before any
/// invocation: SDK getter round-trips can normalize callback results.
private struct NativeAssetReadFixture: Sendable {
    let directory: URL
    let source: URL
    let copies: URL
    let queue: NativeOwnCapturingQueue
    let prepared: any CloudPreparedOwnOperation
    let operation: CKFetchRecordsOperation
    private let handlers: NativeAssetReadHandlers
    let target = CloudControlTarget(zone: "c-synthetic", name: "exact-entity", recordType: "MGEntity")
    static let bytes = Data(repeating: 0x37, count: 131_073)

    init(beforeSourceRead: (@Sendable (URL, CloudMediaStaging.SourceReadBoundary) throws -> Void)? = nil) throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        source = directory.appendingPathComponent("sdk-pa")
        copies = directory.appendingPathComponent("owned")
        try Self.bytes.write(to: source)
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max }, beforeSourceRead: beforeSourceRead)
        queue = NativeOwnCapturingQueue()
        prepared = try NativeCloudOwnTransport(queue: queue, staging: staging).prepare(.assetRead(zone: target.zone, name: target.name))
        // Check through the additional protocol, preserving the same prepared
        // object that must cross pass.submit's guarded enqueue callback.
        let guardedQueue: any CloudAssetReadQueue = prepared
        #expect(guardedQueue.enqueue())
        operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        handlers = try NativeAssetReadHandlers(operation)
    }
    func perRecord(_ id: CKRecord.ID, _ result: Result<CKRecord, any Error>) {
        handlers.receive(id, result)
    }
    func terminal(_ result: Result<Void, any Error>) {
        handlers.finish(result)
    }
    func record(asset: Bool = true) -> CKRecord {
        let record = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        record.encryptedValues["e"] = Data([1, 3, 5]) as CKRecordValue
        if asset { record["pa"] = CKAsset(fileURL: source) }
        return record
    }
    func ownedFiles() throws -> [URL] {
        try FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil).map { $0.appendingPathComponent("asset") }
    }
    func result() async throws -> CloudAssetReadResult {
        guard case .assetRead(let result) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        return result
    }
    func expectMetadataWithoutAsset(_ native: CKRecord) async throws {
        let result = try await result()
        let record = try #require(result.record)
        #expect(record.name == native.recordID.recordName && record.recordType == native.recordType)
        #expect(record.zone == native.recordID.zoneID.zoneName)
        #expect(record.envelope == (native.encryptedValues["e"] as? Data))
        #expect(record.assetURL == nil)
        let fields = try #require(record.systemFields)
        let restored = try NativeCloudKitSystemFields.restore(fields, target: target)
        #expect(restored.recordID == native.recordID && restored.recordType == native.recordType)
        try await result.withRaw { raw in
            let value = try #require(raw)
            #expect(value.assetPath == nil)
        }
        #expect(try ownedFiles().isEmpty)
    }
    func remove() { try? FileManager.default.removeItem(at: directory) }
}

struct NativeCloudAssetReadTransportTests {
    @Test func exactOneIDAllFieldsAndCopiedPayloadSurvivesSDKAndAwaitedHandoff() async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        let id = NativeCloudOwnRecords.id(read.target)
        #expect(read.operation.recordIDs == [id])
        #expect(id.zoneID.zoneName == read.target.zone && id.zoneID.ownerName == CKCurrentUserDefaultName)
        #expect(read.operation.desiredKeys == nil)
        let native = read.record()
        let sourceAttributes = try FileManager.default.attributesOfItem(atPath: read.source.path)
        let sourceDevice = try #require(sourceAttributes[.systemNumber] as? NSNumber)
        let sourceInode = try #require(sourceAttributes[.systemFileNumber] as? NSNumber)
        read.perRecord(id, .success(native))
        // The SDK can remove its source as soon as the per-record callback
        // returns, while the original native operation remains outstanding.
        try FileManager.default.removeItem(at: read.source)
        let files = try read.ownedFiles()
        #expect(files.count == 1)
        let copied = try #require(files.first)
        #expect(copied != read.source)
        #expect(try Data(contentsOf: copied) == NativeAssetReadFixture.bytes)
        read.terminal(.success(()))
        let gate = CloudCallbackGate()
        let handoff = Task {
            do {
                // A temporary owned result is the only file lease after the
                // original reply is consumed, even across raw conversion.
                try await read.result().withRaw { raw in
                    await gate.enter()
                    let raw = try #require(raw)
                    #expect(raw.name == read.target.name && raw.zone == read.target.zone)
                    #expect(raw.recordType == "MGEntity" && raw.envelope == Data([1, 3, 5]))
                    let rawPath = try #require(raw.assetPath)
                    let rawURL = URL(fileURLWithPath: rawPath)
                    #expect(rawURL != read.source)
                    #expect(try Data(contentsOf: rawURL) == NativeAssetReadFixture.bytes)
                    let rawAttributes = try FileManager.default.attributesOfItem(atPath: rawURL.path)
                    let copiedAttributes = try FileManager.default.attributesOfItem(atPath: copied.path)
                    let rawDevice = try #require(rawAttributes[.systemNumber] as? NSNumber)
                    let rawInode = try #require(rawAttributes[.systemFileNumber] as? NSNumber)
                    let copiedDevice = try #require(copiedAttributes[.systemNumber] as? NSNumber)
                    let copiedInode = try #require(copiedAttributes[.systemFileNumber] as? NSNumber)
                    #expect(rawDevice == copiedDevice && rawInode == copiedInode)
                    #expect(rawDevice != sourceDevice || rawInode != sourceInode)
                    #expect(copiedDevice != sourceDevice || copiedInode != sourceInode)
                    #expect(try Data(contentsOf: copied) == NativeAssetReadFixture.bytes)
                    let fields = try #require(raw.systemFields)
                    let restored = try NativeCloudKitSystemFields.restore(fields, target: read.target)
                    #expect(restored.recordID == id && restored.recordType == "MGEntity")
                }
                _ = await gate.operationSucceeded()
            } catch { await gate.abort(error); throw error }
        }
        do {
            try await gate.waitForArrival()
            #expect(try Data(contentsOf: copied) == NativeAssetReadFixture.bytes)
            await gate.open()
            try await handoff.value
            #expect(try read.ownedFiles().isEmpty)
        } catch {
            await gate.open()
            _ = await handoff.result
            throw error
        }
    }

    @Test func inlineResultNeedsNoLeaseOrPayloadCopy() async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        let native = read.record(asset: false)
        read.perRecord(native.recordID, .success(native))
        read.terminal(.success(()))
        let result = try await read.result()
        #expect(result.record?.name == read.target.name && result.record?.envelope == Data([1, 3, 5]))
        #expect(result.record?.assetURL == nil)
        #expect(try read.ownedFiles().isEmpty)
    }

    enum InvalidRecord: CaseIterable, Sendable {
        case owner, zone, type, name, callbackID, ma, a, unexpectedAsset, invalidPayload, malformedEncrypted, missingEncrypted
    }
    @Test(arguments: InvalidRecord.allCases)
    func invalidActualRecordCannotBecomeSuccessfulIntake(kind: InvalidRecord) async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        let expected = NativeCloudOwnRecords.id(read.target)
        var id = expected
        var type = "MGEntity"
        switch kind {
        case .owner: id = .init(recordName: read.target.name, zoneID: .init(zoneName: read.target.zone, ownerName: "foreign-owner"))
        case .zone: id = .init(recordName: read.target.name, zoneID: NativeCloudOwnRecords.zone("c-other"))
        case .name: id = .init(recordName: "other-entity", zoneID: expected.zoneID)
        case .type: type = "MGEpoch"
        default: break
        }
        let native = CKRecord(recordType: type, recordID: id)
        native.encryptedValues["e"] = Data([1]) as CKRecordValue
        switch kind {
        case .ma: native["ma"] = "unsupported" as CKRecordValue
        case .a: native["a"] = CKAsset(fileURL: read.source)
        case .unexpectedAsset: native["unexpected"] = CKAsset(fileURL: read.source)
        case .invalidPayload: native["pa"] = "not-an-asset" as CKRecordValue
        case .malformedEncrypted: native.encryptedValues["e"] = "not-data" as CKRecordValue
        case .missingEncrypted: native.encryptedValues["e"] = nil
        default: break
        }
        let callbackID = kind == .callbackID ? CKRecord.ID(recordName: "other-callback", zoneID: expected.zoneID) : expected
        read.perRecord(callbackID, .success(native))
        read.terminal(.success(()))
        await #expect(throws: NativeCloudKitFailure.self) { try await read.result() }
        #expect(try read.ownedFiles().isEmpty)
    }

    @Test func duplicateCallbackFailsAndRetainsFirstCopyUntilOriginalTerminal() async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        let native = read.record()
        read.perRecord(native.recordID, .success(native))
        read.perRecord(native.recordID, .success(native))
        #expect(try read.ownedFiles().count == 1)
        read.terminal(.success(()))
        await #expect(throws: NativeCloudKitFailure.self) { try await read.result() }
        #expect(try read.ownedFiles().isEmpty)
    }

    @Test func failedCopyCleansPartialDestinationAndOriginalOperationStillTerminates() async throws {
        let sourceReads = Mutex(0)
        let read = try NativeAssetReadFixture(beforeSourceRead: { _, boundary in
            if case .read = boundary {
                let count = sourceReads.withLock { $0 += 1; return $0 }
                if count == 2 { throw CocoaError(.fileReadNoPermission) }
            }
        })
        defer { read.remove() }
        let native = read.record()
        read.perRecord(native.recordID, .success(native))
        #expect(sourceReads.withLock { $0 } == 2)
        #expect(try read.ownedFiles().isEmpty)
        #expect(!read.operation.isCancelled)
        read.terminal(.success(()))
        try await read.expectMetadataWithoutAsset(native)
        #expect(try Data(contentsOf: read.source) == NativeAssetReadFixture.bytes)
    }

    @Test func missingSDKSourceKeepsActualMetadataWithoutLease() async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        let native = read.record()
        #expect((native["pa"] as? CKAsset)?.fileURL == read.source)
        try FileManager.default.removeItem(at: read.source)
        read.perRecord(native.recordID, .success(native))
        #expect(!read.operation.isCancelled)
        read.terminal(.success(()))
        try await read.expectMetadataWithoutAsset(native)
    }

    @Test func cancellationDuringCopyCleansPartialDestinationAndJoinsOriginalTerminal() async throws {
        let owner = Mutex<(any CloudPreparedOwnOperation)?>(nil)
        let read = try NativeAssetReadFixture(beforeSourceRead: { _, boundary in
            if case .read = boundary { owner.withLock { $0?.cancel() } }
        })
        defer { read.remove() }
        owner.withLock { $0 = read.prepared }
        defer { owner.withLock { $0 = nil } }
        let native = read.record()
        read.perRecord(native.recordID, .success(native))
        #expect(read.operation.isCancelled)
        #expect(try read.ownedFiles().isEmpty)
        read.terminal(.success(()))
        await #expect(throws: CancellationError.self) { try await read.result() }
    }

    @Test func cancelledAcceptedReadKeepsCopyAndWaitsForOriginalTerminal() async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        let native = read.record()
        read.perRecord(native.recordID, .success(native))
        let started = CloudCallbackGate()
        let terminalEntered = Mutex(false)
        let completed = Mutex(false)
        let join = Task {
            await started.enter()
            do { _ = try await read.result(); Issue.record("cancelled read became successful") }
            catch { #expect(error is CancellationError) }
            #expect(terminalEntered.withLock { $0 })
            completed.withLock { $0 = true }
        }
        try await started.waitForArrival()
        read.prepared.cancel()
        join.cancel()
        await started.open()
        #expect(read.operation.isCancelled)
        #expect(!completed.withLock { $0 })
        #expect(try read.ownedFiles().count == 1)
        terminalEntered.withLock { $0 = true }
        // Even a late successful SDK terminal cannot turn cancellation into a
        // successful record; only this original terminal releases the join.
        read.terminal(.success(()))
        await join.value
        #expect(completed.withLock { $0 })
        #expect(read.queue.operations.withLock { $0.count } == 1)
        #expect(try read.ownedFiles().isEmpty)
    }

    @Test func cancelBeforeEnqueueRefusesWithoutNativeReply() throws {
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.assetRead(zone: "c-synthetic", name: "exact-entity"))
        prepared.cancel()
        let guardedQueue: any CloudAssetReadQueue = prepared
        #expect(!guardedQueue.enqueue())
        #expect(queue.operations.withLock { $0.isEmpty })
        // False has no native operation/reply to await.
    }

    @Test(arguments: [0, 1, 2])
    func nativeFailureNeverSynthesizesSuccessfulRecord(source: Int) async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        if source == 0 {
            read.perRecord(NativeCloudOwnRecords.id(read.target), .failure(CKError(.unknownItem)))
        } else if source == 1 {
            let native = read.record()
            read.perRecord(native.recordID, .success(native))
        }
        read.terminal(source == 0 ? .success(()) : .failure(CKError(.networkFailure)))
        #expect(try await read.result().record == nil)
        #expect(try read.ownedFiles().isEmpty)
    }

    @Test func missingPerRecordReplyIsAnErrorAndNonCollectionZoneCannotQueue() async throws {
        let read = try NativeAssetReadFixture()
        defer { read.remove() }
        read.terminal(.success(()))
        await #expect { try await read.result() } throws: { error in
            guard let failure = error as? CloudOwnTransportFailure, case .missingReply = failure else { return false }
            return true
        }
        let queue = NativeOwnCapturingQueue()
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudOwnTransport(queue: queue).prepare(.assetRead(zone: "meguri-root", name: "exact-entity"))
        }
        #expect(queue.operations.withLock { $0.isEmpty })
    }
}
#endif
