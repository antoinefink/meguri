#if canImport(CloudKit)
import CloudKit
import Foundation
import MeguriCoreBindings
import Testing
@testable import CoreBridge

struct NativeCloudConflictReceiptTests {
    private let zone = NativeCloudOwnRecords.zone("c-conflict-receipt")

    private func record(name: String = "entity", zone: CKRecordZone.ID? = nil, type: String = "MGEntity") -> CKRecord {
        let record = CKRecord(recordType: type, recordID: .init(recordName: name, zoneID: zone ?? self.zone))
        record.encryptedValues["e"] = Data([1, 2, 3]) as CKRecordValue
        return record
    }

    @Test func actualErrorServerRecordPreservesIdentityEncryptedBytesAndArchivedSystemFields() throws {
        let original = record()
        let server = record()
        server.encryptedValues["e"] = Data([4, 5, 6]) as CKRecordValue
        let error = CKError(.serverRecordChanged, userInfo: [
            CKRecordChangedErrorServerRecordKey: server, CKErrorRetryAfterKey: 1.25
        ])
        #expect(error.serverRecord === server)
        #expect(NativeCloudKitSendError.code(error) == .serverRecordChanged)
        let receipt = try NativeCloudKitSendError.failure(record: original, error: error, collectionZoneID: zone)
        #expect(receipt.name == original.recordID.recordName)
        #expect(receipt.zone == zone.zoneName)
        #expect(receipt.code == .serverRecordChanged)
        #expect(receipt.nativeCode == Int64(CKError.Code.serverRecordChanged.rawValue))
        #expect(receipt.retryAfterMilliseconds == 1_250)
        let metadata = try #require(receipt.serverRecord)
        #expect(metadata.name == server.recordID.recordName && metadata.recordType == "MGEntity")
        #expect(metadata.zone == zone.zoneName && metadata.envelope == Data([4, 5, 6]))
        #expect(metadata.assetURL == nil)
        let fields = try #require(metadata.systemFields)
        let restored = try NativeCloudKitSystemFields.restore(fields, target: .init(zone: zone.zoneName, name: metadata.name, recordType: "MGEntity"))
        #expect(restored.recordID == server.recordID && restored.recordType == server.recordType)
        // This is the same raw projection used by CloudKitTrackingStore.sent.
        let raw = receipt.raw
        #expect(raw.code == .serverRecordChanged && raw.name == receipt.name)
        let rawServer = try #require(raw.serverRecord)
        #expect(rawServer.envelope == metadata.envelope && rawServer.systemFields == fields)
        #expect(rawServer.name == metadata.name && rawServer.zone == metadata.zone && rawServer.recordType == metadata.recordType)
        #expect(rawServer.assetPath == nil)
    }

