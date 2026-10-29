#if canImport(CloudKit)
import CloudKit
import Foundation

public enum NativeCloudKitFailure: Error { case invalidSerialization, invalidSystemFields, unsupportedRecord, unsupportedAsset }

enum NativeCloudKitIdentityValidation {
    static func requireCurrentOwner(_ id: CKRecordZone.ID) throws {
        guard id.ownerName == CKCurrentUserDefaultName else { throw NativeCloudKitFailure.unsupportedRecord }
    }
    static func zoneName(_ id: CKRecordZone.ID) throws -> String {
        try requireCurrentOwner(id)
        return id.zoneName
    }
    static func zoneNames(_ ids: [CKRecordZone.ID]) throws -> [String] {
        for id in ids { try requireCurrentOwner(id) }
        return ids.map(\.zoneName)
    }
    static func collectionRecordNames(_ ids: [CKRecord.ID], zoneID: CKRecordZone.ID) throws -> [String] {
        try requireCurrentOwner(zoneID)
        guard ids.allSatisfy({ $0.zoneID == zoneID }) else { throw NativeCloudKitFailure.unsupportedRecord }
        return ids.map(\.recordName)
    }
    static func fetchOptions(zoneIDs: [CKRecordZone.ID]?, priorityZones: [CKRecordZone.ID], fallback: [String], groupID: String) throws -> CloudFetchOptions {
        for id in (zoneIDs ?? []) + priorityZones { try requireCurrentOwner(id) }
        return .init(zones: zoneIDs?.map(\.zoneName) ?? fallback, priorityZones: priorityZones.map(\.zoneName), groupID: groupID)
    }
    static func databaseZoneNames(_ changes: [CKSyncEngine.PendingDatabaseChange]) throws -> Set<String> {
        let ids = try changes.map { change in
            switch change {
            case .saveZone(let zone): return zone.zoneID
            case .deleteZone(let id): return id
            @unknown default: throw NativeCloudKitFailure.unsupportedRecord
            }
        }
        return Set(try zoneNames(ids))
    }
}

struct NativeCloudKitPendingRecordChanges {
    private let changes: [CKSyncEngine.PendingRecordZoneChange]
    init(_ changes: [CKSyncEngine.PendingRecordZoneChange], zoneID: CKRecordZone.ID) throws {
        let ids = try changes.map { change in
            switch change {
            case .saveRecord(let id), .deleteRecord(let id): return id
            @unknown default: throw NativeCloudKitFailure.unsupportedRecord
            }
        }
        _ = try NativeCloudKitIdentityValidation.collectionRecordNames(ids, zoneID: zoneID)
        self.changes = changes
    }
    var byName: [String: CloudPendingChange] {
        var result: [String: CloudPendingChange] = [:]
        for change in changes {
            switch change {
            case .saveRecord(let id): result[id.recordName] = .save
            case .deleteRecord(let id): result[id.recordName] = .delete
            @unknown default: break
            }
        }
        return result
    }
    func removals(names: Set<String>) -> [CKSyncEngine.PendingRecordZoneChange] {
        changes.filter { change in
            switch change {
            case .saveRecord(let id), .deleteRecord(let id): names.contains(id.recordName)
            @unknown default: false
            }
        }
    }
}

enum NativeCloudKitRecordRouting {
    struct Group {
        let zoneID: CKRecordZone.ID
        let records: [CKRecord]
        let deleted: [CKRecord.ID]
    }
    static func validatedGroups(records: [CKRecord], deleted: [CKRecord.ID]) throws -> [Group] {
        _ = try NativeCloudKitIdentityValidation.zoneNames(records.map { $0.recordID.zoneID } + deleted.map(\.zoneID))
        return groups(records: records, deleted: deleted)
    }
    static func sentBatchID(saved: [CKRecord.ID], failed: [CKRecord.ID], deleted: [CKRecord.ID], zoneID: CKRecordZone.ID, batches: [String: UInt64], pendingBatch: UInt64?) throws -> UInt64 {
        _ = try NativeCloudKitIdentityValidation.collectionRecordNames(saved + failed + deleted, zoneID: zoneID)
        let ids = Set((saved + failed).compactMap { batches[$0.recordName] })
        guard ids.count <= 1, let batch = ids.first ?? pendingBatch else { throw NativeCloudKitFailure.unsupportedRecord }
        return batch
    }
    static func groups(records: [CKRecord], deleted: [CKRecord.ID]) -> [Group] {
        let recordsByZone = Dictionary(grouping: records, by: { $0.recordID.zoneID })
        let deletionsByZone = Dictionary(grouping: deleted, by: { $0.zoneID })
        let zones = Set(recordsByZone.keys).union(deletionsByZone.keys).sorted {
            if $0.zoneName != $1.zoneName { return $0.zoneName < $1.zoneName }
            return $0.ownerName < $1.ownerName
        }
        return zones.map { Group(zoneID: $0, records: recordsByZone[$0] ?? [], deleted: deletionsByZone[$0] ?? []) }
    }
}

