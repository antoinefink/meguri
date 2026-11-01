#if os(macOS)
@testable import CoreBridge
import Foundation
import Testing

actor ServiceSDKFactory: CloudKitEngineFactory {
    let wire: ServiceWorkflowWire
    private(set) var engines: [ServiceSDK] = []
    init(wire: ServiceWorkflowWire) { self.wire = wire }
    func resolveAccount() async throws -> CloudResolvedAccount? {
        .init(status: .available, identity: Data("synthetic-initial-account".utf8))
    }
    func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async throws -> any CloudKitEngine {
        let engine = try ServiceSDK(configuration: configuration, delegate: delegate, wire: wire)
        engines.append(engine)
        return engine
    }
}

// Adapted from CoreBridgeTests/CloudKitEngineFake's guarded SDK lifetimes.
// Service, rather than a scripted Saved reply, owns all records and cursors.
actor ServiceSDK: CloudKitEngine {
    private nonisolated struct Serialization: Codable, Sendable {
        var database: Data?
        var zones: [String: Data] = [:]
        var pending: [String: String] = [:]
    }
    nonisolated let instanceID: String
    private var delegate: (any CloudKitEngineDelegate)?
    private let wire: ServiceWorkflowWire
    private let lifetime = CloudKitNetworkLifetime()
    private var serialization: Serialization
    private var pending: [String: CloudPendingChange] = [:]
    private var eventActive = false
    private var eventWaiters: [CheckedContinuation<Void, Never>] = []
    private var bookkeepingWaiters: [CheckedContinuation<Void, Never>] = []
    private var cancelled = false
    private var released = false
    init(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate, wire: ServiceWorkflowWire) throws {
        instanceID = configuration.instanceID; self.delegate = delegate; self.wire = wire
        serialization = try configuration.serialization.map { try JSONDecoder().decode(Serialization.self, from: $0) } ?? Serialization()
        for (name, value) in serialization.pending {
            guard value == "save" || value == "delete" else { throw ServiceWorkflowFailure.wire }
            pending[name] = value == "save" ? .save : .delete
        }
    }
    func pendingRecordZoneChanges() async -> [String: CloudPendingChange] { pending }
    func pendingDatabaseChanges() async -> Set<String> { [] }
    func removePendingDatabaseChanges() async {}
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws {
        for name in update.removes { pending.removeValue(forKey: name) }
        pending.merge(update.adds) { _, next in next }
    }
    private func deliver(_ event: CloudEngineEvent, removing: [String] = []) async throws {
        guard let delegate else { throw ServiceWorkflowFailure.refused }
        if eventActive { await withCheckedContinuation { eventWaiters.append($0) } }
        else { eventActive = true; lifetime.beginEvent() }
        await delegate.eventWillDeliver(engine: self)
        await delegate.handleEvent(event, engine: self)
        // SDK successful removals happen after the original handler returns.
        for name in removing { pending.removeValue(forKey: name) }
        await delegate.eventHandlerReturned(event, engine: self)
        if eventWaiters.isEmpty {
            eventActive = false; lifetime.endEvent()
            let waiting = bookkeepingWaiters; bookkeepingWaiters.removeAll()
            for waiter in waiting { waiter.resume() }
        } else { eventWaiters.removeFirst().resume() }
        await delegate.eventBookkeepingCompleted(event, engine: self)
    }
    func waitForEventBookkeeping() async throws {
        while eventActive { await withCheckedContinuation { bookkeepingWaiters.append($0) } }
        try Task.checkCancellation()
    }
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        cancelled = false
        let lifetime = lifetime, generation = lifetime.authority()
        return CloudKitNetworkEnqueue { validate in
            try lifetime.admit(cancelSDK: { await self.cancelSDKCalls() }) {
                try await self.invokeSend(zones: zones, generation: generation, validate: validate)
            }
        }
    }
    private func invokeSend(zones: [String], generation: UInt64, validate: @Sendable () throws -> Void) async throws {
        try lifetime.enqueue(generation: generation, validate: validate, begin: {})
        try await sendChanges(zones: zones)
    }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        cancelled = false
        let lifetime = lifetime, generation = lifetime.authority()
        return CloudKitNetworkEnqueue { validate in
            try lifetime.admit(cancelSDK: { await self.cancelSDKCalls() }) {
                try await self.invokeFetch(options: options, generation: generation, validate: validate)
            }
        }
    }
    private func invokeFetch(options: CloudFetchOptions, generation: UInt64, validate: @Sendable () throws -> Void) async throws {
        try lifetime.enqueue(generation: generation, validate: validate, begin: {})
        try await fetchChanges(options: options)
    }
    private func requireNetwork() throws {
        guard !eventActive, !cancelled, !released, delegate != nil else { throw ServiceWorkflowFailure.refused }
        try Task.checkCancellation()
    }
    func sendChanges(zones: [String]) async throws {
        try requireNetwork()
        let delegate = try serviceRequired(delegate)
        for zone in zones {
            try await deliver(.willSend(zone: zone))
            if let batch = await delegate.nextRecordZoneChangeBatch(pending: pending, zone: zone, engine: self) {
                if zone == "meguri-media" {
                    guard let admission = batch.mediaAdmission, try await admission.deliver(batch) != nil else {
                        throw ServiceWorkflowFailure.refused
                    }
                }
                let changes = try await batch.saves.asyncServiceInputs(wire: wire) + batch.deletes.map {
                    ServiceJSON.object(["Delete": .object(["name": .string($0)])])
                }
                let original = try await wire.modify(zone: zone, changes: changes, instance: instanceID)
                var saved: [CloudRecord] = [], failed: [CloudRecordFailure] = [], deleted: [String] = []
                var leases: [CloudMediaStagedAsset] = []
                defer { withExtendedLifetime(leases) {} }
                let results = try serviceRequired(try original.ok()["Modified"]?.items)
                guard results.count == changes.count else { throw ServiceWorkflowFailure.wire }
                for mutation in results {
                    let name = try serviceRequired(mutation["name"]?.text)
                    let result = try serviceRequired(mutation["result"])
                    if let raw = result["Ok"]?["Saved"] {
                        let row = try ServiceRecordBytes(raw)
                        let payload = try row.asset("pa")
                        let asset = try payload ?? row.asset("a")
                        let lease = try await wire.retainAsset(asset, payload: payload != nil)
                        if let lease { leases.append(lease) }
                        saved.append(row.record(zone: zone, lease: lease))
                    } else if result["Ok"]?.text == "Deleted" { deleted.append(name) }
                    else if let error = result["Err"] {
                        let code = try Self.errorCode(error)
                        var server: CloudRecord?
                        if let raw = error["server_record"], case .object = raw {
                            let row = try ServiceRecordBytes(raw)
                            let lease = try await wire.retainAsset(row.asset("pa"), payload: true)
                            if let lease { leases.append(lease) }
                            server = row.record(zone: zone, lease: lease)
                        }
                        failed.append(.init(name: name, code: code, zone: zone, serverRecord: server))
                        pending.removeValue(forKey: name)
                    } else { throw ServiceWorkflowFailure.wire }
                }
                try await wire.recordReceipts(original, zone: zone)
                try await deliver(.sent(batchID: batch.id, saved: saved, failed: failed, deleted: deleted), removing: saved.map(\.name) + deleted)
            }
            try await deliver(.didSend(zone: zone))
        }
        try await saveSerialization()
    }
    private func saveSerialization() async throws {
        serialization.pending = pending.mapValues { $0 == .save ? "save" : "delete" }
        try await deliver(.stateUpdate(JSONEncoder().encode(serialization)))
    }
    func fetchChanges(options: CloudFetchOptions) async throws {
        try requireNetwork()
        let delegate = try serviceRequired(delegate)
        let scoped = await delegate.nextFetchChangesOptions(options, engine: self)
        guard scoped.groupID == options.groupID else { throw ServiceWorkflowFailure.wire }
        try await deliver(.willFetch(groupID: scoped.groupID, reason: .manual))
        var moreDatabase = true
        while moreDatabase {
            let original = try await wire.database(cursor: serialization.database, instance: instanceID)
            let page = try serviceRequired(try original.ok()["DatabaseChanges"])
            let changes = try serviceRequired(page["changes"]?.items)
            if !changes.isEmpty { try await deliver(.databaseChanges) }
            for change in changes {
                let zone = try serviceRequired(change["zone"]?.text)
                if let removed = change["removed"], case .null = removed { continue }
                serialization.zones.removeValue(forKey: zone)
            }
            serialization.database = try serviceRequired(page["token"]).encoded()
            moreDatabase = try serviceRequired(page["more"]?.flag)
        }
        for zone in scoped.zones {
            try await deliver(.willFetchZone(zone: zone))
            var more = true, zoneError: CloudEngineErrorCode?
            while more {
                let original = try await wire.fetch(zone: zone, cursor: serialization.zones[zone], instance: instanceID)
                if let error = original.result["Err"] { zoneError = try Self.errorCode(error); break }
                let page = try serviceRequired(try original.ok()["ZoneChanges"])
                var records: [CloudRecord] = [], deleted: [String] = [], leases: [CloudMediaStagedAsset] = []
                defer { withExtendedLifetime(leases) {} }
                for change in try serviceRequired(page["changes"]?.items) {
                    if let raw = change["Saved"] {
                        let row = try ServiceRecordBytes(raw)
                        let lease = try await wire.retainAsset(row.asset("pa"), payload: true)
                        if let lease { leases.append(lease) }
                        records.append(row.record(zone: zone, lease: lease))
                    } else if let name = change["Deleted"]?["name"]?.text { deleted.append(name) }
                    else { throw ServiceWorkflowFailure.wire }
                }
                try await deliver(.fetched(zone: zone, records: records, deleted: deleted))
                serialization.zones[zone] = try serviceRequired(page["token"]).encoded()
                more = try serviceRequired(page["more"]?.flag)
            }
            try await deliver(.didFetchZone(zone: zone, error: zoneError))
        }
        try await saveSerialization()
        try await deliver(.didFetch(groupID: scoped.groupID, reason: .manual))
    }
    private nonisolated static func errorCode(_ error: ServiceJSON) throws -> CloudEngineErrorCode {
        switch error["code"]?.text {
        case "UnknownItem": .unknownItem
        case "ZoneNotFound", "UserDeletedZone": .zoneNotFound
        case "ServerRecordChanged": .serverRecordChanged
        case "LimitExceeded": .limitExceeded
        case "BatchRequestFailed": .batchRequestFailed
        case "InvalidArguments", "QuotaExceeded", "ChangeTokenExpired": .other
        default: throw ServiceWorkflowFailure.wire
        }
    }
    private func cancelSDKCalls() { cancelled = true }
    func cancelOperations() async {
        let originals = lifetime.beginCancellation()
        defer { lifetime.endCancellation() }
        cancelSDKCalls()
        for operation in originals { await operation.cancelAndDrain() }
    }
    func release() async {
        lifetime.retire()
        await cancelOperations()
        released = true; delegate = nil
    }
}
private extension Array where Element == CloudRecord {
    nonisolated func asyncServiceInputs(wire: ServiceWorkflowWire) async throws -> [ServiceJSON] {
        var inputs: [ServiceJSON] = []
        for record in self { inputs.append(try wire.saveInput(record)) }
        return inputs
    }
}
#endif
