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

    @Test(arguments: [CKError.Code.serviceUnavailable, .requestRateLimited])
    func publicationReadsSavesAndZonesForwardActualServerDuration(code: CKError.Code) async throws {
        let retry = CKError(code, userInfo: [CKErrorRetryAfterKey: 120.0])
        let partial = CKError(.partialFailure, userInfo: [CKPartialErrorsByItemIDKey: [NativeCloudOwnRecords.id(root): retry]])
        // Exercise per-item metadata, operation metadata, and nested partial
        // operation metadata through the original CK completion handlers.
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
                let itemError = source == 0 ? retry : CKError(.networkFailure)
                let terminalError = source == 1 ? retry : (source == 2 ? partial : CKError(.partialFailure))
                if let fetch = operation as? CKFetchRecordsOperation {
                    fetch.perRecordResultBlock?(NativeCloudOwnRecords.id(root), .failure(itemError))
                    fetch.fetchRecordsResultBlock?(.failure(terminalError))
                } else if let save = operation as? CKModifyRecordsOperation {
                    save.perRecordSaveBlock?(NativeCloudOwnRecords.id(root), .failure(itemError))
                    save.modifyRecordsResultBlock?(.failure(terminalError))
                } else if let zones = operation as? CKModifyRecordZonesOperation {
                    let zone = try #require(zones.recordZonesToSave?.first)
                    zones.perRecordZoneSaveBlock?(zone.zoneID, .failure(itemError))
                    zones.modifyRecordZonesResultBlock?(.failure(terminalError))
                } else { throw CloudOwnTransportFailure.unexpectedReply }
                guard case .publication(.retryAfter(let milliseconds)) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
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