/// The application supplies its private database and resolves account identity
/// asynchronously. No account read or foreign callback runs under a core lock.
public actor NativeCloudKitEngineFactory: CloudKitEngineFactory {
    let database: CKDatabase
    private let account: @Sendable () async throws -> CloudResolvedAccount
    private var accountObservers: [UUID: any NSObjectProtocol] = [:]
    public init(database: CKDatabase, accountResolver: @escaping @Sendable () async throws -> CloudResolvedAccount) {
        self.database = database; account = accountResolver
    }
    public func accountChanges() -> AsyncStream<Void>? {
        let (stream, continuation) = AsyncStream<Void>.makeStream()
        let id = UUID()
        accountObservers[id] = NotificationCenter.default.addObserver(forName: .CKAccountChanged, object: nil, queue: nil) { _ in continuation.yield(()) }
        continuation.onTermination = { [weak self] _ in Task { await self?.removeAccountObserver(id) } }
        return stream
    }
    private func removeAccountObserver(_ id: UUID) {
        if let observer = accountObservers.removeValue(forKey: id) { NotificationCenter.default.removeObserver(observer) }
    }
    public func resolveAccount() async throws -> CloudResolvedAccount? { try await account() }
    public func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async throws -> any CloudKitEngine {
        let serialization: CKSyncEngine.State.Serialization?
        if let data = configuration.serialization {
            do { serialization = try JSONDecoder().decode(CKSyncEngine.State.Serialization.self, from: data) }
            catch { throw NativeCloudKitFailure.invalidSerialization }
        } else { serialization = nil }
        let engine = NativeCloudKitEngine(instanceID: configuration.instanceID, delegate: delegate)
        await engine.setCollectionZone(configuration.collectionZone)
        await engine.configure(database: database, serialization: serialization, automatic: configuration.automaticallySync)
        return engine
    }
}

