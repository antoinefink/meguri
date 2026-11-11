#if canImport(CloudKit)
import CloudKit
import Foundation
import Testing
@testable import CoreBridge

struct NativeCloudKitEngineTests {
    #if DEBUG
    @Test func processFetchClassificationUsesOriginalFullZoneAndRetainsNoAssociatedData() {
        let sentinel = UUID().uuidString
        let collection = CKRecordZone.ID(zoneName: sentinel, ownerName: CKCurrentUserDefaultName)
        let root = CKRecordZone.ID(zoneName: "meguri-root", ownerName: CKCurrentUserDefaultName)
        let cases: [(CKRecordZone.ID, String?, DebugCloudDiagnostics.FetchZone)] = [
            (collection, sentinel, .collection),
            (collection, nil, .other),
            (.init(zoneName: sentinel, ownerName: "foreign-owner"), sentinel, .other),
            (.init(zoneName: sentinel + "-other", ownerName: CKCurrentUserDefaultName), sentinel, .other),
            (root, sentinel, .root), (root, nil, .root),
            (.init(zoneName: "meguri-root", ownerName: "foreign-owner"), sentinel, .other),
            (.init(zoneName: "meguri-media", ownerName: CKCurrentUserDefaultName), sentinel, .other)
        ]
        let recorder = DebugCloudDiagnostics.FetchRecorder()
        for (id, registered, expected) in cases {
            let classification = NativeCloudKitIdentityValidation.debugFetchZone(id, collectionZone: registered)
            #expect(classification == expected)
            switch classification {
            case .collection: recorder.record(.modC)
            case .root: recorder.record(.modR)
            case .other: break
            }
        }
        #expect(recorder.formatted == "processAll{will=0 zoneC=0 modC=1 modR=2 coreC=0 refused=0 zoneFail=0 overflow=0}")
        for excluded in [sentinel, "foreign-owner", "meguri-root", "meguri-media"] {
            #expect(!recorder.formatted.contains(excluded))
        }
    }
    #endif

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

    @Test func validatedFetchedChangesKeepCurrentOwnerIDsAndDeletionOnlyGroups() throws {
        let root = CKRecordZone.ID(zoneName: "meguri-root", ownerName: CKCurrentUserDefaultName)
        let collection = CKRecordZone.ID(zoneName: "meguri-collection", ownerName: CKCurrentUserDefaultName)
        let record = CKRecord(recordType: "MGActive", recordID: .init(recordName: "root", zoneID: root))
        let deleted = CKRecord.ID(recordName: "deleted", zoneID: collection)
        let groups = try NativeCloudKitRecordRouting.validatedGroups(records: [record], deleted: [deleted])
        #expect(groups.map(\.zoneID) == [collection, root])
        let deletionOnly = try #require(groups.first)
        #expect(deletionOnly.records.isEmpty)
        #expect(deletionOnly.deleted == [deleted])
        let modified = try #require(groups.last)
        #expect(modified.records.map(\.recordID) == [record.recordID])
        #expect(modified.deleted.isEmpty)
        #expect(try NativeCloudKitRecordRouting.validatedGroups(records: [], deleted: []).isEmpty)
    }

