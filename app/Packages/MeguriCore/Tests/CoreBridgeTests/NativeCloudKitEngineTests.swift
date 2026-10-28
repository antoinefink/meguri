#if canImport(CloudKit)
import CloudKit
import Testing
@testable import CoreBridge

struct NativeCloudKitEngineTests {
    @Test func fetchedRecordsAndDeletionsRouteToTheirActualZones() throws {
        let root = CKRecordZone.ID(zoneName: "meguri-root", ownerName: CKCurrentUserDefaultName)
        let collection = CKRecordZone.ID(zoneName: "meguri-collection", ownerName: CKCurrentUserDefaultName)
        let rootRecord = CKRecord(recordType: "MGActive", recordID: .init(recordName: "root", zoneID: root))
        let first = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "first", zoneID: collection))
        let second = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "second", zoneID: collection))
        let rootDelete = CKRecord.ID(recordName: "root-delete", zoneID: root)
        let collectionDelete = CKRecord.ID(recordName: "collection-delete", zoneID: collection)

        let groups = NativeCloudKitRecordRouting.groups(records: [rootRecord, first, second], deleted: [rootDelete, collectionDelete])
        #expect(groups.map(\.zoneID) == [collection, root])
        let collectionGroup = try #require(groups.first)
        #expect(collectionGroup.records.map(\.recordID) == [first.recordID, second.recordID])
        #expect(collectionGroup.deleted == [collectionDelete])
        let rootGroup = try #require(groups.last)
        #expect(rootGroup.records.map(\.recordID) == [rootRecord.recordID])
        #expect(rootGroup.deleted == [rootDelete])
        let reversed = NativeCloudKitRecordRouting.groups(records: [second, first, rootRecord], deleted: [collectionDelete, rootDelete])
        #expect(reversed.map(\.zoneID) == groups.map(\.zoneID))
    }

    @Test func fetchedDeletionOnlyZoneIsRetained() throws {
        let zone = CKRecordZone.ID(zoneName: "deletion-only", ownerName: CKCurrentUserDefaultName)
        let deleted = CKRecord.ID(recordName: "deleted", zoneID: zone)
        let groups = NativeCloudKitRecordRouting.groups(records: [], deleted: [deleted])
        #expect(groups.count == 1)
        let group = try #require(groups.first)
        #expect(group.zoneID == zone)
        #expect(group.records.isEmpty)
        #expect(group.deleted == [deleted])
    }

    @Test func fetchedZonesWithDifferentOwnersRemainSeparate() {
        let first = CKRecordZone.ID(zoneName: "same-zone", ownerName: "owner-a")
        let second = CKRecordZone.ID(zoneName: "same-zone", ownerName: "owner-b")
        let firstDelete = CKRecord.ID(recordName: "same-name", zoneID: first)
        let secondDelete = CKRecord.ID(recordName: "same-name", zoneID: second)
        let groups = NativeCloudKitRecordRouting.groups(records: [], deleted: [secondDelete, firstDelete])
        #expect(groups.map(\.zoneID) == [first, second])
        #expect(groups.map(\.deleted) == [[firstDelete], [secondDelete]])
    }

    @Test func emptyFetchedBatchHasNoZoneGroups() {
        #expect(NativeCloudKitRecordRouting.groups(records: [], deleted: []).isEmpty)
    }
}
#endif
