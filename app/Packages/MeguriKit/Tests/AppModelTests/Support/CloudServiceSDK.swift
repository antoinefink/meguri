#if os(macOS)
@testable import CoreBridge
import Foundation
import Testing

nonisolated struct ServiceSentBatch: Sendable {
    let zone: String
    let id: UInt64
}

actor ServiceSDKFactory: CloudKitEngineFactory {
    let wire: ServiceWorkflowWire
    private(set) var engines: [ServiceSDK] = []
    init(wire: ServiceWorkflowWire) { self.wire = wire }
    private var nextAccountRead: (@Sendable () async -> Void)?
    private var observeCreation: (@Sendable (CloudEngineConfiguration) async throws -> Void)?
    func inspectCreation(_ inspect: @escaping @Sendable (CloudEngineConfiguration) async throws -> Void) { observeCreation = inspect }
    private(set) var accountReadCount = 0
    func holdNextAccountRead(_ hold: @escaping @Sendable () async -> Void) { nextAccountRead = hold }
    func resolveAccount() async throws -> CloudResolvedAccount? {
        accountReadCount += 1
        let hold = nextAccountRead; nextAccountRead = nil
        await hold?()
        return .init(status: .available, identity: Data("synthetic-initial-account".utf8))
    }
    func makeEngine(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate) async throws -> any CloudKitEngine {
        try await observeCreation?(configuration)
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
    private let automaticallySync: Bool
    private let collectionZone: String
    private var pendingRevision: [String: UInt64] = [:]
    private var revision: UInt64 = 0
    // This is the SDK's network slot, not the application's policy/input lane.
    private var networkActive = false
    private var networkWaiters: [CheckedContinuation<Void, Never>] = []
    // An addition owns a wake until a pass which observed that ordinal ends.
    // A nil batch must not consume an addition arriving in its terminal callback.
    private var automaticRequested: UInt64?
    private var automaticGeneration: UInt64 = 0
    private var automaticRun: (id: UUID, drain: Task<Void, Never>)?
    private(set) var automaticRuns = 0
    private(set) var batchRequests = 0
    private(set) var emptyBatchRequests = 0
    private(set) var pendingAdditionUpdates = 0
    private nonisolated enum SendResult: Sendable, Equatable { case empty, sent, failed }
    private var pending: [String: CloudPendingChange] = [:]
    private var eventActive = false
    private var eventWaiters: [CheckedContinuation<Void, Never>] = []
    private var bookkeepingWaiters: [CheckedContinuation<Void, Never>] = []
    private var cancelled = false
    private var released = false
    private var nextSentResult: (zone: String?, hold: @Sendable () async -> Void)?
    private var nextSentHandlerReturn: (@Sendable () async -> Void)?
    private var nextEmptyTerminalReturn: (@Sendable () async -> Void)?
    private var nextCancellation: (@Sendable () -> Void)?
    private var nextPendingAddition: (@Sendable () -> Void)?
    func holdNextSentHandlerReturn(_ hold: @escaping @Sendable () async -> Void) { nextSentHandlerReturn = hold }
    func inspectNextCancellation(_ inspect: @escaping @Sendable () -> Void) { nextCancellation = inspect }
    func inspectNextPendingAddition(_ inspect: @escaping @Sendable () -> Void) { nextPendingAddition = inspect }
    func holdNextEmptyTerminalReturn(_ hold: @escaping @Sendable () async -> Void) { nextEmptyTerminalReturn = hold }
    var automaticSchedulingEnabled: Bool { automaticallySync }
    private(set) var deliveredSentBatches: [ServiceSentBatch] = []
    private(set) var heldSentBatchID: UInt64?
    private(set) var heldSentZone: String?
    private(set) var heldSavedNames: [String] = []
    // Holds an actual Service response within the original admitted SDK network
    // call, before its original result handler/ACK. It never replaces a result.
    func holdNextSentResult(zone: String?, _ hold: @escaping @Sendable () async -> Void) {
        nextSentResult = (zone, hold)
    }
    init(configuration: CloudEngineConfiguration, delegate: any CloudKitEngineDelegate, wire: ServiceWorkflowWire) throws {
        instanceID = configuration.instanceID; self.delegate = delegate; self.wire = wire
        automaticallySync = configuration.automaticallySync
        collectionZone = configuration.collectionZone
        serialization = try configuration.serialization.map { try JSONDecoder().decode(Serialization.self, from: $0) } ?? Serialization()
        for (name, value) in serialization.pending {
            guard value == "save" || value == "delete" else { throw ServiceWorkflowFailure.wire }
            pending[name] = value == "save" ? .save : .delete
            pendingRevision[name] = 0
        }
    }
    func pendingRecordZoneChanges() async -> [String: CloudPendingChange] { pending }
    func pendingDatabaseChanges() async -> Set<String> { [] }
    func removePendingDatabaseChanges() async {}
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws {
        revision &+= 1
        for name in update.removes { pending.removeValue(forKey: name); pendingRevision.removeValue(forKey: name) }
        for (name, change) in update.adds { pending[name] = change; pendingRevision[name] = revision }
        if !update.adds.isEmpty {
            pendingAdditionUpdates += 1
            let inspect = nextPendingAddition; nextPendingAddition = nil
            inspect?()
            automaticRequested = revision; scheduleAutomaticSend()
        }
    }
    private func removePending(_ names: [String], issued: [String: UInt64]) {
        for name in names where pendingRevision[name] == issued[name] {
            pending.removeValue(forKey: name); pendingRevision.removeValue(forKey: name)
        }
    }
    private func scheduleAutomaticSend() {
        guard automaticallySync, automaticRequested != nil, automaticRun == nil,
              automaticGeneration == lifetime.authority(),
              !eventActive, !networkActive, !cancelled, !released, delegate != nil else { return }
        let generation = lifetime.authority(), id = UUID()
        do {
            // admit owns a detached task: no SDK-handler task locals reach Core
            // or the wire. Cancellation joins this original task and callbacks.
            let operation = try lifetime.admit(cancelSDK: { await self.cancelSDKCalls() }) {
                try await self.runAutomaticSend(generation: generation)
            }
            let drain = Task.detached { [self] in
                _ = try? await operation.drain()
                await automaticFinished(id)
            }
            automaticRuns += 1
            automaticRun = (id, drain)
        } catch { automaticRequested = nil }
    }
    private func runAutomaticSend(generation: UInt64) async throws {
        // A successful real batch is progress; nil or any failure ends the run.
        // Never poll pending names or retry failed records on a clock.
        while true {
            try Task.checkCancellation()
            let result = try await invokeSend(zones: [collectionZone, "meguri-media"],
                generation: generation, automatic: true, prepare: {}, validate: {})
            guard result == .sent, !pending.isEmpty else { return }
        }
    }
    private func automaticFinished(_ id: UUID) {
        guard automaticRun?.id == id else { return }
        automaticRun = nil
        scheduleAutomaticSend() // only an actual update arriving after the run ended
    }
    // Observation joins the original SDK run; it never starts one.
    func waitForAutomaticSend() async {
        while let original = automaticRun { await original.drain.value }
    }
    private func acquireNetwork() async throws {
        if networkActive { await withCheckedContinuation { networkWaiters.append($0) } }
        else { networkActive = true }
        do { try requireNetwork() }
        catch { releaseNetwork(); throw error }
    }
    private func releaseNetwork() {
        if networkWaiters.isEmpty { networkActive = false }
        else { networkWaiters.removeFirst().resume() }
    }
    private func deliver(_ event: CloudEngineEvent, removing: [String] = [], issued: [String: UInt64] = [:],
                         beforeReturn: (@Sendable () async -> Void)? = nil) async throws {
        guard let delegate else { throw ServiceWorkflowFailure.refused }
        if eventActive { await withCheckedContinuation { eventWaiters.append($0) } }
        else { eventActive = true; lifetime.beginEvent() }
        await delegate.eventWillDeliver(engine: self)
        await delegate.handleEvent(event, engine: self)
        if case .sent = event, let hold = nextSentHandlerReturn {
            nextSentHandlerReturn = nil
            await hold()
        }
        // SDK successful removals happen after the original handler returns.
        removePending(removing, issued: issued)
        await delegate.eventHandlerReturned(event, engine: self)
        await beforeReturn?()
        if eventWaiters.isEmpty {
            eventActive = false; lifetime.endEvent()
            let waiting = bookkeepingWaiters; bookkeepingWaiters.removeAll()
            for waiter in waiting { waiter.resume() }
        } else { eventWaiters.removeFirst().resume() }
        await delegate.eventBookkeepingCompleted(event, engine: self)
        scheduleAutomaticSend()
    }
    func waitForEventBookkeeping() async throws {
        while eventActive { await withCheckedContinuation { bookkeepingWaiters.append($0) } }
        try Task.checkCancellation()
    }
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        guard !released else { throw ServiceWorkflowFailure.refused }
        cancelled = false
        let lifetime = lifetime, generation = lifetime.authority()
        return CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: { await self.cancelSDKCalls() }) {
                _ = try await self.invokeSend(zones: zones, generation: generation, prepare: prepare, validate: validate)
            }
        }
    }
    private func invokeSend(zones: [String], generation: UInt64, automatic: Bool = false, prepare: @Sendable () async throws -> Void,
                            validate: @Sendable () throws -> Void) async throws -> SendResult {
        try await waitForEventBookkeeping()
        try await acquireNetwork()
        let observedRevision = revision
        var outcome: SendResult?
        defer {
            if !cancelled, !released, generation == lifetime.authority() {
                if outcome == .sent, !automatic, !pending.isEmpty {
                    // A manual pass can hand remaining names to automatic
                    // scheduling. An automatic run owns its own progress loop.
                    automaticRequested = max(automaticRequested ?? observedRevision, observedRevision)
                } else if let requested = automaticRequested, requested <= observedRevision {
                    automaticRequested = nil
                }
            }
            releaseNetwork()
            scheduleAutomaticSend()
        }
        repeat { try await waitForEventBookkeeping() } while eventActive
        try await prepare()
        repeat { try await waitForEventBookkeeping() } while eventActive
        try lifetime.enqueue(generation: generation, validate: validate, begin: {})
        automaticGeneration = generation
        let result = try await sendPass(zones: zones)
        outcome = result
        return result
    }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue {
        try await waitForEventBookkeeping()
        guard !released else { throw ServiceWorkflowFailure.refused }
        cancelled = false
        let lifetime = lifetime, generation = lifetime.authority()
        return CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: { await self.cancelSDKCalls() }) {
                try await self.invokeFetch(options: options, generation: generation, prepare: prepare, validate: validate)
            }
        }
    }
    private func invokeFetch(options: CloudFetchOptions, generation: UInt64, prepare: @Sendable () async throws -> Void,
                             validate: @Sendable () throws -> Void) async throws {
        try await waitForEventBookkeeping()
        try await acquireNetwork()
        defer { releaseNetwork(); scheduleAutomaticSend() }
        repeat { try await waitForEventBookkeeping() } while eventActive
        try await prepare()
        repeat { try await waitForEventBookkeeping() } while eventActive
        try lifetime.enqueue(generation: generation, validate: validate, begin: {})
        automaticGeneration = generation
        try await fetchPass(options: options)
    }
    private func requireNetwork() throws {
        guard !eventActive, !cancelled, !released, delegate != nil else { throw ServiceWorkflowFailure.refused }
        try Task.checkCancellation()
    }
    func sendChanges(zones: [String]) async throws {
        let prepared = try await prepareSend(zones: zones)
        try await prepared.enqueue(validate: {}).drain()
    }
    private func sendPass(zones: [String]) async throws -> SendResult {
        try requireNetwork()
        var result = SendResult.empty
        let delegate = try serviceRequired(delegate)
        for zone in zones where zone == collectionZone || zone == "meguri-media" {
            var terminalHold: (@Sendable () async -> Void)?
            try requireNetwork()
            try await deliver(.willSend(zone: zone))
            try requireNetwork()
            batchRequests += 1
            let scoped = pending.filter { NativeCloudMediaBatch.zone(name: $0.key, collection: collectionZone) == zone }
            let issued = pendingRevision
            if let batch = await delegate.nextRecordZoneChangeBatch(pending: scoped, zone: zone, engine: self) {
                try requireNetwork()
                guard !batch.saves.isEmpty || !batch.deletes.isEmpty else { throw ServiceWorkflowFailure.wire }
                if result != .failed { result = .sent }
                if zone == "meguri-media" {
                    guard let admission = batch.mediaAdmission, try await admission.deliver(batch) != nil else {
                        throw ServiceWorkflowFailure.refused
                    }
                }
                let changes = try await batch.saves.asyncServiceInputs(wire: wire) + batch.deletes.map {
                    ServiceJSON.object(["Delete": .object(["name": .string($0)])])
                }
                try requireNetwork()
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
                        // Service's per-record errors are non-retained SDK
                        // outcomes; Core may hand the name back after ACK.
                        removePending([name], issued: issued)
                    } else { throw ServiceWorkflowFailure.wire }
                }
                try await wire.recordReceipts(original, zone: zone)
                if let held = nextSentResult, held.zone == nil || held.zone == zone {
                    nextSentResult = nil
                    heldSentBatchID = batch.id
                    heldSentZone = zone
                    heldSavedNames = saved.map(\.name)
                    await held.hold()
                }
                try await deliver(.sent(batchID: batch.id, saved: saved, failed: failed, deleted: deleted),
                    removing: saved.map(\.name) + deleted, issued: issued)
                deliveredSentBatches.append(.init(zone: zone, id: batch.id))
                if !failed.isEmpty { result = .failed }
            } else {
                emptyBatchRequests += 1
                if zone == zones.last, result == .empty {
                    terminalHold = nextEmptyTerminalReturn; nextEmptyTerminalReturn = nil
                }
            }
            try await deliver(.didSend(zone: zone), beforeReturn: terminalHold)
            if result == .failed { break }
        }
        try await saveSerialization()
        return result
    }
    private func saveSerialization() async throws {
        serialization.pending = pending.mapValues { $0 == .save ? "save" : "delete" }
        try await deliver(.stateUpdate(JSONEncoder().encode(serialization)))
    }
    func fetchChanges(options: CloudFetchOptions) async throws {
        let prepared = try await prepareFetch(options: options)
        try await prepared.enqueue(validate: {}).drain()
    }
    private func fetchPass(options: CloudFetchOptions) async throws {
        try requireNetwork()
        let delegate = try serviceRequired(delegate)
        let scoped = await delegate.nextFetchChangesOptions(options, engine: self)
        guard scoped.groupID == options.groupID else { throw ServiceWorkflowFailure.wire }
        try await deliver(.willFetch(groupID: scoped.groupID, reason: .manual))
        var moreDatabase = true
        while moreDatabase {
            try requireNetwork()
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
                try requireNetwork()
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
    private func cancelSDKCalls() {
        cancelled = true
        let inspect = nextCancellation; nextCancellation = nil
        inspect?()
    }
    func cancelOperations() async {
        let originals = lifetime.beginCancellation()
        defer { lifetime.endCancellation() }
        cancelSDKCalls()
        automaticRequested = nil
        let automatic = automaticRun?.drain
        for operation in originals { await operation.cancelAndDrain() }
        await automatic?.value
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
