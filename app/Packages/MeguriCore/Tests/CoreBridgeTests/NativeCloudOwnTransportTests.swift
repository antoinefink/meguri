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
/// Retain the exact closures installed by production, before SDK getter bridging.
final class NativeOwnReadHandlers: Sendable {
    typealias PerRecord = @Sendable (CKRecord.ID, Result<CKRecord, any Error>) -> Void
    typealias Terminal = @Sendable (Result<Void, any Error>) -> Void
    let perRecord = Mutex<PerRecord?>(nil)
    let terminal = Mutex<Terminal?>(nil)
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

    /// Synthetic SDK identity only; this does not prove equivalence of
    /// separately fetched real server objects.
    @Test func epochReadAndIndependentPageProjectionsPreserveExactSystemFields() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        _ = try CloudMediaStaging(directory: directory, available: { UInt64.max })
        defer { try? FileManager.default.removeItem(at: directory) }
        let target = CloudControlTarget(zone: "c-synthetic", name: "epoch", recordType: "MGEpoch")
        let encrypted = Data([1, 3, 5])
        let record = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        record.encryptedValues["e"] = encrypted as CKRecordValue
        guard case .found(let read) = try NativeCloudOwnRecords.readResult(.success(record), target: target) else {
            throw CloudOwnTransportFailure.unexpectedReply
        }
        let page = try NativeCloudStagingRecords.metadata(record, target: target)
        #expect(read.encrypted == encrypted)
        #expect(page.record.encrypted == encrypted)
        #expect(page.payloadFile == nil)
        // Each production projection independently archives system fields.
        #expect(page.record.systemFields == read.systemFields)

        let restored = try NativeCloudKitSystemFields.restore(read.systemFields, target: target)
        #expect(restored !== record)
        restored.encryptedValues["e"] = encrypted as CKRecordValue
        let restoredPage = try NativeCloudStagingRecords.metadata(restored, target: target)
        #expect(restoredPage.record.encrypted == encrypted)
        #expect(restoredPage.payloadFile == nil)
        #expect(restoredPage.record.systemFields == read.systemFields)
        #expect(try FileManager.default.contentsOfDirectory(atPath: directory.path).isEmpty)
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
        let id = NativeCloudOwnRecords.id(root)
        for publication in [false, true] {
            // Preserve both per-record branches; also exercise terminal-only
            // exact, empty, mismatched-ID and foreign-domain partial failures.
            for source in 0...8 {
                let queue = NativeOwnCapturingQueue()
                let handlers = NativeOwnReadHandlers()
                let transport = NativeCloudOwnTransport(queue: queue,
                    readPerRecordHandlerInstalled: { handler in handlers.perRecord.withLock { $0 = handler } },
                    readTerminalHandlerInstalled: { handler in handlers.terminal.withLock { $0 = handler } })
                let command: CloudOwnTransportCommand = publication ? .publication(.read(root)) : .read(root)
                let prepared = try transport.prepare(command)
                #expect(prepared.enqueue())
                let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
                #expect(operation.recordIDs == [id] && operation.desiredKeys == nil)
                let perItem = try #require(handlers.perRecord.withLock { $0 })
                let terminal = try #require(handlers.terminal.withLock { $0 })
                if source == 0 {
                    perItem(id, .failure(CKError(code)))
                    terminal(.failure(CKError(.partialFailure)))
                } else {
                    let error = NSError(domain: source == 7 ? "synthetic.foreign" : CKErrorDomain, code: rawCode)
                    let itemID: CKRecord.ID
                    switch source {
                    case 4: itemID = .init(recordName: "other", zoneID: id.zoneID)
                    case 5: itemID = .init(recordName: id.recordName, zoneID: NativeCloudOwnRecords.zone("other"))
                    case 6: itemID = .init(recordName: id.recordName, zoneID: .init(zoneName: root.zone, ownerName: "foreign-owner"))
                    default: itemID = id
                    }
                    let dictionary: [CKRecord.ID: NSError] = source == 3 ? [:] : [itemID: error]
                    let partial = NSError(domain: CKErrorDomain, code: CKError.Code.partialFailure.rawValue,
                                          userInfo: [CKPartialErrorsByItemIDKey: dictionary])
                    let items = try #require(partial.userInfo[CKPartialErrorsByItemIDKey] as? [AnyHashable: NSError])
                    #expect(partial.domain == CKErrorDomain && partial.code == CKError.Code.partialFailure.rawValue)
                    #expect(items.count == (source == 3 ? 0 : 1))
                    if source != 3 {
                        let item = try #require(items[itemID])
                        #expect(item.domain == error.domain && item.code == rawCode)
                    }
                    if source == 1 { perItem(id, .failure(error)) }
                    if source == 8 {
                        // The original per-record result outranks a conflicting
                        // terminal dictionary for the exact same requested ID.
                        perItem(id, .failure(CKError(.networkFailure)))
                    }
                    terminal(.failure(partial))
                }
                let reply = try await prepared.reply()
                let read: CloudControlRead
                if publication {
                    guard case .publication(.read(let value)) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                    read = value
                } else {
                    guard case .read(let value) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                    read = value
                }
                let expected: CloudControlRead = source < 3
                    ? (rawCode == 11 ? .missing : (rawCode == 26 ? .zoneMissing : .failed)) : .failed
                #expect(read == expected)
                #expect(queue.operations.withLock { $0.count == 1 && $0.allSatisfy { $0 is CKFetchRecordsOperation } })
            }
        }
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
    @Test func adoptionMetadataPageRequestsOnlyEnvelopeWithFixedLimit() async throws {
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(queue.operations.withLock { $0.isEmpty })
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        let configuration = try #require(operation.configurationsByRecordZoneID?[zone])
        #expect(operation.recordZoneIDs == [zone])
        #expect(configuration.previousServerChangeToken == nil)
        #expect(configuration.desiredKeys == ["e"])
        #expect(configuration.resultsLimit == 32)
        // 32 * 1 MiB metadata + 2 * 100 MiB assets < 256 MiB. This request
        // bound alone does not prove actual metadata-byte or callback bounds.
        #expect(!operation.fetchAllChanges)
        #expect(queue.operations.withLock { $0.count == 1 && $0.first === operation })
        let terminal = try #require(operation.fetchRecordZoneChangesResultBlock)
        terminal(.failure(CKError(.networkFailure)))
        await #expect(throws: (any Error).self) { try await prepared.reply() }
    }

