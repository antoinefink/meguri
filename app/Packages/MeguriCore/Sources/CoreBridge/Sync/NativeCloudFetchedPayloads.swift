#if canImport(CloudKit)
import CloudKit
import Foundation

/// Native leases survive suspended delivery. Durable core intake must copy the
/// file before returning; releasing this event never removes a core-owned file.
struct NativeCloudFetchedPayloads {
    let events: [CloudEngineEvent]
    private let assets: [CloudMediaStagedAsset]

    init(records: [CKRecord], deleted: [CKRecord.ID], staging: CloudMediaStaging) throws {
        let groups = try NativeCloudKitRecordRouting.validatedGroups(records: records, deleted: deleted)
        var assets: [CloudMediaStagedAsset] = []
        var events: [CloudEngineEvent] = []
        for group in groups {
            var values: [CloudRecord] = []
            for record in group.records {
                let metadata = try NativeCloudRecordConversion.decode(record)
                var staged: CloudMediaStagedAsset?
                if record.recordType == "MGEntity", let source = (record["pa"] as? CKAsset)?.fileURL {
                    do {
                        staged = try staging.copy(source, cancelled: { false }, retainedAssetLimit: Int.max)
                    } catch is CloudMediaSourceFailure {
                        // Retain metadata so core intake can retry unavailable pa.
                    } catch CloudMediaFailure.stagingLimit {
                        // Budget exhausted: retain metadata and existing copies.
                    }
                }
                if let staged { assets.append(staged) }
                values.append(.init(name: metadata.name, envelope: metadata.envelope,
                    systemFields: metadata.systemFields, recordType: metadata.recordType,
                    zone: metadata.zone, assetURL: staged?.url))
            }
            events.append(.fetched(zone: group.zoneID.zoneName, records: values, deleted: group.deleted.map(\.recordName)))
        }
        self.events = events; self.assets = assets
    }
}
#endif
