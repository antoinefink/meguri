#if canImport(CloudKit)
import CloudKit
import Foundation

/// Rust selects inline versus pa. Native code validates routing and file shape;
/// it never decodes or rewrites the canonical envelope or payload.
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
            if record.recordType == "MGEntity" {
                guard zone.hasPrefix("c-"), record["a"] == nil, record["ma"] == nil,
                      !record.allKeys().contains(where: { $0 != "pa" && record[$0] is CKAsset }) else { throw NativeCloudKitFailure.unsupportedAsset }
                if let asset = record["pa"], !(asset is CKAsset) { throw NativeCloudKitFailure.unsupportedAsset }
            } else {
                guard (record.recordType == "MGActive" && zone == "meguri-root" && name == "active") ||
                      (record.recordType == "MGEpoch" && zone.hasPrefix("c-") && name == "epoch") else { throw NativeCloudKitFailure.unsupportedRecord }
                try NativeCloudOwnRecords.requireNoAssets(record)
            }
        }
        let envelope: Data
        if record.encryptedValues["e"] == nil && ["MGActive", "MGEpoch"].contains(record.recordType) {
            // Preserve a typed unreadable control and its conditional version.
            // Empty bytes cannot decode as a supported Active or Epoch in Rust.
            envelope = Data()
        } else {
            guard let bytes = record.encryptedValues["e"] as? Data, bytes.count <= 1_000_000 else { throw NativeCloudKitFailure.unsupportedRecord }
            envelope = bytes
        }
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
            guard zone.hasPrefix("c-"), record.recordType == "MGEntity" else { throw NativeCloudKitFailure.unsupportedAsset }
            if let asset = record.assetURL { try validatePayloadFile(asset) }
        }
        let target = CloudControlTarget(zone: zone, name: record.name, recordType: record.recordType)
        let native: CKRecord
        if let fields = record.systemFields { native = try NativeCloudKitSystemFields.restore(fields, target: target) }
        else { native = CKRecord(recordType: record.recordType, recordID: NativeCloudOwnRecords.id(target)) }
        native.encryptedValues["e"] = record.envelope as CKRecordValue
        if record.recordType == "MGEntity" {
            native["a"] = nil; native["ma"] = nil
            native["pa"] = record.assetURL.map { CKAsset(fileURL: $0) }
        } else if let asset = record.assetURL { native["a"] = CKAsset(fileURL: asset) }
        return native
    }
    static func validatePayloadFile(_ file: URL) throws {
        guard file.isFileURL else { throw NativeCloudKitFailure.unsupportedAsset }
        let values = try file.resourceValues(forKeys: [.isRegularFileKey, .isSymbolicLinkKey, .fileSizeKey])
        guard values.isRegularFile == true, values.isSymbolicLink != true, let size = values.fileSize,
              size > 0, UInt64(size) <= CloudMediaStaging.assetLimit else { throw NativeCloudKitFailure.unsupportedAsset }
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
            guard zone.hasPrefix("c-"), batch.deletes.isEmpty,
                  batch.saves.allSatisfy({ $0.recordType == "MGEntity" && ($0.zone == nil || $0.zone == zone) }) else { throw NativeCloudKitFailure.unsupportedRecord }
            for record in batch.saves {
                if let file = record.assetURL { try NativeCloudRecordConversion.validatePayloadFile(file) }
            }
        }
    }
}
#endif