    @Test func adoptionMetadataPageRejectsUnexpectedPayloadBeforeOpeningOrStagingIt() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        let bytes = Data([7, 5, 3])
        try bytes.write(to: source)
        let copies = directory.appendingPathComponent("owned")
        let opened = Mutex(0)
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max }, beforeSourceRead: { _, boundary in
            if case .open = boundary { opened.withLock { $0 += 1 } }
        })
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let changed = try #require(operation.recordWasChangedBlock)
        let terminal = try #require(operation.fetchRecordZoneChangesResultBlock)
        let target = CloudControlTarget(zone: "c-synthetic", name: "ordinary-note", recordType: "MGEntity")
        let record = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        record.encryptedValues["e"] = Data([1, 3, 5]) as CKRecordValue
        record["pa"] = CKAsset(fileURL: source)
        changed(record.recordID, .success(record))
        #expect(opened.withLock { $0 } == 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
        // The original terminal still owns completion. Refusal must outrank
        // missing zone completion, rather than merely cleaning a copied asset.
        terminal(.success(()))
        await #expect(throws: NativeCloudKitFailure.self) { try await prepared.reply() }
        #expect(opened.withLock { $0 } == 0)
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
        #expect(try Data(contentsOf: source) == bytes)
        #expect(queue.operations.withLock { $0.count == 1 && $0.first === operation })
    }

    // Only tag lookup is synthetic in positive tests. CKRecord identity,
    // encrypted fields and public system-field encode/restore remain real.
    // This supplies no evidence about actual server-assigned tags.
    private func payloadExpected(_ record: CKRecord) throws -> CloudControlRecord {
        .init(encrypted: record.encryptedValues["e"] as? Data, systemFields: try NativeCloudKitSystemFields.encode(record))
    }

    @Test func adoptionStrictPayloadRejectsMissingVersionBeforeOpening() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        try Data([7]).write(to: source)
        let opened = Mutex(0)
        let staging = try CloudMediaStaging(directory: directory.appendingPathComponent("owned"), available: { UInt64.max }, beforeSourceRead: { _, boundary in
            if case .open = boundary { opened.withLock { $0 += 1 } }
        })
        let queue = NativeOwnCapturingQueue()
        let native = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        native.encryptedValues["e"] = Data([1]) as CKRecordValue
        #expect(native.recordChangeTag == nil)
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging).prepare(.adoptionPayload(zone: "c-synthetic", name: "ordinary", expected: payloadExpected(native)))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        native["pa"] = CKAsset(fileURL: source)
        operation.perRecordResultBlock?(native.recordID, .success(native))
        operation.fetchRecordsResultBlock?(.success(()))
        await #expect(throws: NativeCloudKitFailure.self) { try await prepared.reply() }
        #expect(opened.withLock { $0 } == 0)
        #expect(try Data(contentsOf: source) == Data([7]))
    }

    @Test func adoptionStrictPayloadCopiesBeforeCallbackReturnAndRetainsOriginalMetadata() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-pa")
        let bytes = Data(repeating: 7, count: 1_048_576)
        try bytes.write(to: source)
        let copies = directory.appendingPathComponent("owned")
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max })
        let queue = NativeOwnCapturingQueue()
        let target = CloudControlTarget(zone: "c-synthetic", name: "ordinary-note", recordType: "MGEntity")
        let record = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        record.encryptedValues["e"] = Data([1, 3, 5]) as CKRecordValue
        let expected = try payloadExpected(record)
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging, payloadTagReader: { _ in "synthetic-version" }).prepare(.adoptionPayload(zone: target.zone, name: target.name, expected: expected))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        #expect(operation.recordIDs == [record.recordID] && operation.desiredKeys == nil)
        #expect(operation.configuration.qualityOfService == .userInitiated)
        let ordinary = try NativeCloudOwnTransport(queue: queue).prepare(.read(root))
        #expect(ordinary.enqueue())
        let ordinaryOperation = try #require(queue.operations.withLock { $0.last as? CKFetchRecordsOperation })
        #expect(ordinaryOperation.configuration.qualityOfService == CKFetchRecordsOperation().configuration.qualityOfService)
        ordinaryOperation.fetchRecordsResultBlock?(.failure(CKError(.networkFailure)))
        guard case .read(.failed) = try await ordinary.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        record["pa"] = CKAsset(fileURL: source)
        operation.perRecordResultBlock?(record.recordID, .success(record))
        let owned = try #require(FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil).first).appendingPathComponent("asset")
        try FileManager.default.removeItem(at: source)
        #expect(try Data(contentsOf: owned) == bytes)
        operation.fetchRecordsResultBlock?(.success(()))
        try await consumePayload(prepared, expected: expected, copied: owned, bytes: bytes)
        #expect(!FileManager.default.fileExists(atPath: owned.path))
    }

    private func consumePayload(_ prepared: any CloudPreparedOwnOperation, expected: CloudControlRecord,
                                copied: URL, bytes: Data) async throws {
        guard case .adoptionPayload(let record) = try await prepared.reply() else { throw CloudOwnTransportFailure.unexpectedReply }
        #expect(record.record == expected)
        try await CloudAdoptionResult.payload(record).withRaw { reply in
            await Task.yield()
            try await MaintenanceLane.shared.call(.cloudConnectionAdoptionResult) {
                guard case .payload(let value) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                #expect(value.payloadFile == copied.path)
                #expect(try Data(contentsOf: copied) == bytes)
            }
        }
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
        let copied: URL
        do {
            let (reply, path) = try payloadProjection(native, target: target, staging: staging, source: source)
            copied = path
            try await reply.withRaw { value in
                await Task.yield()
                try await MaintenanceLane.shared.call(.cloudConnectionAdoptionResult) {
                    guard case .payload(let record) = value else { throw CloudOwnTransportFailure.unexpectedReply }
                    #expect(record.payloadFile == copied.path)
                    #expect(try Data(contentsOf: copied) == bytes)
                }
            }
            withExtendedLifetime(reply) {
                #expect(FileManager.default.fileExists(atPath: copied.path))
            }
        }
        #expect(!FileManager.default.fileExists(atPath: copied.path))
    }

    private func payloadProjection(_ native: CKRecord, target: CloudControlTarget, staging: CloudMediaStaging,
                                   source: URL) throws -> (CloudAdoptionResult, URL) {
        let expected = try payloadExpected(native)
        let projection = try NativeCloudStagingRecords.payload(native, target: target, expected: expected, staging: staging, cancelled: { false }, tagReader: { _ in "synthetic-version" })
        let copied = try #require(projection.payloadFile)
        #expect(copied != source)
        try FileManager.default.removeItem(at: source)
        #expect(projection.record == expected)
        return (.payload(projection), copied)
    }

    @Test(arguments: [false, true])
    func adoptionStrictReadsRetainMaximumAssetsUntilOriginalFailure(cancel: Bool) async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let copies = directory.appendingPathComponent("owned")
        let opened = Mutex(0)
        let staging = try CloudMediaStaging(directory: copies, available: { CloudMediaStaging.reserve + CloudMediaStaging.stagedLimit },
                                           beforeSourceRead: { _, boundary in if case .open = boundary { opened.withLock { $0 += 1 } } })
        let queue = NativeOwnCapturingQueue()
        let transport = NativeCloudOwnTransport(queue: queue, staging: staging, payloadTagReader: { _ in "synthetic-version" })
        #expect(CloudMediaStaging.assetLimit == 100 * 1_024 * 1_024)
        #expect(CloudMediaStaging.stagedLimit == 256 * 1_024 * 1_024)
        #expect(CloudMediaStaging.reserve == 512 * 1_024 * 1_024)
        var prepared: [any CloudPreparedOwnOperation] = []
        var operations: [CKFetchRecordsOperation] = []
        for index in 0..<3 {
            let source = directory.appendingPathComponent("sdk-pa-\(index)")
            #expect(FileManager.default.createFile(atPath: source.path, contents: nil))
            let file = try FileHandle(forWritingTo: source)
            let marker = Data([UInt8(index + 1)])
            try file.write(contentsOf: marker)
            try file.truncate(atOffset: CloudMediaStaging.assetLimit)
            try file.seek(toOffset: CloudMediaStaging.assetLimit - 1)
            try file.write(contentsOf: marker)
            try file.close()
            let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-\(index)", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
            record.encryptedValues["e"] = marker as CKRecordValue
            let expected = try payloadExpected(record)
            let op = try transport.prepare(.adoptionPayload(zone: "c-synthetic", name: record.recordID.recordName, expected: expected))
            #expect(op.enqueue())
            let native = try #require(queue.operations.withLock { $0.last as? CKFetchRecordsOperation })
            prepared.append(op); operations.append(native)
            record["pa"] = CKAsset(fileURL: source)
            native.perRecordResultBlock?(record.recordID, .success(record))
            try FileManager.default.removeItem(at: source)
        }
        // Production Core emits one at a time. Even adversarial concurrent
        // transport users share the same two-file/256 MiB reservation.
        #expect(opened.withLock { $0 } == 2)
        let owned = try FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil).map { $0.appendingPathComponent("asset") }
        try #require(owned.count == 2)
        var markers = Set<Data>()
        for copy in owned {
            #expect(UInt64(try #require(copy.resourceValues(forKeys: [.fileSizeKey]).fileSize)) == CloudMediaStaging.assetLimit)
            let file = try FileHandle(forReadingFrom: copy)
            let marker = try #require(try file.read(upToCount: 1))
            markers.insert(marker)
            try file.seek(toOffset: CloudMediaStaging.assetLimit - 1)
            #expect(try file.read(upToCount: 1) == marker)
            try file.close()
        }
        #expect(markers == Set([Data([1]), Data([2])]))
        for (op, native) in zip(prepared, operations) {
            if cancel { op.cancel(); #expect(native.isCancelled) }
        }
        #expect(owned.allSatisfy { FileManager.default.fileExists(atPath: $0.path) })
        for (op, native) in zip(prepared, operations) {
            native.fetchRecordsResultBlock?(.failure(CKError(cancel ? .operationCancelled : .networkFailure)))
            await #expect(throws: (any Error).self) { try await op.reply() }
        }
        #expect(owned.allSatisfy { !FileManager.default.fileExists(atPath: $0.path) })
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
    }

    @Test func adoptionAndEnginePayloadConsumersShareBoundsAndReleaseReservations() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-large")
        #expect(FileManager.default.createFile(atPath: source.path, contents: Data([7])))
        let writer = try FileHandle(forWritingTo: source)
        try writer.truncate(atOffset: CloudMediaStaging.assetLimit)
        try writer.close()
        let tiny = directory.appendingPathComponent("sdk-tiny")
        try Data([9]).write(to: tiny)
        let copies = directory.appendingPathComponent("app-root/cloud-native-staging", isDirectory: true)
        let engineCopies = directory.appendingPathComponent("default/MeguriCloudPayloads", isDirectory: true)
        let opened = Mutex(0)
        let reservations = CloudMediaStaging.Reservations()
        let staging = try CloudMediaStaging(directory: copies, available: { UInt64.max }, reservations: reservations,
            beforeSourceRead: { _, boundary in
                if case .open = boundary { opened.withLock { $0 += 1 } }
            })
        let engineStaging = try CloudMediaStaging(directory: engineCopies, available: { UInt64.max }, reservations: reservations,
            beforeSourceRead: { _, boundary in
                if case .open = boundary { opened.withLock { $0 += 1 } }
            })
        #expect(staging !== engineStaging)
        let target = CloudControlTarget(zone: "c-synthetic", name: "adoption", recordType: "MGEntity")
        let record = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        record.encryptedValues["e"] = Data([1]) as CKRecordValue
        record["pa"] = CKAsset(fileURL: source)
        let expected = try payloadExpected(record)
        let queue = NativeOwnCapturingQueue()
        let transport = NativeCloudOwnTransport(queue: queue, staging: staging, payloadTagReader: { _ in "synthetic-version" })
        let prepared = try transport.prepare(.adoptionPayload(zone: target.zone, name: target.name, expected: expected))
        #expect(prepared.enqueue())
        let original = try #require(queue.operations.withLock { $0.last as? CKFetchRecordsOperation })
        original.perRecordResultBlock?(record.recordID, .success(record))
        let engineRecord = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "engine", zoneID: record.recordID.zoneID))
        engineRecord.encryptedValues["e"] = Data([2]) as CKRecordValue
        engineRecord["pa"] = CKAsset(fileURL: source)
        var delivery: NativeCloudFetchedPayloads? = try .init(records: [engineRecord], deleted: [], staging: engineStaging)
        func assetURL(_ value: NativeCloudFetchedPayloads?) throws -> URL? {
            guard case .fetched(_, let records, _) = try #require(value?.events.first) else { throw CloudOwnTransportFailure.unexpectedReply }
            return try #require(records.first).assetURL
        }
        let engineCopy = try #require(try assetURL(delivery))
        #expect(engineCopy.deletingLastPathComponent().deletingLastPathComponent() == engineCopies)
        let adoptionCopy = try #require(FileManager.default.contentsOfDirectory(at: copies, includingPropertiesForKeys: nil).first).appendingPathComponent("asset")
        #expect(FileManager.default.fileExists(atPath: adoptionCopy.path))
        #expect(opened.withLock { $0 } == 2)
        // Both directories retain payloads; byte admission covers both owners.
        let overflow = try NativeCloudFetchedPayloads(records: [engineRecord], deleted: [], staging: engineStaging)
        #expect(try assetURL(overflow) == nil)
        record["pa"] = CKAsset(fileURL: tiny)
        let countLimited = try transport.prepare(.adoptionPayload(zone: target.zone, name: target.name, expected: expected))
        #expect(countLimited.enqueue())
        let countOperation = try #require(queue.operations.withLock { $0.last as? CKFetchRecordsOperation })
        countOperation.perRecordResultBlock?(record.recordID, .success(record))
        countOperation.fetchRecordsResultBlock?(.success(()))
        await #expect(throws: CloudMediaFailure.self) { try await countLimited.reply() }
        #expect(opened.withLock { $0 } == 2)
        original.fetchRecordsResultBlock?(.failure(CKError(.networkFailure)))
        await #expect(throws: (any Error).self) { try await prepared.reply() }
        #expect(!FileManager.default.fileExists(atPath: adoptionCopy.path))
        #expect(FileManager.default.fileExists(atPath: engineCopy.path))
        var retry: NativeCloudFetchedPayloads? = try .init(records: [engineRecord], deleted: [], staging: engineStaging)
        let retryCopy = try #require(try assetURL(retry))
        #expect(opened.withLock { $0 } == 3)
        delivery = nil
        retry = nil
        #expect(!FileManager.default.fileExists(atPath: engineCopy.path))
        #expect(!FileManager.default.fileExists(atPath: retryCopy.path))
        // Releasing engine events restores adoption's count reservation too.
        let afterRelease = try transport.prepare(.adoptionPayload(zone: target.zone, name: target.name, expected: expected))
        #expect(afterRelease.enqueue())
        let last = try #require(queue.operations.withLock { $0.last as? CKFetchRecordsOperation })
        last.perRecordResultBlock?(record.recordID, .success(record))
        #expect(opened.withLock { $0 } == 4)
        last.fetchRecordsResultBlock?(.failure(CKError(.networkFailure)))
        await #expect(throws: (any Error).self) { try await afterRelease.reply() }
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
        #expect(try FileManager.default.contentsOfDirectory(atPath: engineCopies.path).isEmpty)
    }

    @Test(arguments: [false, true])
    func adoptionCallbackBeyondRequestedBoundFailsBeforeCopyingAnotherFile(includeDeletion: Bool) async throws {
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let zone = NativeCloudOwnRecords.zone("c-synthetic")
        let limit = try #require(operation.configurationsByRecordZoneID?[zone]?.resultsLimit)
        #expect(limit == 32)
        for index in 0..<limit {
            let id = CKRecord.ID(recordName: "ordinary-\(index)", zoneID: zone)
            if includeDeletion && index == 0 { operation.recordWithIDWasDeletedBlock?(id, "MGEntity") }
            else {
                let record = CKRecord(recordType: "MGEntity", recordID: id)
                record.encryptedValues["e"] = Data([1]) as CKRecordValue
                operation.recordWasChangedBlock?(id, .success(record))
            }
        }
        let extra = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "excess", zoneID: zone))
        extra.encryptedValues["e"] = Data([1]) as CKRecordValue
        operation.recordWasChangedBlock?(extra.recordID, .success(extra))
        operation.fetchRecordZoneChangesResultBlock?(.success(()))
        await #expect(throws: NativeCloudKitFailure.self) { try await prepared.reply() }
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
        let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary-note", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        record.encryptedValues["e"] = Data([1]) as CKRecordValue
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging, payloadTagReader: { _ in "synthetic-version" }).prepare(.adoptionPayload(zone: "c-synthetic", name: record.recordID.recordName, expected: payloadExpected(record)))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        record["pa"] = CKAsset(fileURL: source)
        operation.perRecordResultBlock?(record.recordID, .success(record))
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).count == 1)
        if cancel {
            prepared.cancel(); #expect(operation.isCancelled)
            #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).count == 1)
        }
        operation.fetchRecordsResultBlock?(.failure(CKError(cancel ? .operationCancelled : .networkFailure)))
        await #expect(throws: (any Error).self) { try await prepared.reply() }
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
            #expect(operation.configurationsByRecordZoneID?[zone]?.desiredKeys == ["e"])
            let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "entity", zoneID: zone))
            record.encryptedValues["e"] = Data([1]) as CKRecordValue
            if withAsset { record["pa"] = CKAsset(fileURL: URL(fileURLWithPath: "/tmp/synthetic-unread-asset")) }
            operation.recordWasChangedBlock?(record.recordID, .success(record))
            operation.recordWithIDWasDeletedBlock?(.init(recordName: "deleted", zoneID: zone), "MGEntity")
            operation.fetchRecordZoneChangesResultBlock?(.success(()))
            do { _ = try await prepared.reply(); Issue.record("incomplete/asset page became final") } catch {}
        }
    }
    @Test func adoptionMetadataActualEncodedBytesAreBoundedBeforeTerminal() async throws {
        let queue = NativeOwnCapturingQueue()
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.adoptionPage(zone: "c-synthetic", cursor: nil))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordZoneChangesOperation })
        let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "oversized-metadata", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        // The encrypted field alone reaches the cap. Its actual SDK archive,
        // type and name push the encoded metadata over the fixed total bound.
        record.encryptedValues["e"] = Data(repeating: 1, count: NativeCloudStagingRecords.metadataLimit) as CKRecordValue
        operation.recordWasChangedBlock?(record.recordID, .success(record))
        operation.fetchRecordZoneChangesResultBlock?(.success(()))
        await #expect(throws: NativeCloudKitFailure.self) { try await prepared.reply() }
    }

    @Test(arguments: [0, 1, 2, 3, 4, 5, 6, 7])
    func adoptionStrictPayloadRefusesChangedMetadataMissingAssetAndDuplicateCallbacksBeforeAnotherCopy(variant: Int) async throws {
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
        let target = CloudControlTarget(zone: "c-synthetic", name: "ordinary-note", recordType: "MGEntity")
        let expectedNative = CKRecord(recordType: target.recordType, recordID: NativeCloudOwnRecords.id(target))
        expectedNative.encryptedValues["e"] = Data([1, 3, 5]) as CKRecordValue
        let expected = try payloadExpected(expectedNative)
        let prepared = try NativeCloudOwnTransport(queue: queue, staging: staging, payloadTagReader: { record in
            variant == 7 && record.encryptedValues["e"] as? Data != nil ? "different-synthetic-version" : "synthetic-version"
        }).prepare(.adoptionPayload(zone: target.zone, name: target.name, expected: expected))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        let record = CKRecord(recordType: variant == 1 ? "MGOther" : target.recordType,
                              recordID: variant == 2 ? .init(recordName: "other", zoneID: NativeCloudOwnRecords.zone(target.zone)) : NativeCloudOwnRecords.id(target))
        record.encryptedValues["e"] = Data(variant == 0 ? [9] : [1, 3, 5]) as CKRecordValue
        if variant != 3 { record["pa"] = CKAsset(fileURL: source) }
        if variant == 4 { record["ma"] = CKAsset(fileURL: source) }
        if variant == 5 { operation.perRecordResultBlock?(record.recordID, .failure(CKError(.unknownItem))) }
        else { operation.perRecordResultBlock?(NativeCloudOwnRecords.id(target), .success(record)) }
        if variant == 6 { operation.perRecordResultBlock?(record.recordID, .success(record)) }
        #expect(opened.withLock { $0 } == (variant == 6 ? 1 : 0))
        operation.fetchRecordsResultBlock?(.success(()))
        await #expect(throws: (any Error).self) { try await prepared.reply() }
        #expect(try FileManager.default.contentsOfDirectory(atPath: copies.path).isEmpty)
        #expect(try Data(contentsOf: source) == Data([7, 5, 3]))
    }

    @Test func adoptionStrictPayloadWithoutPerRecordReplyIsMissingAndInvalidArchiveNeverEnqueues() async throws {
        let queue = NativeOwnCapturingQueue()
        #expect(throws: (any Error).self) {
            try NativeCloudOwnTransport(queue: queue).prepare(.adoptionPayload(zone: "c-synthetic", name: "ordinary", expected: .init(encrypted: Data([1]), systemFields: Data([0]))))
        }
        #expect(queue.operations.withLock { $0.isEmpty })
        let record = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "ordinary", zoneID: NativeCloudOwnRecords.zone("c-synthetic")))
        record.encryptedValues["e"] = Data([1]) as CKRecordValue
        let prepared = try NativeCloudOwnTransport(queue: queue).prepare(.adoptionPayload(zone: "c-synthetic", name: "ordinary", expected: payloadExpected(record)))
        #expect(prepared.enqueue())
        let operation = try #require(queue.operations.withLock { $0.first as? CKFetchRecordsOperation })
        operation.fetchRecordsResultBlock?(.success(()))
        await #expect(throws: CloudOwnTransportFailure.self) { try await prepared.reply() }
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