/// Owns the actual Apple SDK object and its serial event delivery slot. Each
/// factory invocation creates a new delegate object with an immutable identity.
private actor NativeCloudKitEngine: CloudKitEngine, CKSyncEngineDelegate {
    nonisolated let instanceID: String
    private var sdk: CKSyncEngine?
    private let delegate: any CloudKitEngineDelegate
    private var activeEvent = false
    private var eventWaiters: [CheckedContinuation<Void, Never>] = []
    private var returnedWork: Task<Void, Never>?
    private var batches: [String: UInt64] = [:]
    private var pendingBatch: UInt64?
    private var failure: (any Error)?
    init(instanceID: String, delegate: any CloudKitEngineDelegate) {
        self.instanceID = instanceID; self.delegate = delegate
    }
    func configure(database: CKDatabase, serialization: CKSyncEngine.State.Serialization?, automatic: Bool) {
        var configuration = CKSyncEngine.Configuration(database: database, stateSerialization: serialization, delegate: self)
        configuration.automaticallySync = automatic
        sdk = CKSyncEngine(configuration)
    }
    private func zone(_ name: String) -> CKRecordZone.ID { .init(zoneName: name, ownerName: CKCurrentUserDefaultName) }
    private func recordID(_ name: String, zone: String) -> CKRecord.ID { .init(recordName: name, zoneID: self.zone(zone)) }
    private var collectionZone: String?
    private func validatedPendingRecords() throws -> NativeCloudKitPendingRecordChanges {
        guard let collectionZone else { throw NativeCloudKitFailure.unsupportedRecord }
        return try .init(sdk?.state.pendingRecordZoneChanges ?? [], zoneID: zone(collectionZone))
    }
    func pendingRecordZoneChanges() -> [String: CloudPendingChange] {
        guard failure == nil else { return [:] }
        do { return try validatedPendingRecords().byName }
        catch { failure = error; return [:] }
    }
    func pendingDatabaseChanges() -> Set<String> {
        guard failure == nil else { return [] }
        do { return try NativeCloudKitIdentityValidation.databaseZoneNames(sdk?.state.pendingDatabaseChanges ?? []) }
        catch { failure = error; return [] }
    }
    func applyPendingUpdate(_ update: CloudPendingUpdate) throws {
        if let failure { throw failure }
        guard let sdk else { return }
        do {
            let pending = try validatedPendingRecords()
            guard let collectionZone else { throw NativeCloudKitFailure.unsupportedRecord }
            let removes = pending.removals(names: update.removes)
            let adds: [CKSyncEngine.PendingRecordZoneChange] = update.adds.map { name, change in
                let id = recordID(name, zone: collectionZone)
                return change == .save ? .saveRecord(id) : .deleteRecord(id)
            }
            sdk.state.remove(pendingRecordZoneChanges: removes)
            sdk.state.add(pendingRecordZoneChanges: adds)
        } catch {
            failure = error
            throw error
        }
    }
    func setCollectionZone(_ zone: String) { collectionZone = zone }
    func removePendingDatabaseChanges() {
        guard failure == nil, let sdk else { return }
        do {
            let changes = sdk.state.pendingDatabaseChanges
            _ = try NativeCloudKitIdentityValidation.databaseZoneNames(changes)
            sdk.state.remove(pendingDatabaseChanges: changes)
        } catch { failure = error }
    }
    func sendChanges(zones: [String]) async throws {
        guard let sdk else { throw CancellationError() }
        if let failure { throw failure }
        if let returnedWork { await returnedWork.value }
        if let failure { throw failure }
        try await sdk.sendChanges(.init(scope: .zoneIDs(zones.map(zone))))
        if let returnedWork { await returnedWork.value }
        if let failure { throw failure }
    }
    func fetchChanges(options: CloudFetchOptions) async throws {
        guard let sdk else { throw CancellationError() }
        if let failure { throw failure }
        if let returnedWork { await returnedWork.value }
        if let failure { throw failure }
        let group = CKOperationGroup(); group.name = options.groupID
        var native = CKSyncEngine.FetchChangesOptions(scope: .zoneIDs(options.zones.map(zone)), operationGroup: group)
        native.prioritizedZoneIDs = options.priorityZones.map(zone)
        try await sdk.fetchChanges(native)
        if let returnedWork { await returnedWork.value }
        if let failure { throw failure }
    }
    func cancelOperations() async { await sdk?.cancelOperations(); if let returnedWork { await returnedWork.value } }
    func release() { sdk = nil }
    private func fields(_ record: CKRecord) throws -> Data {
        try NativeCloudKitSystemFields.encode(record)
    }
    private func decode(_ record: CKRecord) throws -> CloudRecord {
        try NativeCloudKitIdentityValidation.requireCurrentOwner(record.recordID.zoneID)
        guard ["MGEntity", "MGActive", "MGEpoch"].contains(record.recordType) else { throw NativeCloudKitFailure.unsupportedRecord }
        guard record["pa"] == nil && record["ma"] == nil && record["a"] == nil else { throw NativeCloudKitFailure.unsupportedAsset }
        guard let bytes = record.encryptedValues["e"] as? Data else { throw NativeCloudKitFailure.unsupportedRecord }
        return CloudRecord(name: record.recordID.recordName, envelope: bytes, systemFields: try fields(record))
    }
    private func encode(_ record: CloudRecord, zone: String) throws -> CKRecord {
        let id = recordID(record.name, zone: zone)
        let native: CKRecord
        if let fields = record.systemFields {
            let decoder = try NSKeyedUnarchiver(forReadingFrom: fields)
            decoder.requiresSecureCoding = true
            guard let restored = CKRecord(coder: decoder), restored.recordID == id, restored.recordType == "MGEntity" else { throw NativeCloudKitFailure.invalidSystemFields }
            decoder.finishDecoding(); native = restored
        } else { native = CKRecord(recordType: "MGEntity", recordID: id) }
        native.encryptedValues["e"] = record.envelope as CKRecordValue
        return native
    }
    private func code(_ error: any Error) -> CloudEngineErrorCode {
        guard let error = error as? CKError else { return .other }
        return switch error.code {
        case .notAuthenticated: .notAuthenticated
        case .accountTemporarilyUnavailable: .accountTemporarilyUnavailable
        case .networkFailure: .networkFailure
        case .networkUnavailable: .networkUnavailable
        case .requestRateLimited: .requestRateLimited
        case .serviceUnavailable: .serviceUnavailable
        case .zoneBusy: .zoneBusy
        case .limitExceeded: .limitExceeded
        case .batchRequestFailed: .batchRequestFailed
        case .zoneNotFound: .zoneNotFound
        default: .other
        }
    }
    func handleEvent(_ event: CKSyncEngine.Event, syncEngine: CKSyncEngine) async {
        if activeEvent { await withCheckedContinuation { eventWaiters.append($0) } }
        else { activeEvent = true }
        if let returnedWork { await returnedWork.value }
        await delegate.eventWillDeliver(engine: self)
        do {
            if failure != nil { throw NativeCloudKitFailure.unsupportedRecord }
            let forwarded: CloudEngineEvent
            switch event {
            case .stateUpdate(let update): forwarded = .stateUpdate(try JSONEncoder().encode(update.stateSerialization))
            case .accountChange(let change):
                let kind: CloudAccountChange
                switch change.changeType { case .signIn: kind = .signIn; case .signOut: kind = .signOut; case .switchAccounts: kind = .switchAccounts; @unknown default: throw NativeCloudKitFailure.unsupportedRecord }
                forwarded = .accountChanged(change: kind, identity: nil)
            case .fetchedRecordZoneChanges(let fetched):
                let groups = try NativeCloudKitRecordRouting.validatedGroups(records: fetched.modifications.map(\.record), deleted: fetched.deletions.map(\.recordID))
                let events: [CloudEngineEvent] = try groups.map {
                    .fetched(zone: $0.zoneID.zoneName, records: try $0.records.map(decode), deleted: $0.deleted.map(\.recordName))
                }
                for event in events.dropLast() { await delegate.handleEvent(event, engine: self) }
                // Empty batches own a callback too; no zone or drain is inferred.
                forwarded = events.last ?? .databaseChanges
            case .sentRecordZoneChanges(let sent):
                guard let collectionZone else { throw NativeCloudKitFailure.unsupportedRecord }
                let batch = try NativeCloudKitRecordRouting.sentBatchID(saved: sent.savedRecords.map(\.recordID), failed: sent.failedRecordSaves.map { $0.record.recordID }, deleted: sent.deletedRecordIDs, zoneID: zone(collectionZone), batches: batches, pendingBatch: pendingBatch)
                forwarded = .sent(batchID: batch, saved: try sent.savedRecords.map(decode), failed: sent.failedRecordSaves.map { .init(name: $0.record.recordID.recordName, code: code($0.error)) }, deleted: sent.deletedRecordIDs.map(\.recordName))
            case .willFetchChanges(let will): forwarded = .willFetch(groupID: will.context.options.operationGroup.name ?? "", reason: will.context.reason == .manual ? .manual : .scheduled)
            case .willFetchRecordZoneChanges(let will):
                forwarded = .willFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(will.zoneID))
            case .didFetchRecordZoneChanges(let did):
                forwarded = .didFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(did.zoneID), error: did.error.map(code))
            case .didFetchChanges(let did): forwarded = .didFetch(groupID: did.context.options.operationGroup.name ?? "", reason: did.context.reason == .manual ? .manual : .scheduled)
            case .willSendChanges: forwarded = .willSend(zone: collectionZone ?? "")
            case .didSendChanges: forwarded = .didSend(zone: collectionZone ?? "")
            case .fetchedDatabaseChanges(let changes):
                guard changes.deletions.isEmpty else { throw NativeCloudKitFailure.unsupportedRecord }
                forwarded = .databaseChanges
            case .sentDatabaseChanges: throw NativeCloudKitFailure.unsupportedRecord
            @unknown default: throw NativeCloudKitFailure.unsupportedRecord
            }
            do {
                await delegate.handleEvent(forwarded, engine: self)
                // Enqueued on this actor with no further suspension before this
                // SDK callback returns. The next event joins the completion.
                returnedWork = Task { await self.returned(forwarded) }
            }
        } catch {
            failure = error
            // A native decoding failure is terminal; it cannot be followed by
            // a successful opaque-state persistence through this instance.
            await delegate.handleEvent(.deliveryFailed, engine: self)
            returnedWork = Task { await self.returned(.deliveryFailed) }
        }
    }
    private func returned(_ event: CloudEngineEvent) async {
        await delegate.eventHandlerReturned(event, engine: self)
        if eventWaiters.isEmpty { activeEvent = false }
        else { eventWaiters.removeFirst().resume() }
    }
    func nextRecordZoneChangeBatch(_ context: CKSyncEngine.SendChangesContext, syncEngine: CKSyncEngine) async -> CKSyncEngine.RecordZoneChangeBatch? {
        guard failure == nil, let collectionZone else { return nil }
        if let returnedWork { await returnedWork.value }
        var pending = pendingRecordZoneChanges()
        guard failure == nil else { return nil }
        switch context.options.scope {
        case .zoneIDs(let ids): guard ids.contains(zone(collectionZone)) else { return nil }
        case .recordIDs(let ids):
            let allowed = Set(ids.filter { $0.zoneID == zone(collectionZone) }.map(\.recordName))
            pending = pending.filter { allowed.contains($0.key) }
        case .allExcluding(let ids): guard !ids.contains(zone(collectionZone)) else { return nil }
        case .all: break
        @unknown default: return nil
        }
        guard let batch = await delegate.nextRecordZoneChangeBatch(pending: pending, zone: collectionZone, engine: self) else { return nil }
        guard failure == nil else { return nil }
        do {
            let saves = try batch.saves.map { try encode($0, zone: collectionZone) }
            pendingBatch = batch.id
            for save in saves { batches[save.recordID.recordName] = batch.id }
            return .init(recordsToSave: saves, recordIDsToDelete: batch.deletes.map { recordID($0, zone: collectionZone) }, atomicByZone: false)
        } catch {
            failure = error
            await delegate.eventWillDeliver(engine: self)
            await delegate.handleEvent(.deliveryFailed, engine: self)
            returnedWork = Task { await self.returned(.deliveryFailed) }
            return nil
        }
    }
    func nextFetchChangesOptions(_ context: CKSyncEngine.FetchChangesContext, syncEngine: CKSyncEngine) async -> CKSyncEngine.FetchChangesOptions {
        guard failure == nil, let collectionZone else { return .init(scope: .zoneIDs([])) }
        let group = context.options.operationGroup
        // Preserve manual scopes exactly; scheduled callbacks are narrowed to
        // the registered root and collection zones.
        let requested: [CKRecordZone.ID]?
        switch context.options.scope {
        case .zoneIDs(let ids): requested = ids
        default: requested = nil
        }
        do {
            let input = try NativeCloudKitIdentityValidation.fetchOptions(zoneIDs: requested, priorityZones: context.options.prioritizedZoneIDs, fallback: ["meguri-root", collectionZone], groupID: group.name ?? "")
            let options = await delegate.nextFetchChangesOptions(input, engine: self)
            guard failure == nil else { return .init(scope: .zoneIDs([])) }
            var result = CKSyncEngine.FetchChangesOptions(scope: .zoneIDs(options.zones.map(zone)), operationGroup: group)
            result.prioritizedZoneIDs = options.priorityZones.map(zone)
            return result
        } catch {
            failure = error
            return .init(scope: .zoneIDs([]))
        }
    }
}
#endif

#if canImport(CloudKit)
extension CloudKitSyncAdapter {
    public init(core: CoreCloudWorkflowService, database: CKDatabase, accountResolver: @escaping @Sendable () async throws -> CloudResolvedAccount) {
        self.init(core: core, engineFactory: NativeCloudKitEngineFactory(database: database, accountResolver: accountResolver))
    }
}
#endif
