#if canImport(CloudKit)
import CloudKit
import Foundation

public enum NativeCloudKitFailure: Error { case invalidSerialization, invalidSystemFields, unsupportedRecord, unsupportedAsset }

enum NativeCloudKitIdentityValidation {
    #if DEBUG
    // Compare original full zone identity in memory; retain only a finite class.
    static func debugFetchZone(_ id: CKRecordZone.ID, collectionZone: String?) -> DebugCloudDiagnostics.FetchZone {
        guard id.ownerName == CKCurrentUserDefaultName else { return .other }
        if let collectionZone, id.zoneName == collectionZone { return .collection }
        return id.zoneName == "meguri-root" ? .root : .other
    }
    // Caller admits the original SDK. Compare full IDs; retain no SDK objects.
    static func debugRecordSentRecord(_ id: CKRecord.ID, collectionZoneID: CKRecordZone.ID?,
                                     saved: Bool, recorder: DebugCloudDiagnostics.FetchRecorder) {
        guard id.zoneID.ownerName == CKCurrentUserDefaultName else { return }
        if let collectionZoneID, id.zoneID == collectionZoneID {
            recorder.recordSent(saved ? .sentC : .sendFailC)
        } else if id.zoneID == CKRecordZone.ID(zoneName: "meguri-media", ownerName: CKCurrentUserDefaultName) {
            recorder.recordSent(saved ? .sentM : .sendFailM)
        }
    }
    #endif
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
    static func validateDatabaseDeletions(_ ids: [CKRecordZone.ID], collectionZone: String?) throws {
        for id in ids { try requireCurrentOwner(id) }
        for id in ids where id.zoneName.hasPrefix("c-") {
            guard collectionZone != nil, isCanonicalCollectionZone(id.zoneName) else {
                throw NativeCloudKitFailure.unsupportedRecord
            }
        }
    }
    private static func isCanonicalCollectionZone(_ name: String) -> Bool {
        // K4.1: 128 bits in lowercase unpadded RFC 4648 base32. The final
        // symbol has two zero pad bits: a/e/i/m/q/u/y/4 are canonical.
        let bytes = Array(name.utf8)
        guard bytes.count == 28, bytes[0] == 99, bytes[1] == 45,
              bytes.dropFirst(2).allSatisfy({ (97...122).contains($0) || (50...55).contains($0) }),
              let last = bytes.last else { return false }
        return [97, 101, 105, 109, 113, 117, 121, 52].contains(last)
    }
    static func databaseDeletionEvent(modified: [CKRecordZone.ID], deleted: [CKRecordZone.ID],
                                      collectionZone: String?) throws -> CloudEngineEvent {
        try databaseDeletionEvent(modified: modified, losses: deleted.map { ($0, .deleted) }, collectionZone: collectionZone)
    }
    static func databaseDeletionEvent(modified: [CKRecordZone.ID], losses: [(CKRecordZone.ID, CloudZoneLossCause)],
                                      collectionZone: String?) throws -> CloudEngineEvent {
        let ids = losses.map { $0.0 }
        for id in modified + ids { try requireCurrentOwner(id) }
        try validateDatabaseDeletions(ids, collectionZone: collectionZone)
        let owned = losses.filter { id, _ in id.zoneName == "meguri-root" || id.zoneName == "meguri-media" || id.zoneName.hasPrefix("c-") }
        guard collectionZone != nil || owned.isEmpty else { throw NativeCloudKitFailure.unsupportedRecord }
        let known = owned.contains { id, _ in id.zoneName == "meguri-root" || id.zoneName == "meguri-media" || id.zoneName == collectionZone }
        if known || owned.contains(where: { $0.1 == .encryptedDataReset }) { return .fetchedZoneLosses(owned.map { .init(zone: $0.0.zoneName, cause: $0.1) }) }
        let names = owned.map { $0.0.zoneName }
        return names.isEmpty ? .databaseChanges : .fetchedCollectionZoneDeletions(names)
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
    init(_ changes: [CKSyncEngine.PendingRecordZoneChange], collectionZoneID: CKRecordZone.ID) throws {
        try NativeCloudKitIdentityValidation.requireCurrentOwner(collectionZoneID)
        for change in changes {
            let id: CKRecord.ID
            switch change { case .saveRecord(let value), .deleteRecord(let value): id = value; @unknown default: throw NativeCloudKitFailure.unsupportedRecord }
            try NativeCloudKitIdentityValidation.requireCurrentOwner(id.zoneID)
            let expected = NativeCloudMediaBatch.zone(name: id.recordName, collection: collectionZoneID.zoneName)
            guard id.zoneID.zoneName == expected else { throw NativeCloudKitFailure.unsupportedRecord }
        }
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
    static func sentEngineBatchID(saved: [CKRecord.ID], failed: [CKRecord.ID], deleted: [CKRecord.ID], collectionZoneID: CKRecordZone.ID, batches: [String: UInt64], pendingBatch: UInt64?) throws -> UInt64 {
        let all = saved + failed + deleted
        if all.allSatisfy({ $0.zoneID == collectionZoneID }) {
            return try sentBatchID(saved: saved, failed: failed, deleted: deleted, zoneID: collectionZoneID, batches: batches, pendingBatch: pendingBatch)
        }
        let mediaZone = NativeCloudOwnRecords.zone("meguri-media")
        try NativeCloudKitIdentityValidation.requireCurrentOwner(collectionZoneID)
        guard !all.isEmpty, all.allSatisfy({ $0.zoneID == mediaZone && NativeCloudMediaRecords.isBlobName($0.recordName) }) else { throw NativeCloudKitFailure.unsupportedRecord }
        let matched = all.compactMap { batches[$0.recordName] }
        guard matched.count == all.count, Set(matched).count == 1, let id = matched.first else { throw NativeCloudKitFailure.unsupportedRecord }
        return id
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
    private let observeNative: (@Sendable (CloudNativeEvidenceObservation) async -> Void)?
    private var accountObservers: [UUID: any NSObjectProtocol] = [:]
    public init(database: CKDatabase, observeNative: (@Sendable (CloudNativeEvidenceObservation) async -> Void)? = nil,
                accountResolver: @escaping @Sendable () async throws -> CloudResolvedAccount) {
        self.database = database; account = accountResolver; self.observeNative = observeNative
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
        let engine = NativeCloudKitEngine(instanceID: configuration.instanceID, delegate: delegate, observeNative: observeNative)
        await engine.setCollectionZone(configuration.collectionZone)
        await engine.configure(database: database, serialization: serialization, automatic: configuration.automaticallySync)
        return engine
    }
}

/// Owns the actual Apple SDK object and its serial event delivery slot. Each
/// factory invocation creates a new delegate object with an immutable identity.
private final class PreparedNativeMediaBatch: @unchecked Sendable {
    let value: CKSyncEngine.RecordZoneChangeBatch
    init(_ value: CKSyncEngine.RecordZoneChangeBatch) { self.value = value }
}
enum NativeCloudKitSendError {
    // Only the actual sent callback's matching server record can take this
    // immutable-blob Saved path. Rust still validates the retained media Work.
    static func savedBlobConflict(record: CKRecord, error: CKError) throws -> CloudRecord? {
        guard record.recordID.zoneID == NativeCloudOwnRecords.zone("meguri-media"),
              error.code == .serverRecordChanged,
              let server = error.serverRecord, server.recordID == record.recordID else { return nil }
        return try NativeCloudRecordConversion.decode(server)
    }
    static func code(_ error: any Error) -> CloudEngineErrorCode {
        if let wrapped = error as? CloudKitSDKOperationFailure { return code(wrapped.underlying) }
        guard (error as NSError).domain == CKErrorDomain, let error = error as? CKError else { return .other }
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
        case .unknownItem: .unknownItem
        case .zoneNotFound:
            ((error as NSError).userInfo[CKErrorUserDidResetEncryptedDataKey] as? NSNumber)?.boolValue == true ? .encryptedDataReset : .zoneNotFound
        case .userDeletedZone: .userDeletedZone
        case .serverRecordChanged: .serverRecordChanged
        default: .other
        }
    }
    /// Only original terminal facts with an exact requested zone can establish
    /// a loss. A multi-zone terminal without item identities is ambiguous.
    static func zoneLosses(_ error: any Error, zones: [String], zoneOperation: Bool = false) throws -> [CloudZoneLoss] {
        if let wrapped = error as? CloudKitSDKOperationFailure {
            return try zoneLosses(wrapped.underlying, zones: zones, zoneOperation: zoneOperation)
        }
        let native = error as NSError
        guard native.domain == CKErrorDomain else { return [] }
        if native.code == CKError.Code.partialFailure.rawValue {
            let items: [AnyHashable: Any]
            if let raw = native.userInfo[CKPartialErrorsByItemIDKey] {
                guard let dictionary = raw as? [AnyHashable: Any] else { throw NativeCloudKitFailure.unsupportedRecord }
                items = dictionary
            } else { items = [:] }
            // Validate the complete dictionary before returning any authority.
            let scoped = try items.map { key, value -> (String, any Error) in
                let id: CKRecordZone.ID
                if !zoneOperation, let record = key.base as? CKRecord.ID { id = record.zoneID }
                else if let zone = key.base as? CKRecordZone.ID { id = zone }
                else { throw NativeCloudKitFailure.unsupportedRecord }
                try NativeCloudKitIdentityValidation.requireCurrentOwner(id)
                guard zones.contains(id.zoneName), let failure = value as? any Error else {
                    throw NativeCloudKitFailure.unsupportedRecord
                }
                return (id.zoneName, failure)
            }
            let losses = scoped.compactMap { NativeCloudOwnRecords.zoneLoss($0.1, zone: $0.0) }
            return losses
        }
        // Reset is account-wide. The original multi-zone operation scope is
        // sufficient without attributing an ordinary absence to a guessed zone.
        if code(error) == .encryptedDataReset {
            return zones.map { .init(zone: $0, cause: .encryptedDataReset) }
        }
        guard zones.count == 1, let zone = zones.first else { return [] }
        return NativeCloudOwnRecords.zoneLoss(error, zone: zone).map { [$0] } ?? []
    }
    static func failure(record: CKRecord, error: CKError, collectionZoneID: CKRecordZone.ID) throws -> CloudRecordFailure {
        if error.code == .serverRecordChanged, record.recordType == "MGBlob",
           record.recordID.zoneID == NativeCloudOwnRecords.zone("meguri-media") {
            // No positive blob proof: park the retained upload as before,
            // without forwarding unverifiable server metadata to collection intake.
            return .init(name: record.recordID.recordName, code: .other,
                zone: record.recordID.zoneID.zoneName, nativeCode: Int64(error.code.rawValue),
                retryAfterMilliseconds: NativeCloudOwnRecords.retryDuration(error).flatMap { Int64(exactly: $0) })
        }
        let serverRecord: CloudRecord?
        if error.code == .serverRecordChanged {
            try NativeCloudKitIdentityValidation.requireCurrentOwner(collectionZoneID)
            guard record.recordID.zoneID == collectionZoneID, record.recordType == "MGEntity",
                  let server = error.serverRecord, server.recordID == record.recordID,
                  server.recordType == "MGEntity" else { throw NativeCloudKitFailure.unsupportedRecord }
            // The shared SDK converter validates encrypted e and archives the
            // actual system fields. It never projects any CKAsset file URL.
            let metadata = try NativeCloudRecordConversion.decode(server)
            guard !metadata.envelope.isEmpty else { throw NativeCloudKitFailure.unsupportedRecord }
            serverRecord = metadata
        } else { serverRecord = nil }
        return .init(name: record.recordID.recordName, code: code(error),
            zone: record.recordID.zoneID.zoneName, nativeCode: Int64(error.code.rawValue),
            retryAfterMilliseconds: NativeCloudOwnRecords.retryDuration(error).flatMap { Int64(exactly: $0) },
            serverRecord: serverRecord)
    }
}

private actor NativeCloudKitEngine: CloudKitEngine, CKSyncEngineDelegate {
    nonisolated let instanceID: String
    private var sdk: CKSyncEngine?
    #if DEBUG
    private var debugFetchOrdinal: UInt16 = 0
    private weak var debugFetchGroup: CKOperationGroup?
    #endif
    private let delegate: any CloudKitEngineDelegate
    private let observeNative: (@Sendable (CloudNativeEvidenceObservation) async -> Void)?
    private var manualSends: [String: CloudNativeSendOperation] = [:]
    private var scheduledSends: [String: CloudNativeSendOperation] = [:]
    private var evidenceTurn = false
    private var evidenceWaiters: [CheckedContinuation<Void, Never>] = []
    private var activeEvent = false
    private var eventWaiters: [CheckedContinuation<Void, Never>] = []
    private var returnedWork: Task<Void, Never>?
    private let networkLifetime = CloudKitNetworkLifetime()
    private var networkEventWaiters: [UUID: CheckedContinuation<Void, Never>] = [:]
    private var batches: [String: UInt64] = [:]
    private var pendingBatch: UInt64?
    private var savedRemoval = NativeCloudSavedRemoval()
    private var failure: (any Error)?
    private func fetchedPayloadStaging() throws -> CloudMediaStaging {
        try CloudMediaStaging.sharedDefault()
    }
    init(instanceID: String, delegate: any CloudKitEngineDelegate,
         observeNative: (@Sendable (CloudNativeEvidenceObservation) async -> Void)? = nil) {
        self.instanceID = instanceID; self.delegate = delegate; self.observeNative = observeNative
    }
    func configure(database: CKDatabase, serialization: CKSyncEngine.State.Serialization?, automatic: Bool) {
        var configuration = CKSyncEngine.Configuration(database: database, stateSerialization: serialization, delegate: self)
        configuration.automaticallySync = automatic
        sdk = CKSyncEngine(configuration)
        #if DEBUG
        debugFetchOrdinal = DebugCloudDiagnostics.nativeFetchSequence.register(
            automatic: automatic, restored: serialization != nil)
        #endif
    }
    private func zone(_ name: String) -> CKRecordZone.ID { .init(zoneName: name, ownerName: CKCurrentUserDefaultName) }
    private func recordID(_ name: String, zone: String) -> CKRecord.ID { .init(recordName: name, zoneID: self.zone(zone)) }
    private var collectionZone: String?
    private func validatedPendingRecords() throws -> NativeCloudKitPendingRecordChanges {
        guard let collectionZone else { throw NativeCloudKitFailure.unsupportedRecord }
        return try .init(sdk?.state.pendingRecordZoneChanges ?? [], collectionZoneID: zone(collectionZone))
    }
    func pendingRecordZoneChanges() -> [String: CloudPendingChange] {
        guard failure == nil else { return [:] }
        do { return try validatedPendingRecords().byName }
        catch { failure = error; return [:] }
    }
    func pendingNamesForStateEvidence() async throws -> Set<String>? {
        guard observeNative != nil else { return nil }
        if let failure { throw failure }
        guard sdk != nil, evidenceTurn else { throw NativeCloudKitFailure.unsupportedRecord }
        return Set(try validatedPendingRecords().byName.keys)
    }
    private func enterEvidenceTurn() async {
        if evidenceTurn { await withCheckedContinuation { evidenceWaiters.append($0) } }
        else { evidenceTurn = true }
    }
    private func leaveEvidenceTurn() {
        if evidenceWaiters.isEmpty { evidenceTurn = false }
        else { evidenceWaiters.removeFirst().resume() }
    }
    func takeSavedPendingRemovals() async -> [CloudSavedPendingRemoval] {
        guard failure == nil, sdk != nil else { return [] }
        do {
            savedRemoval.observe(instance: instanceID, pending: try validatedPendingRecords().byName,
                                 generation: networkLifetime.authority())
            return savedRemoval.take(generation: networkLifetime.authority())
        } catch { failure = error; return [] }
    }
    func pendingDatabaseChanges() -> Set<String> {
        guard failure == nil else { return [] }
        do { return try NativeCloudKitIdentityValidation.databaseZoneNames(sdk?.state.pendingDatabaseChanges ?? []) }
        catch { failure = error; return [] }
    }
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws -> CloudPendingUpdate {
        let observing = observeNative != nil
        let authority = observing ? networkLifetime.authority() : nil
        if observing { await enterEvidenceTurn() }
        defer { if observing { leaveEvidenceTurn() } }
        if let authority, authority != networkLifetime.authority() { throw CancellationError() }
        if let failure { throw failure }
        guard let sdk else { return .init(id: update.id, kind: update.kind, adds: [:], removes: []) }
        do {
            let pending = try validatedPendingRecords()
            savedRemoval.observe(instance: instanceID, pending: pending.byName, generation: networkLifetime.authority())
            guard let collectionZone else { throw NativeCloudKitFailure.unsupportedRecord }
            let applied = update.mutations(to: pending.byName)
            let removes = pending.removals(names: applied.removes)
            let adds: [CKSyncEngine.PendingRecordZoneChange] = applied.adds.map { name, change in
                let id = recordID(name, zone: NativeCloudMediaBatch.zone(name: name, collection: collectionZone))
                return change == .save ? .saveRecord(id) : .deleteRecord(id)
            }
            if !removes.isEmpty || !adds.isEmpty {
                savedRemoval.applicationWillChangePending()
                sdk.state.remove(pendingRecordZoneChanges: removes)
                sdk.state.add(pendingRecordZoneChanges: adds)
            }
            if let observeNative {
                let removed = Dictionary(uniqueKeysWithValues: update.removes.map { ($0, pending.byName[$0] != nil) })
                await observeNative(.pendingUpdated(origin: instanceID, adds: Set(applied.adds.keys), removes: removed))
            }
            return applied
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
    private nonisolated static func networkFailure(_ error: (any Error)?) -> (any Error)? {
        guard let error, let sdkError = error as? CKError else { return error }
        let retry = (sdkError.userInfo[CKErrorRetryAfterKey] as? NSNumber)?.doubleValue
        let milliseconds = retry.flatMap { value -> Int64? in
            guard value.isFinite, value >= 0, value < Double(Int64.max) / 1000 else { return nil }
            return Int64(value * 1000)
        }
        return CloudKitSDKOperationFailure(underlying: error, code: Int64(sdkError.code.rawValue), retryAfterMilliseconds: milliseconds)
    }
    private func drainReturnedWork() async throws {
        if let returnedWork { await returnedWork.value }
        if let failure { throw failure }
    }
    func waitForEventBookkeeping() async throws {
        repeat {
            try await waitForEvents()
            try await drainReturnedWork()
        } while activeEvent
        try Task.checkCancellation()
    }
    private func waitForEvents() async throws {
        let id = UUID()
        try await withTaskCancellationHandler {
            while activeEvent {
                try Task.checkCancellation()
                await withCheckedContinuation { networkEventWaiters[id] = $0 }
            }
            try Task.checkCancellation()
        } onCancel: {
            Task { await self.cancelNetworkEventWaiter(id) }
        }
    }
    private func cancelNetworkEventWaiter(_ id: UUID) {
        networkEventWaiters.removeValue(forKey: id)?.resume()
    }
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        guard let sdk else { throw CancellationError() }
        let options = CKSyncEngine.SendChangesOptions(scope: .zoneIDs(zones.map(zone)))
        let lifetime = networkLifetime
        let generation = lifetime.authority()
        return CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: {
                await sdk.cancelOperations()
                _ = try? await self.drainReturnedWork()
            }) {
                var operationError: (any Error)?
                do {
                    try await self.invokePreparedSend(sdk: sdk, options: options, generation: generation, prepare: prepare, validate: validate)
                } catch { operationError = Self.networkFailure(error) }
                // Even SDK failure/cancellation must drain returned event work.
                do { try await self.drainReturnedWork() }
                catch { if operationError == nil { operationError = error } }
                if let operationError { throw operationError }
            }
        }
    }
    private func invokePreparedSend(sdk: CKSyncEngine, options: CKSyncEngine.SendChangesOptions,
                                    generation: UInt64, prepare: @Sendable () async throws -> Void,
                                    validate: @Sendable () throws -> Void) async throws {
        repeat { try await waitForEventBookkeeping() } while activeEvent
        try await prepare()
        // Preparation suspends the actor; join events and returned work again.
        repeat { try await waitForEventBookkeeping() } while activeEvent
        // Same actor turn: event-idle admission, Core validation, lock release,
        // then the captured SDK invocation without another application await.
        try networkLifetime.enqueue(generation: generation, validate: validate, begin: {})
        guard let observeNative else {
            try await sdk.sendChanges(options)
            return
        }
        var observedOptions = options
        observedOptions.operationGroup = CKOperationGroup()
        let group = observedOptions.operationGroup.operationGroupID
        let operation = CloudNativeSendOperation(identity: UUID(), reason: .manual)
        manualSends[group] = operation
        let result: Result<Void, any Error>
        do {
            try await sdk.sendChanges(observedOptions)
            result = .success(())
        } catch { result = .failure(error) }
        manualSends.removeValue(forKey: group)
        await observeNative(.manualReturned(origin: instanceID, operation: operation))
        try result.get()
    }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        guard let sdk else { throw CancellationError() }
        let group = CKOperationGroup(); group.name = options.groupID
        var native = CKSyncEngine.FetchChangesOptions(scope: .zoneIDs(options.zones.map(zone)), operationGroup: group)
        native.prioritizedZoneIDs = options.priorityZones.map(zone)
        #if DEBUG
        DebugCloudDiagnostics.nativeFetchSequence.record(.prepared(scope: debugFetchScope(native),
            priority: debugFetchPriority(native)), ordinal: debugFetchOrdinal)
        #endif
        let prepared = native
        let lifetime = networkLifetime
        let generation = lifetime.authority()
        return CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: {
                await sdk.cancelOperations()
                _ = try? await self.drainReturnedWork()
            }) {
                var operationError: (any Error)?
                do {
                    try await self.invokePreparedFetch(sdk: sdk, options: prepared, generation: generation, prepare: prepare, validate: validate)
                } catch { operationError = Self.networkFailure(error) }
                do { try await self.drainReturnedWork() }
                catch { if operationError == nil { operationError = error } }
                if let operationError { throw operationError }
            }
        }
    }
    private func invokePreparedFetch(sdk: CKSyncEngine, options: CKSyncEngine.FetchChangesOptions,
                                     generation: UInt64, prepare: @Sendable () async throws -> Void,
                                     validate: @Sendable () throws -> Void) async throws {
        repeat { try await waitForEventBookkeeping() } while activeEvent
        try await prepare()
        // Preparation suspends the actor; join events and returned work again.
        repeat { try await waitForEventBookkeeping() } while activeEvent
        try networkLifetime.enqueue(generation: generation, validate: validate, begin: {})
        #if DEBUG
        let diagnosticOrdinal = debugFetchOrdinal
        debugFetchGroup = options.operationGroup
        DebugCloudDiagnostics.nativeFetchSequence.record(.invoke, ordinal: diagnosticOrdinal)
        defer {
            DebugCloudDiagnostics.nativeFetchSequence.record(.returned, ordinal: diagnosticOrdinal)
            if debugFetchGroup === options.operationGroup { debugFetchGroup = nil }
        }
        #endif
        try await sdk.fetchChanges(options)
    }
    func sendChanges(zones: [String]) async throws {
        let prepared = try await prepareSend(zones: zones)
        let operation = try prepared.enqueue(validate: {})
        try await operation.drain()
    }
    func fetchChanges(options: CloudFetchOptions) async throws {
        let prepared = try await prepareFetch(options: options)
        let operation = try prepared.enqueue(validate: {})
        try await operation.drain()
    }
    func cancelOperations() async {
        #if DEBUG
        DebugCloudDiagnostics.nativeFetchSequence.record(.cancel, ordinal: debugFetchOrdinal)
        #endif
        let original = sdk
        let operations = networkLifetime.beginCancellation()
        defer { networkLifetime.endCancellation() }
        await original?.cancelOperations()
        for operation in operations { await operation.cancelAndDrain() }
        if let returnedWork { await returnedWork.value }
    }
    func release() async {
        #if DEBUG
        DebugCloudDiagnostics.nativeFetchSequence.record(.release, ordinal: debugFetchOrdinal)
        #endif
        networkLifetime.retire()
        await cancelOperations()
        sdk = nil
        savedRemoval = NativeCloudSavedRemoval()
    }
    private func fields(_ record: CKRecord) throws -> Data {
        try NativeCloudKitSystemFields.encode(record)
    }
    private func decode(_ record: CKRecord) throws -> CloudRecord {
        try NativeCloudRecordConversion.decode(record)
    }
    private func encode(_ record: CloudRecord, zone: String) throws -> CKRecord {
        try NativeCloudRecordConversion.encode(record, zone: zone)
    }
    private func code(_ error: any Error) -> CloudEngineErrorCode {
        NativeCloudKitSendError.code(error)
    }
    #if DEBUG
    private func debugFetchScope(_ options: CKSyncEngine.FetchChangesOptions) -> DebugCloudDiagnostics.FetchInclusion {
        .init(root: options.scope.contains(zone("meguri-root")),
              collection: collectionZone.map { options.scope.contains(zone($0)) } ?? false)
    }
    private func debugFetchPriority(_ options: CKSyncEngine.FetchChangesOptions) -> DebugCloudDiagnostics.FetchInclusion {
        .init(root: options.prioritizedZoneIDs.contains(zone("meguri-root")),
              collection: collectionZone.map { options.prioritizedZoneIDs.contains(zone($0)) } ?? false)
    }
    // Caller admits the original SDK first. Read public state only; retain no IDs.
    private func debugRecordUnfetchedServerChanges(_ syncEngine: CKSyncEngine) {
        let unfetched = syncEngine.state.zoneIDsWithUnfetchedServerChanges
        let registered = collectionZone.map { zone($0) }
        DebugCloudDiagnostics.fetchRecorder.recordServerChangeObservations(
            unfetched: !unfetched.isEmpty,
            unfetchedCollection: registered.map { unfetched.contains($0) } ?? false)
    }
    #endif
    func handleEvent(_ event: CKSyncEngine.Event, syncEngine: CKSyncEngine) async {
        guard sdk === syncEngine else { return }
        #if DEBUG
        // Process-wide original callback facts, after SDK admission and before
        // conversion can throw. No current-owner or convergence authority.
        switch event {
        case .willFetchChanges, .fetchedDatabaseChanges, .stateUpdate, .didFetchChanges:
            debugRecordUnfetchedServerChanges(syncEngine)
        default: break
        }
        switch event {
        case .willFetchChanges(let will):
            DebugCloudDiagnostics.nativeFetchSequence.record(.will(
                reason: will.context.reason == .manual ? .manual : .scheduled,
                sameGroup: debugFetchGroup === will.context.options.operationGroup), ordinal: debugFetchOrdinal)
            DebugCloudDiagnostics.fetchRecorder.record(.will)
        case .didFetchChanges(let did):
            DebugCloudDiagnostics.nativeFetchSequence.record(.did(
                reason: did.context.reason == .manual ? .manual : .scheduled,
                sameGroup: debugFetchGroup === did.context.options.operationGroup), ordinal: debugFetchOrdinal)
        case .fetchedDatabaseChanges(let changes):
            DebugCloudDiagnostics.nativeFetchSequence.record(.database, ordinal: debugFetchOrdinal)
            let registered = collectionZone.map { zone($0) }
            DebugCloudDiagnostics.fetchRecorder.recordServerChangeObservations(
                database: true,
                databaseCollection: registered.map { id in changes.modifications.contains { $0.zoneID == id } } ?? false)
        case .willFetchRecordZoneChanges(let will):
            DebugCloudDiagnostics.nativeFetchSequence.record(.willZone(
                NativeCloudKitIdentityValidation.debugFetchZone(will.zoneID, collectionZone: collectionZone)),
                ordinal: debugFetchOrdinal)
            if NativeCloudKitIdentityValidation.debugFetchZone(will.zoneID, collectionZone: collectionZone) == .collection {
                DebugCloudDiagnostics.fetchRecorder.record(.zoneC)
            }
        case .fetchedRecordZoneChanges(let fetched):
            var root = false, collection = false, other = false
            for id in fetched.modifications.map({ $0.record.recordID.zoneID }) + fetched.deletions.map({ $0.recordID.zoneID }) {
                switch NativeCloudKitIdentityValidation.debugFetchZone(id, collectionZone: collectionZone) {
                case .root: root = true
                case .collection: collection = true
                case .other: other = true
                }
            }
            DebugCloudDiagnostics.nativeFetchSequence.record(.fetchedZones(root: root, collection: collection, other: other),
                ordinal: debugFetchOrdinal)
            for modification in fetched.modifications {
                switch NativeCloudKitIdentityValidation.debugFetchZone(modification.record.recordID.zoneID, collectionZone: collectionZone) {
                case .collection: DebugCloudDiagnostics.fetchRecorder.record(.modC)
                case .root: DebugCloudDiagnostics.fetchRecorder.record(.modR)
                case .other: break
                }
            }
        case .didFetchRecordZoneChanges(let did):
            if did.error != nil { DebugCloudDiagnostics.fetchRecorder.record(.zoneFail) }
        case .sentRecordZoneChanges(let sent):
            let registered = collectionZone.map { zone($0) }
            for record in sent.savedRecords {
                NativeCloudKitIdentityValidation.debugRecordSentRecord(record.recordID,
                    collectionZoneID: registered, saved: true, recorder: DebugCloudDiagnostics.fetchRecorder)
            }
            for failure in sent.failedRecordSaves {
                NativeCloudKitIdentityValidation.debugRecordSentRecord(failure.record.recordID,
                    collectionZoneID: registered, saved: false, recorder: DebugCloudDiagnostics.fetchRecorder)
            }
            for id in sent.failedRecordDeletes.keys {
                NativeCloudKitIdentityValidation.debugRecordSentRecord(id,
                    collectionZoneID: registered, saved: false, recorder: DebugCloudDiagnostics.fetchRecorder)
            }
        default: break
        }
        #endif
        if activeEvent { await withCheckedContinuation { eventWaiters.append($0) } }
        else { activeEvent = true; networkLifetime.beginEvent() }
        if let returnedWork { await returnedWork.value }
        // The signed observer holds this turn through the delegate's state
        // read and append. Returned bookkeeping can apply its updates later.
        let observing = observeNative != nil
        if observing { await enterEvidenceTurn() }
        defer { if observing { leaveEvidenceTurn() } }
        if sdk === syncEngine {
            let receipts = await takeSavedPendingRemovals()
            if !receipts.isEmpty { await delegate.savedPendingRemovalsConfirmed(receipts, engine: self) }
        }
        // Stage before the delivery gate can suspend. Keep all native leases
        // through original callback completion, including any suspended wait.
        let staged = Result<NativeCloudFetchedPayloads?, any Error> {
            if case .fetchedRecordZoneChanges(let fetched) = event {
                return try NativeCloudFetchedPayloads(records: fetched.modifications.map(\.record),
                    deleted: fetched.deletions.map(\.recordID), staging: fetchedPayloadStaging())
            }
            return nil
        }
        defer { withExtendedLifetime(staged) {} }
        await delegate.eventWillDeliver(engine: self)
        do {
            let fetchedPayloads = try staged.get()
            if failure != nil { throw NativeCloudKitFailure.unsupportedRecord }
            let forwarded: CloudEngineEvent
            switch event {
            case .stateUpdate(let update): forwarded = .stateUpdate(try JSONEncoder().encode(update.stateSerialization))
            case .accountChange(let change):
                let kind: CloudAccountChange
                switch change.changeType { case .signIn: kind = .signIn; case .signOut: kind = .signOut; case .switchAccounts: kind = .switchAccounts; @unknown default: throw NativeCloudKitFailure.unsupportedRecord }
                forwarded = .accountChanged(change: kind, identity: nil)
            case .fetchedRecordZoneChanges:
                guard let fetchedPayloads else { throw NativeCloudKitFailure.unsupportedRecord }
                let events = fetchedPayloads.events
                for event in events.dropLast() { await delegate.handleEvent(event, engine: self) }
                // Empty batches own a callback too; no zone or drain is inferred.
                forwarded = events.last ?? .databaseChanges
            case .sentRecordZoneChanges(let sent):
                guard let collectionZone else { throw NativeCloudKitFailure.unsupportedRecord }
                let batch = try NativeCloudKitRecordRouting.sentEngineBatchID(saved: sent.savedRecords.map(\.recordID), failed: sent.failedRecordSaves.map { $0.record.recordID } + Array(sent.failedRecordDeletes.keys), deleted: sent.deletedRecordIDs, collectionZoneID: zone(collectionZone), batches: batches, pendingBatch: pendingBatch)
                let saved = try sent.savedRecords.map(decode)
                var failed: [CloudRecordFailure] = []
                for item in sent.failedRecordSaves {
                    // K9.3 immutable-blob conflicts count only from the real
                    // SDK sent callback and its actual server record. Rust
                    // still checks the original Work and exact blob metadata.
                    if let blob = try NativeCloudKitSendError.savedBlobConflict(record: item.record, error: item.error) {
                        failed.append(CloudRecordFailure(name: blob.name, code: .serverRecordChanged,
                            zone: "meguri-media", nativeCode: Int64(item.error.code.rawValue),
                            retryAfterMilliseconds: NativeCloudOwnRecords.retryDuration(item.error).flatMap { Int64(exactly: $0) },
                            serverRecord: blob))
                    } else {
                        failed.append(try NativeCloudKitSendError.failure(record: item.record, error: item.error, collectionZoneID: zone(collectionZone)))
                    }
                }
                let failedDeletes = try sent.failedRecordDeletes.map { id, error in
                    guard id.zoneID == NativeCloudOwnRecords.zone("meguri-media") else { throw NativeCloudKitFailure.unsupportedRecord }
                    _ = try NativeCloudKitIdentityValidation.zoneName(id.zoneID)
                    return CloudRecordFailure(name: id.recordName, code: NativeCloudKitSendError.code(error),
                        zone: id.zoneID.zoneName, nativeCode: Int64(error.code.rawValue),
                        retryAfterMilliseconds: NativeCloudOwnRecords.retryDuration(error).flatMap { Int64(exactly: $0) })
                }
                savedRemoval.sent(batch, names: Set(saved.map(\.name) + sent.deletedRecordIDs.map(\.recordName)))
                forwarded = .sent(batchID: batch, saved: saved, failed: failed, deleted: sent.deletedRecordIDs.map(\.recordName), failedDeletes: failedDeletes)
            case .willFetchChanges(let will): forwarded = .willFetch(groupID: will.context.options.operationGroup.name ?? "", reason: will.context.reason == .manual ? .manual : .scheduled)
            case .willFetchRecordZoneChanges(let will):
                forwarded = .willFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(will.zoneID))
            case .didFetchRecordZoneChanges(let did):
                forwarded = .didFetchZone(zone: try NativeCloudKitIdentityValidation.zoneName(did.zoneID), error: did.error.map(code))
            case .didFetchChanges(let did): forwarded = .didFetch(groupID: did.context.options.operationGroup.name ?? "", reason: did.context.reason == .manual ? .manual : .scheduled)
            case .willSendChanges(let event):
                if let observeNative, event.context.reason == .scheduled {
                    let group = event.context.options.operationGroup.operationGroupID
                    if scheduledSends[group] == nil {
                        scheduledSends[group] = .init(identity: UUID(), reason: .scheduled)
                    } else { await observeNative(.unbound) }
                }
                forwarded = .willSend(zone: collectionZone ?? "")
            case .didSendChanges(let event):
                if observeNative != nil, event.context.reason == .scheduled {
                    scheduledSends.removeValue(forKey: event.context.options.operationGroup.operationGroupID)
                }
                forwarded = .didSend(zone: collectionZone ?? "")
            case .fetchedDatabaseChanges(let changes):
                forwarded = try NativeCloudKitIdentityValidation.databaseDeletionEvent(
                    modified: changes.modifications.map(\.zoneID), losses: try changes.deletions.map { deletion in
                        let cause: CloudZoneLossCause
                        switch deletion.reason {
                        case .deleted: cause = .deleted
                        case .purged: cause = .purged
                        case .encryptedDataReset: cause = .encryptedDataReset
                        @unknown default: throw NativeCloudKitFailure.unsupportedRecord
                        }
                        return (deletion.zoneID, cause)
                    }, collectionZone: collectionZone)
                #if DEBUG
                if case .databaseChanges = forwarded, !changes.deletions.isEmpty {
                    DebugCloudDiagnostics.nativeFetchSequence.record(.unrelatedDatabaseDeletion, ordinal: debugFetchOrdinal)
                }
                #endif
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
            #if DEBUG
            if case .fetchedRecordZoneChanges = event { DebugCloudDiagnostics.fetchRecorder.record(.refused) }
            #endif
            failure = error
            // A native decoding failure is terminal; it cannot be followed by
            // a successful opaque-state persistence through this instance.
            await delegate.handleEvent(.deliveryFailed, engine: self)
            returnedWork = Task { await self.returned(.deliveryFailed) }
        }
    }
    private func returned(_ event: CloudEngineEvent) async {
        await delegate.eventHandlerReturned(event, engine: self)
        if eventWaiters.isEmpty {
            activeEvent = false; networkLifetime.endEvent()
            let pending = networkEventWaiters; networkEventWaiters.removeAll()
            for waiter in pending.values { waiter.resume() }
        } else { eventWaiters.removeFirst().resume() }
        await delegate.eventBookkeepingCompleted(event, engine: self)
    }
    func nextRecordZoneChangeBatch(_ context: CKSyncEngine.SendChangesContext, syncEngine: CKSyncEngine) async -> CKSyncEngine.RecordZoneChangeBatch? {
        guard failure == nil, sdk === syncEngine, let collectionZone else { return nil }
        if let returnedWork { await returnedWork.value }
        let pending = pendingRecordZoneChanges()
        let removalIssue = savedRemoval.capture(pending: pending, generation: networkLifetime.authority())
        guard failure == nil else { return nil }
        // Scope each request to one zone, retaining collection-before-media
        // order whenever the original SDK request permits both zones.
        let candidates = [collectionZone, "meguri-media"].filter { name in
            switch context.options.scope {
            case .zoneIDs(let ids): ids.contains(zone(name))
            case .recordIDs(let ids): ids.contains { $0.zoneID == zone(name) }
            case .allExcluding(let ids): !ids.contains(zone(name))
            case .all: true
            @unknown default: false
            }
        }
        var selected: (String, CloudEngineBatch)?
        for candidate in candidates {
            var scoped = pending.filter { NativeCloudMediaBatch.zone(name: $0.key, collection: collectionZone) == candidate }
            if case .recordIDs(let ids) = context.options.scope {
                let names = Set(ids.filter { $0.zoneID == zone(candidate) }.map(\.recordName))
                scoped = scoped.filter { names.contains($0.key) }
            }
            if let batch = await delegate.nextRecordZoneChangeBatch(pending: scoped, zone: candidate, engine: self) { selected = (candidate, batch); break }
        }
        guard let (batchZone, batch) = selected else { return nil }
        if let observeNative {
            let group = context.options.operationGroup.operationGroupID
            let operation: CloudNativeSendOperation? = switch context.reason {
            case .manual: manualSends.count == 1 ? manualSends[group] : nil
            case .scheduled: scheduledSends[group]
            @unknown default: nil
            }
            if let operation {
                await observeNative(.built(origin: instanceID, operation: operation, zone: batchZone, batch: batch))
            } else { await observeNative(.unbound) }
            guard sdk === syncEngine else { return nil }
        }
        guard failure == nil else { return nil }
        do {
            try NativeCloudMediaBatch.validate(batch, zone: batchZone)
            let saves = try batch.saves.map { try encode($0, zone: batchZone) }
            savedRemoval.handed(batch.id, names: Set(batch.saves.map(\.name) + batch.deletes), issue: removalIssue)
            pendingBatch = batch.id
            for save in saves { batches[save.recordID.recordName] = batch.id }
            for name in batch.deletes { batches[name] = batch.id }
            let prepared = CKSyncEngine.RecordZoneChangeBatch(recordsToSave: saves, recordIDsToDelete: batch.deletes.map { recordID($0, zone: batchZone) }, atomicByZone: false)
            if batchZone == "meguri-media" {
                guard let admission = batch.mediaAdmission else { throw NativeCloudKitFailure.unsupportedRecord }
                let lifetime = networkLifetime
                let generation = lifetime.authority()
                do {
                    return try await admission.deliver(PreparedNativeMediaBatch(prepared)) { deliver in
                        var delivered = false
                        try lifetime.enqueue(generation: generation, validate: {}) { delivered = deliver() }
                        return delivered
                    }?.value
                } catch {
                    return nil
                }
            }
            return prepared
        } catch {
            failure = error
            let observing = observeNative != nil
            if observing { await enterEvidenceTurn() }
            defer { if observing { leaveEvidenceTurn() } }
            await delegate.eventWillDeliver(engine: self)
            await delegate.handleEvent(.deliveryFailed, engine: self)
            returnedWork = Task { await self.returned(.deliveryFailed) }
            return nil
        }
    }
    func nextFetchChangesOptions(_ context: CKSyncEngine.FetchChangesContext, syncEngine: CKSyncEngine) async -> CKSyncEngine.FetchChangesOptions {
        #if DEBUG
        // Keep the first identity check. A later release does not change this
        // callback's process-wide count or grant current-owner authority.
        let originalSDKAtEntry = sdk === syncEngine
        if originalSDKAtEntry { debugRecordUnfetchedServerChanges(syncEngine) }
        let observedCollectionZone = originalSDKAtEntry ? collectionZone.map { zone($0) } : nil
        let requestedCollection = observedCollectionZone.map { context.options.scope.contains($0) }
        let diagnosticOrdinal = debugFetchOrdinal
        let requestedScope = debugFetchScope(context.options)
        let reason: DebugCloudDiagnostics.FetchReason = context.reason == .manual ? .manual : .scheduled
        let sameGroup = debugFetchGroup === context.options.operationGroup
        var offeredScope = DebugCloudDiagnostics.FetchInclusion()
        var offeredPriority = DebugCloudDiagnostics.FetchInclusion()
        // All existing early/error returns offer the empty scope.
        var offeredCollection = false
        defer {
            if originalSDKAtEntry {
                DebugCloudDiagnostics.nativeFetchSequence.record(.options(reason: reason, requested: requestedScope,
                    offered: offeredScope, priority: offeredPriority, sameGroup: sameGroup), ordinal: diagnosticOrdinal)
            }
            if let requestedCollection {
                DebugCloudDiagnostics.fetchRecorder.recordFetchOptions(
                    requestedCollection: requestedCollection, offeredCollection: offeredCollection)
            }
        }
        #endif
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
            #if DEBUG
            offeredScope = debugFetchScope(result)
            offeredPriority = debugFetchPriority(result)
            if let observedCollectionZone { offeredCollection = result.scope.contains(observedCollectionZone) }
            #endif
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
