#if canImport(CloudKit)
import CloudKit
import Foundation
import Instrumentation
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
    /// K3/K7/K18.4, V14: ordinary pa is accepted per record before zone completion.
    @Test func adoptionRecordAcceptsEntityPayloadAssetBeforeZoneCompletion() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("ordinary-payload")
        try Data(repeating: 7, count: 1_048_576).write(to: source)
        let copies = directory.appendingPathComponent("owned")
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max })
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        #expect(!operation.fetchAllChanges)
        #expect(operation.configurationsByRecordZoneID?[zone]?.desiredKeys == nil)
        let target = CloudControlTarget(zone: "c-synthetic", name: "entity", recordType: "MGEntity")
        let record = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        let encrypted = Data([1, 3, 5])
        record.encryptedValues["e"] = encrypted as CKRecordValue
        record["pa"] = CKAsset(fileURL: source)
        operation.recordWasChangedBlock?(record.recordID, .success(record))
        try FileManager.default.removeItem(at: source)
        let owned = try #require(FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil).first).appendingPathComponent("asset")
        #expect(try Data(contentsOf: owned) == Data(repeating: 7, count: 1_048_576))
        // Omit zone completion: an accepted record must reach the missingReply check.
        operation.fetchRecordZoneChangesResultBlock?(.success(()))
        await #expect {
            try await prepared.reply()
        } throws: { error in
            guard let failure = error as? CloudOwnTransportFailure else { return false }
            if case .missingReply = failure { return true }
            return false
        }
        #expect(!FileManager.default.fileExists(atPath: owned.path))
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
    }

    @Test func stagingProjectionOwnsPayloadAcrossAwaitedMaintenanceHandoff() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        let bytes = Data(repeating: 0x37, count: 1_048_576)
        try bytes.write(to: source)
        let staging = try CloudMediaStaging(directory: directory.appendingPathComponent("owned"), available: { UInt64.max })
        let target = CloudControlTarget(zone: "c-synthetic", name: "ordinary-note", recordType: "MGEntity")
        let native = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        native.encryptedValues["e"] = Data([1, 3, 5]) as CKRecordValue
        native["pa"] = CKAsset(fileURL: source)
        let copied = try await handoffProjection(native, target: target, staging: staging, source: source, expected: bytes)
        #expect(!FileManager.default.fileExists(atPath: copied.path))
        #expect(throws: NativeCloudKitFailure.self) { try NativeCloudOwnRecords.control(native, target: target) }
    }

    private func handoffProjection(_ native: CKRecord, target: CloudControlTarget, staging: CloudMediaStaging,
                                   source: URL, expected: Data) async throws -> URL {
        let (result, copied) = try projectionReply(native, target: target, staging: staging, source: source)
        // The factory's projection has returned: the reply is now the only
        // native owner, and raw conversion retains only strings and metadata.
        try await result.withRaw { reply in
            await Task.yield()
            try await MaintenanceLane.shared.call(.cloudConnectionAdoptionResult) {
                guard case .page(let records, _, _) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                let path = try #require(records.first?.payloadFile)
                #expect(path == copied.path)
                #expect(try Data(contentsOf: URL(fileURLWithPath: path)) == expected)
            }
        }
        return copied
    }

    private func projectionReply(_ native: CKRecord, target: CloudControlTarget, staging: CloudMediaStaging,
                                 source: URL) throws -> (CloudAdoptionResult, URL) {
        let projection = try NativeCloudStagingRecords.project(native, target: target, staging: staging, cancelled: { false })
        let copied = try #require(projection.payloadFile)
        #expect(copied != source)
        try FileManager.default.removeItem(at: source)
        let restored = try NativeCloudKitSystemFields.restore(projection.record.systemFields, target: target)
        #expect(restored.recordID == native.recordID && restored.recordType == native.recordType)
        #expect(projection.record.encrypted == (native.encryptedValues["e"] as? Data))
        return (.page(records: [projection], deleted: [], next: nil), copied)
    }

    @Test(arguments: [false, true])
    func adoptionRequestedPageRetainsMaximumAssetsUntilOriginalFailure(cancel: Bool) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let copies = directory.appendingPathComponent("owned")
        let staging = try CloudMediaStaging(directory: copies, available: { CloudMediaStaging.reserve + CloudMediaStaging.stagedLimit })
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        let configuration = try #require(operation.configurationsByRecordZoneID?[zone])
        #expect(CloudMediaStaging.assetLimit == 100 * 1_024 * 1_024)
        #expect(CloudMediaStaging.stagedLimit == 256 * 1_024 * 1_024)
        #expect(CloudMediaStaging.reserve == 512 * 1_024 * 1_024)
        #expect(configuration.resultsLimit == NativeCloudStagingRecords.pageLimit)
        #expect(configuration.resultsLimit == 2)
        #expect(UInt64(configuration.resultsLimit) * CloudMediaStaging.assetLimit <= CloudMediaStaging.stagedLimit)
        #expect(UInt64(configuration.resultsLimit + 1) * CloudMediaStaging.assetLimit > CloudMediaStaging.stagedLimit)
        #expect(configuration.previousServerChangeToken == nil && configuration.desiredKeys == nil)
        #expect(!operation.fetchAllChanges)
        for index in 0..<configuration.resultsLimit {
            let source = directory.appendingPathComponent("sdk-pa-\(index)")
            #expect(FileManager.default.createFile(atPath: source.path, contents: nil))
            let file = try FileHandle(forWritingTo: source)
            let marker = Data([UInt8(index + 1)])
            try file.write(contentsOf: marker)
            try file.truncate(atOffset: CloudMediaStaging.assetLimit)
            try file.seek(toOffset: CloudMediaStaging.assetLimit - 1)
            try file.write(contentsOf: marker)
            try file.close()
            try NativeCloudRecordConversion.validatePayloadFile(source)
            let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-\(index)", zoneID: zone))
            record.encryptedValues["e"] = marker as CKRecordValue
            record["pa"] = CKAsset(fileURL: source)
            operation.recordWasChangedBlock?(record.recordID, .success(record))
            try FileManager.default.removeItem(at: source)
        }
        let owned = try FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil).map { $0.appendingPathComponent("asset") }
        try #require(owned.count == configuration.resultsLimit)
        var retainedBytes: UInt64 = 0
        var markers = Set<Data>()
        for copy in owned {
            let size = try #require(copy.resourceValues(forKeys: [.fileSizeKey]).fileSize)
            #expect(UInt64(size) == CloudMediaStaging.assetLimit)
            retainedBytes += UInt64(size)
            let file = try FileHandle(forReadingFrom: copy)
            let firstByte = try file.read(upToCount: 1)
            let marker = try #require(firstByte)
            markers.insert(marker)
            try file.seek(toOffset: CloudMediaStaging.assetLimit - 1)
            #expect(try file.read(upToCount: 1) == marker)
            try file.close()
        }
        #expect(markers == Set([Data([1]), Data([2])]))
        #expect(retainedBytes == 200 * 1_024 * 1_024 && retainedBytes <= CloudMediaStaging.stagedLimit)
        if cancel {
            prepared.cancel()
            #expect(operation.isCancelled)
        }
        #expect(owned.allSatisfy { FileManager.default.fileExists(atPath: $0.path) })
        operation.fetchRecordZoneChangesResultBlock?(.failure(CKError(cancel ? .operationCancelled : .networkFailure)))
        do { _ = try await prepared.reply(); Issue.record("failed maximum-asset page became success") } catch {}
        #expect(owned.allSatisfy { !FileManager.default.fileExists(atPath: $0.path) })
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
    }

    @Test(arguments: [false, true])
    func adoptionCallbackBeyondRequestedBoundFailsBeforeCopyingAnotherFile(includeDeletion: Bool) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        try Data([7, 5, 3]).write(to: source)
        let copies = directory.appendingPathComponent("owned")
        let opened = Mutex(0)
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max }, beforeSourceRead: { _, boundary in
            if case .open = boundary { opened.withLock { $0 += 1 } }
        })
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        let limit = try #require(operation.configurationsByRecordZoneID?[zone]?.resultsLimit)
        for index in 0..<limit {
            let id = CKRecord.ID(recordName: "ordinary-\(index)", zoneID: zone)
            if includeDeletion && index == 0 {
                operation.recordWithIDWasDeletedBlock?(id, "MGEntity")
            } else {
                let record = CKRecord(recordType: "MGEntity", recordID: id)
                record.encryptedValues["e"] = Data([1]) as CKRecordValue
                record["pa"] = CKAsset(fileURL: source)
                operation.recordWasChangedBlock?(id, .success(record))
            }
        }
        let expectedCopies = limit - (includeDeletion ? 1 : 0)
        #expect(opened.withLock { $0 } == expectedCopies)
        for index in 0..<2 {
            let extra = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "excess-\(index)", zoneID: zone))
            extra.encryptedValues["e"] = Data([1]) as CKRecordValue
            extra["pa"] = CKAsset(fileURL: source)
            operation.recordWasChangedBlock?(extra.recordID, .success(extra))
        }
        #expect(opened.withLock { $0 } == expectedCopies)
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).count == expectedCopies)
        // No token is fabricated. The bound failure must outrank missing zone
        // completion even when the original operation reports success.
        operation.fetchRecordZoneChangesResultBlock?(.success(()))
        await #expect(throws: NativeCloudKitFailure.self) { try await prepared.reply() }
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
        #expect(try Data(contentsOf: source) == Data([7, 5, 3]))
    }

    @Test(arguments: [false, true])
    func adoptionTerminalFailureAndCancellationReleaseCallbackCopies(cancel: Bool) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        let bytes = Data(repeating: 9, count: 131_073)
        try bytes.write(to: source)
        let copies = directory.appendingPathComponent("owned")
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max })
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-note", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        record.encryptedValues["e"] = Data([1]) as CKRecordValue
        record["pa"] = CKAsset(fileURL: source)
        operation.recordWasChangedBlock?(record.recordID, .success(record))
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).count == 1)
        if cancel {
            prepared.cancel()
            #expect(operation.isCancelled)
            // Cancellation alone cannot drop bytes still used by SDK callbacks.
            #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).count == 1)
        }
        operation.fetchRecordZoneChangesResultBlock?(.failure(CKError(cancel ? .operationCancelled : .networkFailure)))
        do { _ = try await prepared.reply(); Issue.record("terminal failure became a page") } catch {}
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
        #expect(try Data(contentsOf: source) == bytes)
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