    @Test func mixedFetchedOwnersRejectBeforeAnyGroupProjection() {
        let current = CKRecordZone.ID(zoneName: "same-zone", ownerName: CKCurrentUserDefaultName)
        let foreign = CKRecordZone.ID(zoneName: "same-zone", ownerName: "foreign-owner")
        let valid = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "same-name", zoneID: current))
        let invalid = CKRecord(recordType: "MGEntity", recordID: .init(recordName: "same-name", zoneID: foreign))
        let validDelete = CKRecord.ID(recordName: "same-name", zoneID: current)
        let invalidDelete = CKRecord.ID(recordName: "same-name", zoneID: foreign)
        var projectedZones: [CKRecordZone.ID] = []
        #expect(throws: NativeCloudKitFailure.self) {
            projectedZones = try NativeCloudKitRecordRouting.validatedGroups(records: [valid, invalid], deleted: [validDelete]).map(\.zoneID)
        }
        #expect(projectedZones.isEmpty)
        #expect(throws: NativeCloudKitFailure.self) {
            projectedZones = try NativeCloudKitRecordRouting.validatedGroups(records: [valid], deleted: [validDelete, invalidDelete]).map(\.zoneID)
        }
        #expect(projectedZones.isEmpty)
    }

    @Test func sentResultsResolveOnlyWithinConfiguredFullZone() throws {
        let zone = CKRecordZone.ID(zoneName: "collection", ownerName: CKCurrentUserDefaultName)
        let saved = CKRecord.ID(recordName: "saved", zoneID: zone)
        let failed = CKRecord.ID(recordName: "failed", zoneID: zone)
        let deleted = CKRecord.ID(recordName: "deleted", zoneID: zone)
        #expect(try NativeCloudKitRecordRouting.sentBatchID(saved: [saved], failed: [failed], deleted: [deleted], zoneID: zone, batches: ["saved": 42, "failed": 42, "deleted": 42], pendingBatch: nil) == 42)
        #expect(try NativeCloudKitRecordRouting.sentBatchID(saved: [], failed: [], deleted: [deleted], zoneID: zone, batches: [:], pendingBatch: 42) == 42)
        #expect(try NativeCloudKitRecordRouting.sentBatchID(saved: [], failed: [], deleted: [deleted], zoneID: zone, batches: ["deleted": 41], pendingBatch: 42) == 42)
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitRecordRouting.sentBatchID(saved: [saved], failed: [failed], deleted: [], zoneID: zone, batches: ["saved": 42, "failed": 43], pendingBatch: 42)
        }
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitRecordRouting.sentBatchID(saved: [saved], failed: [], deleted: [], zoneID: zone, batches: [:], pendingBatch: nil)
        }
    }

    @Test func foreignSentResultsCannotResolveCollidingBatchNamesOrFallback() {
        let zone = CKRecordZone.ID(zoneName: "collection", ownerName: CKCurrentUserDefaultName)
        let valid = CKRecord.ID(recordName: "same-name", zoneID: zone)
        let collisions = [
            CKRecordZone.ID(zoneName: zone.zoneName, ownerName: "foreign-owner"),
            CKRecordZone.ID(zoneName: "another-collection", ownerName: CKCurrentUserDefaultName)
        ]
        for otherZone in collisions {
            let invalid = CKRecord.ID(recordName: valid.recordName, zoneID: otherZone)
            #expect(throws: NativeCloudKitFailure.self) {
                try NativeCloudKitRecordRouting.sentBatchID(saved: [valid, invalid], failed: [], deleted: [], zoneID: zone, batches: [valid.recordName: 42], pendingBatch: 42)
            }
            #expect(throws: NativeCloudKitFailure.self) {
                try NativeCloudKitRecordRouting.sentBatchID(saved: [valid], failed: [invalid], deleted: [], zoneID: zone, batches: [valid.recordName: 42], pendingBatch: 42)
            }
            #expect(throws: NativeCloudKitFailure.self) {
                try NativeCloudKitRecordRouting.sentBatchID(saved: [valid], failed: [], deleted: [invalid], zoneID: zone, batches: [valid.recordName: 42], pendingBatch: 42)
            }
        }
    }

    @Test func zoneCallbackProjectionRejectsForeignOwnerWithoutDrainEvidence() throws {
        let zone = CKRecordZone.ID(zoneName: "meguri-root", ownerName: CKCurrentUserDefaultName)
        let foreign = CKRecordZone.ID(zoneName: zone.zoneName, ownerName: "foreign-owner")
        #expect(CloudEngineEvent.willFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(zone)) == .willFetchZone(zone: zone.zoneName))
        #expect(CloudEngineEvent.didFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(zone), error: nil) == .didFetchZone(zone: zone.zoneName, error: nil))
        var events: [CloudEngineEvent] = []
        #expect(throws: NativeCloudKitFailure.self) {
            events.append(.willFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(foreign)))
        }
        #expect(throws: NativeCloudKitFailure.self) {
            events.append(.didFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(foreign), error: nil))
        }
        #expect(events.isEmpty)
    }

    @Test func fetchScopeValidationPreservesExplicitAndScheduledScopes() throws {
        let root = CKRecordZone.ID(zoneName: "meguri-root", ownerName: CKCurrentUserDefaultName)
        let collection = CKRecordZone.ID(zoneName: "collection", ownerName: CKCurrentUserDefaultName)
        let explicit = try NativeCloudKitIdentityValidation.fetchOptions(zoneIDs: [collection, root], priorityZones: [root], fallback: ["unused"], groupID: "manual")
        #expect(explicit == CloudFetchOptions(zones: [collection.zoneName, root.zoneName], priorityZones: [root.zoneName], groupID: "manual"))
        let empty = try NativeCloudKitIdentityValidation.fetchOptions(zoneIDs: [], priorityZones: [], fallback: ["unused"], groupID: "empty")
        #expect(empty.zones.isEmpty)
        let scheduled = try NativeCloudKitIdentityValidation.fetchOptions(zoneIDs: nil, priorityZones: [collection], fallback: [root.zoneName, collection.zoneName], groupID: "scheduled")
        #expect(scheduled == CloudFetchOptions(zones: [root.zoneName, collection.zoneName], priorityZones: [collection.zoneName], groupID: "scheduled"))
    }

    @Test func foreignExplicitOrPrioritizedFetchScopeCannotBeProjected() {
        let zone = CKRecordZone.ID(zoneName: "collection", ownerName: CKCurrentUserDefaultName)
        let foreign = CKRecordZone.ID(zoneName: zone.zoneName, ownerName: "foreign-owner")
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitIdentityValidation.fetchOptions(zoneIDs: [zone, foreign], priorityZones: [zone], fallback: [], groupID: "manual")
        }
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitIdentityValidation.fetchOptions(zoneIDs: [zone], priorityZones: [zone, foreign], fallback: [], groupID: "manual")
        }
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitIdentityValidation.fetchOptions(zoneIDs: nil, priorityZones: [foreign], fallback: [zone.zoneName], groupID: "scheduled")
        }
    }

    @Test func validPendingChangesProjectAndSelectOriginalNativeIDs() throws {
        let zone = CKRecordZone.ID(zoneName: "collection", ownerName: CKCurrentUserDefaultName)
        let saved = CKRecord.ID(recordName: "saved", zoneID: zone)
        let deleted = CKRecord.ID(recordName: "deleted", zoneID: zone)
        let pending = try NativeCloudKitPendingRecordChanges([.saveRecord(saved), .deleteRecord(deleted)], zoneID: zone)
        #expect(pending.byName == ["saved": .save, "deleted": .delete])
        let removals = pending.removals(names: [deleted.recordName, "absent"])
        #expect(removals.count == 1)
        switch try #require(removals.first) {
        case .deleteRecord(let id): #expect(id == deleted)
        default: Issue.record("Expected the original native deletion")
        }
    }

    @Test func pendingSaveAndDeleteCollisionsRejectBeforeSelectionOrRemoval() {
        let zone = CKRecordZone.ID(zoneName: "collection", ownerName: CKCurrentUserDefaultName)
        let valid = CKRecord.ID(recordName: "same-name", zoneID: zone)
        for otherZone in [CKRecordZone.ID(zoneName: zone.zoneName, ownerName: "foreign-owner"), CKRecordZone.ID(zoneName: "another-collection", ownerName: CKCurrentUserDefaultName)] {
            let invalid = CKRecord.ID(recordName: valid.recordName, zoneID: otherZone)
            let inputs: [[CKSyncEngine.PendingRecordZoneChange]] = [
                [.saveRecord(valid), .deleteRecord(invalid)],
                [.deleteRecord(valid), .saveRecord(invalid)]
            ]
            for changes in inputs {
                #expect(throws: NativeCloudKitFailure.self) {
                    try NativeCloudKitPendingRecordChanges(changes, zoneID: zone).byName
                }
                #expect(throws: NativeCloudKitFailure.self) {
                    try NativeCloudKitPendingRecordChanges(changes, zoneID: zone).removals(names: [valid.recordName])
                }
            }
        }
    }

    @Test func pendingDatabaseChangesValidateAllOwnersBeforeProjectionOrRemoval() throws {
        let zone = CKRecordZone.ID(zoneName: "collection", ownerName: CKCurrentUserDefaultName)
        let root = CKRecordZone.ID(zoneName: "meguri-root", ownerName: CKCurrentUserDefaultName)
        let foreign = CKRecordZone.ID(zoneName: zone.zoneName, ownerName: "foreign-owner")
        #expect(try NativeCloudKitIdentityValidation.databaseZoneNames([.saveZone(CKRecordZone(zoneID: zone)), .deleteZone(root)]) == [zone.zoneName, root.zoneName])
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitIdentityValidation.databaseZoneNames([.saveZone(CKRecordZone(zoneID: zone)), .deleteZone(foreign)])
        }
        #expect(throws: NativeCloudKitFailure.self) {
            try NativeCloudKitIdentityValidation.databaseZoneNames([.deleteZone(zone), .saveZone(CKRecordZone(zoneID: foreign))])
        }
    }
}
extension NativeCloudKitEngineTests {
    @Test func actualUnknownItemRetainsItsSemanticSendFailure() {
        #expect(NativeCloudKitSendError.code(CKError(.unknownItem)) == .unknownItem)
        #expect(NativeCloudKitSendError.code(CKError(.zoneNotFound)) == .zoneNotFound)
        #expect(NativeCloudKitSendError.code(CKError(.networkFailure)) == .networkFailure)
        #expect(NativeCloudKitSendError.code(NSError(domain: "synthetic", code: CKError.Code.unknownItem.rawValue)) == .other)
    }
}
#endif
