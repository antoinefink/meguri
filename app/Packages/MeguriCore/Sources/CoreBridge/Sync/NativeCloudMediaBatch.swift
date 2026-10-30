#if canImport(CloudKit)
import CloudKit
import Foundation

/// Asset support is confined to immutable MGBlob saves in meguri-media.
/// Collection pa/ma and control-record assets remain explicitly unsupported.
enum NativeCloudRecordConversion {
    static func decode(_ record: CKRecord) throws -> CloudRecord {
        try NativeCloudKitIdentityValidation.requireCurrentOwner(record.recordID.zoneID)
        let zone = record.recordID.zoneID.zoneName
        let name = record.recordID.recordName
        if zone == "meguri-media" {
            guard record.recordType == "MGBlob", NativeCloudMediaRecords.isBlobName(name),
                  record["pa"] == nil, record["ma"] == nil,
                  !record.allKeys().contains(where: { $0 != "a" && record[$0] is CKAsset }) else { throw NativeCloudKitFailure.unsupportedRecord }
            if let asset = record["a"], !(asset is CKAsset) { throw NativeCloudKitFailure.unsupportedAsset }
        } else {
            guard ["MGEntity", "MGActive", "MGEpoch"].contains(record.recordType) else { throw NativeCloudKitFailure.unsupportedRecord }
            try NativeCloudOwnRecords.requireNoAssets(record)
        }
        guard let envelope = record.encryptedValues["e"] as? Data, envelope.count <= 1_000_000 else { throw NativeCloudKitFailure.unsupportedRecord }
        // Sent callbacks need metadata only. The original upload file remains
        // in Rust's batch registry; never adopt an SDK-returned asset URL here.
        return .init(name: name, envelope: envelope, systemFields: try NativeCloudKitSystemFields.encode(record), recordType: record.recordType, zone: zone)
    }
    static func encode(_ record: CloudRecord, zone: String) throws -> CKRecord {
        guard record.zone == nil || record.zone == zone, record.envelope.count <= 1_000_000 else { throw NativeCloudKitFailure.unsupportedRecord }
        if zone == "meguri-media" {
            guard record.recordType == "MGBlob", NativeCloudMediaRecords.isBlobName(record.name), let asset = record.assetURL else { throw NativeCloudKitFailure.unsupportedRecord }
            let values = try asset.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
            guard values.isRegularFile == true, values.isSymbolicLink != true, let size = values.fileSize,
                  size > 0, UInt64(size) <= CloudMediaStaging.assetLimit else { throw NativeCloudKitFailure.unsupportedAsset }
        } else {
            guard record.recordType == "MGEntity", record.assetURL == nil else { throw NativeCloudKitFailure.unsupportedAsset }
        }
        let target = CloudControlTarget(zone: zone, name: record.name, recordType: record.recordType)
        let native: CKRecord
        if let fields = record.systemFields { native = try NativeCloudKitSystemFields.restore(fields, target: target) }
        else { native = CKRecord(recordType: record.recordType, recordID: NativeCloudOwnRecords.id(target)) }
        native.encryptedValues["e"] = record.envelope as CKRecordValue
        if let asset = record.assetURL { native["a"] = CKAsset(fileURL: asset) }
        return native
    }
}

enum NativeCloudMediaBatch {
    static func zone(name: String, collection: String) -> String {
        NativeCloudMediaRecords.isBlobName(name) ? "meguri-media" : collection
    }
    static func validate(_ batch: CloudEngineBatch, zone: String) throws {
        guard Set(batch.saves.map(\.name)).count == batch.saves.count,
              Set(batch.deletes).count == batch.deletes.count,
              Set(batch.saves.map(\.name)).isDisjoint(with: batch.deletes) else { throw NativeCloudKitFailure.unsupportedRecord }
        if zone == "meguri-media" {
            guard batch.saves.count <= 8, batch.deletes.allSatisfy(NativeCloudMediaRecords.isBlobName) else { throw NativeCloudKitFailure.unsupportedRecord }
            var bytes: UInt64 = 0
            for record in batch.saves {
                guard record.zone == zone, record.recordType == "MGBlob", let file = record.assetURL else { throw NativeCloudKitFailure.unsupportedRecord }
                let size = try file.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? -1
                guard size > 0, UInt64(size) <= CloudMediaStaging.assetLimit else { throw NativeCloudKitFailure.unsupportedAsset }
                bytes += UInt64(size)
            }
            guard bytes <= 128 * 1_024 * 1_024 else { throw NativeCloudKitFailure.unsupportedAsset }
        } else {
            guard batch.deletes.isEmpty, batch.saves.allSatisfy({ $0.recordType == "MGEntity" && $0.assetURL == nil && ($0.zone == nil || $0.zone == zone) }) else { throw NativeCloudKitFailure.unsupportedRecord }
        }
    }
}
#endif
