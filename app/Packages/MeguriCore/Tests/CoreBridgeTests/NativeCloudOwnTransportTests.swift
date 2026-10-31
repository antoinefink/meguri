#if canImport(CloudKit)
import CloudKit
import Foundation
import Synchronization
import Testing
@testable import CoreBridge

/// Captures real CKDatabaseOperation subclasses; never configures an account
/// or adds them to a live database. Tests drive their original SDK callbacks.
final class NativeOwnCapturingQueue: NativeCloudDatabaseQueue, Sendable {
    let operations = Mutex<[CKDatabaseOperation]>([])
    func add(_ operation: CKDatabaseOperation) -> Bool { operations.withLock { $0.append(operation) }; return true }
}
struct NativeCloudOwnTransportTests {
    let active = CloudControlTarget(zone: "meguri-root", name: "active", recordType: "MGActive")
    let root = CloudControlTarget(zone: "meguri-media", name: "root", recordType: "MGMediaRoot")

    @Test func actualActiveAndEpochReadsRemainEOnlyWhenEntityAssetsAreSupported() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("unexpected-control-asset")
        try Data([7, 5, 3]).write(to: file)
        let epoch = CloudControlTarget(zone: "c-synthetic", name: "epoch", recordType: "MGEpoch")
        for target in [active, epoch] {
            for field in ["pa", "ma", "a"] {
                let queue = NativeOwnCapturingQueue()
                let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.read(target))
                #expect(prepared.enqueue())
                let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
                #expect(operation.desiredKeys == nil)
                let record = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
                record.encryptedValues["e"] = Data([1]) as CKRecordValue
                record[field] = CKAsset(fileURL: file)
                operation.perRecordResultBlock?(record.recordID, .success(record))
                operation.fetchRecordsResultBlock?(.success(()))
                await #expect(throws: NativeCloudKitFailure.self) { try await prepared.reply() }
            }
        }
    }

    @Test(arguments: [CKError.Code.serviceUnavailable, .requestRateLimited])
    func publicationReadsSavesAndZonesForwardActualServerDuration(code: CKError.Code) async throws {
        let retry = CKError(code, userInfo: [CKErrorRetryAfterKey: 120.0])
        for source in 0..<3 {
            for kind in 0..<3 {
                let queue = NativeOwnCapturingQueue()
                let command: CloudOwnOperation
                switch kind {
                case 0: command = .read(root)
                case 1: command = .save(root, encrypted: Data([2]), systemFields: nil)
                default: command = .createZones(["meguri-root", "meguri-media", "c-synthetic"])
                }
                let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.publication(command))
                #expect(prepared.enqueue())
                let operation = try #require(queue.operations.withLock { $0.first })
                let itemID: AnyHashable
                if let zones = operation as? CKModifyRecordZonesOperation {
                    itemID = try #require(zones.recordZonesToSave?.first).zoneID
                } else { itemID = NativeCloudOwnRecords.id(root) }
                let partial = CKError(.partialFailure, userInfo: [CKPartialErrorsByItemIDKey: [itemID: retry]])
                let terminal: Result<Void, any Error> = source == 0 ? .success(()) : .failure(source == 1 ? retry : partial)
                let observed = Mutex<Bool?>(nil)
                if let fetch = operation as? CKFetchRecordsOperation {
                    let original = try #require(fetch.fetchRecordsResultBlock)
                    fetch.fetchRecordsResultBlock = { result in
                        observed.withLock { if case .success = result { $0 = true } else { $0 = false } }
                        original(result)
                    }
                    if source != 1 { fetch.perRecordResultBlock?(NativeCloudOwnRecords.id(root), .failure(retry)) }
                    fetch.fetchRecordsResultBlock?(terminal)
                } else if let save = operation as? CKModifyRecordsOperation {
                    let original = try #require(save.modifyRecordsResultBlock)
                    save.modifyRecordsResultBlock = { result in
                        observed.withLock { if case .success = result { $0 = true } else { $0 = false } }
                        original(result)
                    }
                    if source != 1 { save.perRecordSaveBlock?(NativeCloudOwnRecords.id(root), .failure(retry)) }
                    save.modifyRecordsResultBlock?(terminal)
                } else if let zones = operation as? CKModifyRecordZonesOperation {
                    let original = try #require(zones.modifyRecordZonesResultBlock)
                    zones.modifyRecordZonesResultBlock = { result in
                        observed.withLock { if case .success = result { $0 = true } else { $0 = false } }
                        original(result)
                    }
                    let zone = try #require(zones.recordZonesToSave?.first)
                    if source != 1 { zones.perRecordZoneSaveBlock?(zone.zoneID, .failure(retry)) }
                    zones.modifyRecordZonesResultBlock?(terminal)
                } else { throw CloudOwnTransportFailure.unexpectedReply }
                let aggregateSucceeded = observed.withLock { $0 }
                #expect(aggregateSucceeded == (source != 1))
                let reply = try await prepared.reply()
                guard case .publication(.retryAfter(let milliseconds)) = reply else {
                    Issue.record()
                    continue
                }
                #expect(milliseconds == 120_000)
            }
        }
    }
    @Test func retryMetadataUsesLongestPartialDelayAndRoundsFractionalMillisecondsUp() throws {
        let retry = CKError(.requestRateLimited, userInfo: [CKErrorRetryAfterKey: 120.0001])
        let partial = CKError(.partialFailure, userInfo: [CKErrorRetryAfterKey: 5.0, CKPartialErrorsByItemIDKey: [NativeCloudOwnRecords.id(root): retry]])
        #expect(NativeCloudOwnRecords.retryDuration(partial) == 120_001)
        guard case .retryAfter(let milliseconds) = try NativeCloudOwnRecords.saveResult(.failure(partial), target: root) else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(milliseconds == 120_001)
    }
    @Test(arguments: [CKError.Code.serviceUnavailable, .requestRateLimited], [0, 1])
    func nestedNSErrorDictionariesPreserveMetadataAndSelectLongestDelay(code: CKError.Code, representation: Int) throws {
        let id = NativeCloudOwnRecords.id(root)
        let retry = NSError(domain: CKErrorDomain, code: code.rawValue, userInfo: [CKErrorRetryAfterKey: NSNumber(value: 120.0001)])
        let nested = NSError(domain: CKErrorDomain, code: CKError.Code.partialFailure.rawValue, userInfo: [CKErrorRetryAfterKey: 10.0, CKPartialErrorsByItemIDKey: [id: retry]])
        let dictionary: Any
        if representation == 0 { dictionary = [id: nested] }
        else { dictionary = NSDictionary(dictionary: [id: nested]) }
        let terminal = NSError(domain: CKErrorDomain, code: CKError.Code.partialFailure.rawValue, userInfo: [CKErrorRetryAfterKey: 5.0, CKPartialErrorsByItemIDKey: dictionary])
        #expect(terminal.domain == CKErrorDomain && terminal.code == CKError.Code.partialFailure.rawValue)
        let partials = try #require(terminal.userInfo[CKPartialErrorsByItemIDKey] as? [AnyHashable: Any])
        #expect(partials.count == 1)
        let inner = try #require(partials[id] as? NSError)
        let leaves = try #require(inner.userInfo[CKPartialErrorsByItemIDKey] as? [AnyHashable: Any])
        #expect(leaves.count == 1)
        let leaf = try #require(leaves[id] as? NSError)
        #expect(leaf.domain == CKErrorDomain && leaf.code == code.rawValue)
        #expect((leaf.userInfo[CKErrorRetryAfterKey] as? NSNumber)?.doubleValue == 120.0001)
        #expect(NativeCloudOwnRecords.retryDuration(terminal) == 120_001)
        guard case .retryAfter(let milliseconds) = try NativeCloudOwnRecords.saveResult(.failure(terminal), target: root) else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(milliseconds == 120_001)
        let longer = NSError(domain: CKErrorDomain, code: CKError.Code.partialFailure.rawValue, userInfo: [CKErrorRetryAfterKey: 180.0, CKPartialErrorsByItemIDKey: dictionary])
        #expect(NativeCloudOwnRecords.retryDuration(longer) == 180_000)
        let foreign = NSError(domain: "synthetic-other", code: code.rawValue, userInfo: terminal.userInfo)
        #expect(NativeCloudOwnRecords.retryDuration(foreign) == nil)
        let foreignLeaf = NSError(domain: "synthetic-other", code: code.rawValue, userInfo: retry.userInfo)
        let rejected = NSError(domain: CKErrorDomain, code: CKError.Code.partialFailure.rawValue, userInfo: [CKPartialErrorsByItemIDKey: [id: foreignLeaf]])
        #expect(NativeCloudOwnRecords.retryDuration(rejected) == nil)
    }
    @Test(arguments: [CKError.Code.serviceUnavailable, .requestRateLimited])
    func perItemNSErrorRetryIsForwardedWithSuccessfulAggregate(code: CKError.Code) async throws {
        let id = NativeCloudOwnRecords.id(root)
        let retry = NSError(domain: CKErrorDomain, code: code.rawValue, userInfo: [CKErrorRetryAfterKey: NSNumber(value: 120.0)])
        for kind in 0..<4 {
            let queue = NativeOwnCapturingQueue()
            let command: CloudOwnTransportCommand
            switch kind {
            case 0: command = .publication(.read(root))
            case 1: command = .publication(.save(root, encrypted: Data([2]), systemFields: nil))
            case 2: command = .publication(.createZones(["meguri-root", "meguri-media", "c-synthetic"]))
            default: command = .read(root)
            }
            let prepared = try NativeCloudOwnTransport(queue: queue).prepare(command)
            #expect(prepared.enqueue())
            let operation = try #require(queue.operations.withLock { $0.first })
            let observed = Mutex<Bool?>(nil)
            if let fetch = operation as? CKFetchRecordsOperation {
                let original = try #require(fetch.fetchRecordsResultBlock)
                fetch.fetchRecordsResultBlock = { result in
                    observed.withLock { if case .success = result { $0 = true } else { $0 = false } }
                    original(result)
                }
                fetch.perRecordResultBlock?(id, .failure(retry))
                fetch.fetchRecordsResultBlock?(.success(()))
            } else if let save = operation as? CKModifyRecordsOperation {
                let original = try #require(save.modifyRecordsResultBlock)
                save.modifyRecordsResultBlock = { result in
                    observed.withLock { if case .success = result { $0 = true } else { $0 = false } }
                    original(result)
                }
                save.perRecordSaveBlock?(id, .failure(retry))
                save.modifyRecordsResultBlock?(.success(()))
            } else if let zones = operation as? CKModifyRecordZonesOperation {
                let original = try #require(zones.modifyRecordZonesResultBlock)
                zones.modifyRecordZonesResultBlock = { result in
                    observed.withLock { if case .success = result { $0 = true } else { $0 = false } }
                    original(result)
                }
                let zone = try #require(zones.recordZonesToSave?.first)
                zones.perRecordZoneSaveBlock?(zone.zoneID, .failure(retry))
                zones.modifyRecordZonesResultBlock?(.success(()))
            } else { throw CloudOwnTransportFailure.unexpectedReply }
            let aggregateSucceeded = observed.withLock { $0 }
            #expect(aggregateSucceeded == true)
            let reply = try await prepared.reply()
            if kind == 3 {
                guard case .read(.failed) = reply else {
                    Issue.record()
                    continue
                }
            } else {
                guard case .publication(.retryAfter(let milliseconds)) = reply else {
                    Issue.record()
                    continue
                }
                #expect(milliseconds == 120_000)
            }
        }
    }
    @Test func recordIDReadIncludesAllFieldsAndPreservesFoundWithoutE() async throws {
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.read(root))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        #expect(operation.desiredKeys == nil)
        #expect(operation.recordIDs == [NativeCloudOwnRecords.id(root)])
        let record = CKRecord(recordType: root.recordType, recordID: NativeCloudOwnRecords.id(root))
        operation.perRecordResultBlock?(record.recordID, .success(record))
        operation.fetchRecordsResultBlock?(.success(()))
        guard case .read(.found(let result)) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(result.encrypted == nil && !result.systemFields.isEmpty)
    }
    @Test(arguments: [CKError.Code.unknownItem, .zoneNotFound, .networkFailure, .partialFailure])
    func recordIDReadDistinguishesMissingZoneMissingAndFailed(code: CKError.Code) async throws {
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.read(root))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        operation.perRecordResultBlock?(NativeCloudOwnRecords.id(root), .failure(CKError(code)))
        operation.fetchRecordsResultBlock?(.failure(CKError(.partialFailure)))
        guard case .read(let read) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        switch code { case .unknownItem: #expect(read == .missing); case .zoneNotFound: #expect(read == .zoneMissing); default: #expect(read == .failed) }
    }
    /// P24's reset classification boundary only. This does not claim the reset
    /// workflow or its 37-code end-to-end obligation is implemented/closed.
    @Test(arguments: Array(1...37))
    func noCloudKitReadErrorBecomesFoundWithAbsentE(rawCode: Int) async throws {
        let code = try #require(CKError.Code(rawValue: rawCode))
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.read(root))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        operation.perRecordResultBlock?(NativeCloudOwnRecords.id(root), .failure(CKError(code)))
        operation.fetchRecordsResultBlock?(.failure(CKError(.partialFailure)))
        guard case .read(let read) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        if case .found = read { Issue.record("error became encrypted reset evidence") }
        #expect(queue.operations.withLock { $0.allSatisfy { $0 is CKFetchRecordsOperation } })
    }
    @Test func recordReadRejectsWrongOwnerTypeNameAndZoneBeforeReturningControlBytes() throws {
        let id = NativeCloudOwnRecords.id(active)
        let invalid = [
            CKRecord(recordType: "MGEntity", recordID: id),
            CKRecord(recordType: active.recordType, recordID: .init(recordName: "other", zoneID: id.zoneID)),
            CKRecord(recordType: active.recordType, recordID: .init(recordName: active.name, zoneID: NativeCloudOwnRecords.zone("other"))),
            CKRecord(recordType: active.recordType, recordID: .init(recordName: active.name, zoneID: .init(zoneName: active.zone, ownerName: "foreign-owner"))),
        ]
        for record in invalid {
            #expect(throws: (any Error).self) { try NativeCloudOwnRecords.control(record, target: active) }
        }
    }
    @Test func conditionalSaveUsesEncryptedValuesAndOriginalSystemFieldIdentity() throws {
        let record = CKRecord(recordType: active.recordType, recordID: NativeCloudOwnRecords.id(active))
        let fields = try NativeCloudKitSystemFields.encode(record)
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.publication(.save(active, encrypted: Data([1, 3, 5]), systemFields: fields)))
        #expect(queue.operations.withLock { $0.isEmpty })
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKModifyRecordsOperation })
        #expect(operation.savePolicy == .ifServerRecordUnchanged && operation.isAtomic)
        #expect(operation.recordIDsToDelete?.isEmpty ?? true)
        let saved = try #require(operation.recordsToSave?.first)
        #expect(saved.recordID == record.recordID && saved.recordType == record.recordType)
        #expect(saved["e"] == nil)
        #expect(saved.encryptedValues["e"] as? Data == Data([1, 3, 5]))
        #expect(saved.recordChangeTag == record.recordChangeTag)
    }
    @Test func conditionalSaveRejectsForeignAndMismatchedArchivesBeforeAnyEnqueue() throws {
        let queue = NativeOwnCapturingQueue()
        let transport = NativeCloudOwnTransport(queue: queue)
        let id = NativeCloudOwnRecords.id(active)
        for record in [
            CKRecord(recordType: "MGEpoch", recordID: id),
            CKRecord(recordType: active.recordType, recordID: .init(recordName: "other", zoneID: id.zoneID)),
            CKRecord(recordType: active.recordType, recordID: .init(recordName: active.name, zoneID: NativeCloudOwnRecords.zone("other"))),
            CKRecord(recordType: active.recordType, recordID: .init(recordName: active.name, zoneID: .init(zoneName: active.zone, ownerName: "foreign-owner"))),
        ] {
            let archive = try NativeCloudKitSystemFields.encode(record)
            #expect(throws: (any Error).self) { try transport.prepare(.publication(.save(active, encrypted: Data([7]), systemFields: archive))) }
        }
        #expect(queue.operations.withLock { $0.isEmpty })
    }
    @Test func savedAndConflictingControlRecordsReturnTheirActualEncryptedBytes() throws {
        let record = CKRecord(recordType: root.recordType, recordID: NativeCloudOwnRecords.id(root))
        record.encryptedValues["e"] = Data([8, 9]) as CKRecordValue
        guard case .saved(let saved) = try NativeCloudOwnRecords.saveResult(.success(record), target: root) else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(saved.encrypted == Data([8, 9]))
        let error = CKError(.serverRecordChanged, userInfo: [CKRecordChangedErrorServerRecordKey: record])
        guard case .conflict(let conflict) = try NativeCloudOwnRecords.saveResult(.failure(error), target: root) else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(conflict == saved)
        guard case .failed = try NativeCloudOwnRecords.saveResult(.failure(CKError(.networkFailure)), target: root) else { throw CloudOwnTransportFailure.unexpectedReply }
    }
    @Test func actualZoneOperationCreatesAllThreeZonesWithNoDeletionsAndRequiresAllResults() async throws {
        let queue = NativeOwnCapturingQueue()
        let zones = ["meguri-root", "meguri-media", "c-synthetic"]
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.publication(.createZones(zones)))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKModifyRecordZonesOperation })
        #expect(Set(operation.recordZonesToSave?.map { $0.zoneID.zoneName } ?? []) == Set(zones))
        #expect(operation.recordZoneIDsToDelete?.isEmpty ?? true)
        for zone in operation.recordZonesToSave ?? [] { operation.perRecordZoneSaveBlock?(zone.zoneID, .success(zone)) }
        operation.modifyRecordZonesResultBlock?(.success(()))
        guard case .publication(.zonesCreated) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        let incomplete = try NativeCloudOwnTransport(queue: queue).prepare(.publication(.createZones(zones)))
        #expect(incomplete.enqueue())
        let other = try #require(queue.operations.withLock { $0.last as? CKModifyRecordZonesOperation })
        other.modifyRecordZonesResultBlock?(.success(()))
        guard case .publication(.failed) = try await incomplete.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
    }
    @Test func stagingAssetFailureAndMissingZoneCompletionNeverProduceAnEmptyFinalPage() async throws {
        for withAsset in [false, true] {
            let queue = NativeOwnCapturingQueue()
            let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
            #expect(prepared.enqueue())
            let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
            #expect(!operation.fetchAllChanges)
            let zone = NativeCloudOwnRecords.zone("c-synthetic")
            #expect(operation.configurationsByRecordZoneID?[zone]?.previousServerChangeToken == nil)
            #expect(operation.configurationsByRecordZoneID?[zone]?.desiredKeys == nil)
            let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "entity", zoneID: zone))
            record.encryptedValues["e"] = Data([1]) as CKRecordValue
            if withAsset { record["pa"] = CKAsset(fileURL: URL(fileURLWithPath: "/tmp/synthetic-unread-asset")) }
            operation.recordWasChangedBlock?(record.recordID, .success(record))
            operation.recordWithIDWasDeletedBlock?(.init(recordName: "deleted", zoneID: zone), "MGEntity")
            operation.fetchRecordZoneChangesResultBlock?(.success(()))
            do { _ = try await prepared.reply(); Issue.record("incomplete/asset page became final") } catch {}
        }
    }
    @Test func corruptContinuationIsRejectedBeforeAZoneOperationCanBeQueued() {
        let queue = NativeOwnCapturingQueue()
        #expect(throws: (any Error).self) { try NativeCloudOwnTransport(queue: queue).prepare(.adoptionPage(zone: "c-synthetic", cursor: Data([0, 1, 2]))) }
        #expect(queue.operations.withLock { $0.isEmpty })
    }
    @Test func cancellationBeforeQueueRejectsAndCancellationAfterQueueDrainsOriginalSDKReply() async throws {
        let queue = NativeOwnCapturingQueue()
        let transport = NativeCloudOwnTransport(queue: queue)
        let rejected = try transport.prepare(.read(root))
        rejected.cancel()
        #expect(!rejected.enqueue() && queue.operations.withLock { $0.isEmpty })
        let accepted = try transport.prepare(.read(root))
        #expect(accepted.enqueue())
        accepted.cancel()
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        #expect(operation.isCancelled)
        operation.fetchRecordsResultBlock?(.failure(CKError(.operationCancelled)))
        guard case .read(.failed) = try await accepted.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(queue.operations.withLock { $0.count } == 1)
    }
}
#endif