    @Test func errorAssetURLIsNeverReadCopiedOrForwarded() throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = directory.appendingPathComponent("sdk-error-pa")
        try Data([7, 8, 9]).write(to: source)
        let server = record()
        server["pa"] = CKAsset(fileURL: source)
        try FileManager.default.removeItem(at: source)
        let error = CKError(.serverRecordChanged, userInfo: [CKRecordChangedErrorServerRecordKey: server])
        let receipt = try NativeCloudKitSendError.failure(record: record(), error: error, collectionZoneID: zone)
        #expect(receipt.serverRecord?.assetURL == nil)
        #expect(receipt.raw.serverRecord?.assetPath == nil)
        #expect(try FileManager.default.contentsOfDirectory(atPath: directory.path).isEmpty)
    }

    @Test func wrongOriginalOrServerIdentityAndTypeRejectBeforeProjection() {
        let invalid = [
            record(name: "other-entity"),
            record(zone: NativeCloudOwnRecords.zone("c-other")),
            record(zone: .init(zoneName: zone.zoneName, ownerName: "foreign-owner")),
            record(type: "MGBlob"), record(type: "MGEpoch")
        ]
        for server in invalid {
            let error = CKError(.serverRecordChanged, userInfo: [CKRecordChangedErrorServerRecordKey: server])
            #expect(throws: NativeCloudKitFailure.self) {
                try NativeCloudKitSendError.failure(record: record(), error: error, collectionZoneID: zone)
            }
        }
        for original in invalid {
            let server = record(name: original.recordID.recordName, zone: original.recordID.zoneID, type: original.recordType)
            let error = CKError(.serverRecordChanged, userInfo: [CKRecordChangedErrorServerRecordKey: server])
            // A different name is legal only when it is the original failed ID.
            if original.recordID.zoneID == zone && original.recordType == "MGEntity" { continue }
            #expect(throws: NativeCloudKitFailure.self) {
                try NativeCloudKitSendError.failure(record: original, error: error, collectionZoneID: zone)
            }
        }
        let foreign = CKRecordZone.ID(zoneName: zone.zoneName, ownerName: "foreign-owner")
        let foreignRecord = record(zone: foreign)
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitSendError.failure(record: foreignRecord,
                error: CKError(.serverRecordChanged, userInfo: [CKRecordChangedErrorServerRecordKey: foreignRecord]), collectionZoneID: foreign)
        }
    }

    @Test func absentAndMalformedActualMetadataFailInsteadOfOtherOrSaved() {
        for info: [String: Any] in [[:], [CKRecordChangedErrorServerRecordKey: "not a record"]] {
            #expect(throws: NativeCloudKitFailure.self) {
                try NativeCloudKitSendError.failure(record: record(), error: CKError(.serverRecordChanged, userInfo: info), collectionZoneID: zone)
            }
        }
        for problem in ["missing", "plaintext", "wrong-type", "empty", "oversized", "asset-type", "manifest-asset"] {
            let server = record()
            switch problem {
            case "missing": server.encryptedValues["e"] = nil
            case "plaintext": server.encryptedValues["e"] = nil; server["e"] = Data([1]) as CKRecordValue
            case "wrong-type": server.encryptedValues["e"] = "wrong type" as CKRecordValue
            case "empty": server.encryptedValues["e"] = Data() as CKRecordValue
            case "oversized": server.encryptedValues["e"] = Data(repeating: 1, count: 1_000_001) as CKRecordValue
            case "asset-type": server["pa"] = "wrong type" as CKRecordValue
            default: server["ma"] = Data([1]) as CKRecordValue
            }
            #expect(throws: NativeCloudKitFailure.self) {
                try NativeCloudKitSendError.failure(record: record(),
                    error: CKError(.serverRecordChanged, userInfo: [CKRecordChangedErrorServerRecordKey: server]), collectionZoneID: zone)
            }
        }
    }

    @Test func unrelatedErrorsDoNotProjectServerRecordsAndExistingInitializerStillDefaultsToNone() throws {
        let error = CKError(.networkFailure, userInfo: [CKRecordChangedErrorServerRecordKey: record()])
        let receipt = try NativeCloudKitSendError.failure(record: record(), error: error, collectionZoneID: zone)
        #expect(receipt.code == .networkFailure && receipt.serverRecord == nil)
        #expect(receipt.raw.serverRecord == nil)
        #expect(CoreBridge.CloudRecordFailure(name: "legacy", code: .other).serverRecord == nil)
    }

    @Test func immutableBlobServerMetadataStillUsesExistingConverter() throws {
        let blob = CKRecord(recordType: "MGBlob", recordID: .init(recordName: "baaaaaaaaaaaaaaaaaaaaaaaaaa", zoneID: NativeCloudOwnRecords.zone("meguri-media")))
        blob.encryptedValues["e"] = Data([9, 10]) as CKRecordValue
        let error = CKError(.serverRecordChanged, userInfo: [CKRecordChangedErrorServerRecordKey: blob])
        let actual = try #require(error.serverRecord)
        // The unchanged sent callback blob branch decodes the actual record
        // as Saved; collection failure projection must not take its ownership.
        let saved = try NativeCloudKitSendError.savedBlobConflict(record: blob, error: error)
        let metadata = try #require(saved)
        #expect(metadata.recordType == "MGBlob" && metadata.zone == "meguri-media")
        #expect(metadata.envelope == Data([9, 10]) && metadata.assetURL == nil)
        #expect(actual === blob)
    }

    @Test func missingOrMismatchedBlobProofRoutesAsOtherWithoutServerMetadata() throws {
        let mediaZone = NativeCloudOwnRecords.zone("meguri-media")
        let blob = record(name: "baaaaaaaaaaaaaaaaaaaaaaaaaa", zone: mediaZone, type: "MGBlob")
        let mismatches = [
            record(name: "bbbbbbbbbbbbbbbbbbbbbbbbbbb", zone: mediaZone, type: "MGBlob"),
            record(name: blob.recordID.recordName, zone: zone, type: "MGBlob"),
            record(name: blob.recordID.recordName,
                zone: .init(zoneName: mediaZone.zoneName, ownerName: "foreign-owner"), type: "MGBlob")
        ]
        for server in [nil] + mismatches.map(Optional.some) {
            let info: [String: Any] = server.map { [CKRecordChangedErrorServerRecordKey: $0] } ?? [:]
            let error = CKError(.serverRecordChanged, userInfo: info)
            // These are the production callback's original batch routing and
            // positive-proof/failure converters, in callback order.
            #expect(try NativeCloudKitRecordRouting.sentEngineBatchID(saved: [], failed: [blob.recordID],
                deleted: [], collectionZoneID: zone, batches: [blob.recordID.recordName: 42], pendingBatch: nil) == 42)
            #expect(try NativeCloudKitSendError.savedBlobConflict(record: blob, error: error) == nil)
            let failure = try NativeCloudKitSendError.failure(record: blob, error: error, collectionZoneID: zone)
            #expect(failure.code == .other && failure.zone == "meguri-media")
            #expect(failure.nativeCode == Int64(CKError.Code.serverRecordChanged.rawValue))
            #expect(failure.serverRecord == nil && failure.raw.serverRecord == nil)
        }
    }
}
#endif
