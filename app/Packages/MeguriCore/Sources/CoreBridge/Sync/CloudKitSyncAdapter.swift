import Foundation

/// Immutable callback origin. An old delivery never looks up a replacement's id.
private actor CloudSDKDelegate: CloudKitEngineDelegate {
    private weak var adapter: CloudKitSyncAdapter?
    let origin: String
    init(adapter: CloudKitSyncAdapter, origin: String) { self.adapter = adapter; self.origin = origin }
    func eventWillDeliver(engine: any CloudKitEngine) async { await adapter?.eventWillDeliver(origin: origin) }
    func handleEvent(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {
        await adapter?.handleEvent(event, origin: origin)
    }
    func eventHandlerReturned(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {
        await adapter?.eventHandlerReturned(event, origin: origin)
    }
    func nextRecordZoneChangeBatch(pending: [String: CloudPendingChange], zone: String, engine: any CloudKitEngine) async -> CloudEngineBatch? {
        await adapter?.nextBatch(origin: origin, pending: pending, zone: zone)
    }
    func nextFetchChangesOptions(_ options: CloudFetchOptions, engine: any CloudKitEngine) async -> CloudFetchOptions {
        await adapter?.fetchOptions(origin: origin, options: options) ?? .init(zones: [], priorityZones: [], groupID: options.groupID)
    }
}

public actor CloudKitSyncAdapter {
    private final class Instance {
        let engine: any CloudKitEngine
        let zone: String
        var stopping = false
        var authorityRetired = false
        var handlers = 0
        var networks = 0
        var applying = false
        var batches: [UInt64: UInt32] = [:]
        var failed = false
        init(engine: any CloudKitEngine, zone: String) { self.engine = engine; self.zone = zone }
    }
    private let core: CoreCloudWorkflowService
    private let factory: any CloudKitEngineFactory
    private let exchangeDidJoin: @Sendable () async -> Void
    private let eventDidPersist: @Sendable (CloudEngineEvent) async -> Void
    private var owned: [String: Instance] = [:]
    private var current: String?
    private var automatic = false
    private var heldAutomatic: Bool?
    private var requestedRunning = false
    private var intentGeneration: UInt64 = 0
    private var creatingSDK = false
    private var identityReads: Set<UInt64> = []
    private var platformEventsTask: Task<Void, Never>?
    private var observingPlatform = false
    private var lastReachable = false
    private var exchangeRunning = false
    private var exchangeTask: (id: UUID, task: Task<CloudExchangeResult, any Error>)?
    private var stopTasks: [String: Task<Void, any Error>] = [:]
    private var fetchBusy = false
    private var version: UInt64 = 0
    private var waiters: [UUID: CheckedContinuation<Void, any Error>] = [:]
    public private(set) var lastFailure: (any Error)?

    public init(core: CoreCloudWorkflowService, engineFactory: any CloudKitEngineFactory,
                eventDidPersist: @escaping @Sendable (CloudEngineEvent) async -> Void = { _ in },
                exchangeDidJoin: @escaping @Sendable () async -> Void = {}) {
        self.core = core; factory = engineFactory; self.eventDidPersist = eventDidPersist
        self.exchangeDidJoin = exchangeDidJoin
    }
    private func changed() {
        version &+= 1
        let pending = waiters; waiters.removeAll()
        for waiter in pending.values { waiter.resume() }
    }
    private func cancelWait(_ id: UUID) { waiters.removeValue(forKey: id)?.resume(throwing: CancellationError()) }
    private func waitForChange(after old: UInt64) async throws {
        try Task.checkCancellation()
        if version != old { return }
        let id = UUID()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                if Task.isCancelled { continuation.resume(throwing: CancellationError()) }
                else if version != old { continuation.resume() }
                else { waiters[id] = continuation }
            }
        } onCancel: { Task { await self.cancelWait(id) } }
    }
    private func valid(_ origin: String) async -> Bool {
        guard let instance = owned[origin], !instance.stopping, current == origin else { return false }
        do { try await core.validateSDK(origin) }
        catch { return false }
        return current == origin && owned[origin] === instance && !instance.stopping
    }
    public func start(automaticallySync: Bool) async throws {
        requestedRunning = true
        if let origin = current, let instance = owned[origin], !instance.stopping { return }
        intentGeneration &+= 1
        let intent = intentGeneration
        automatic = automaticallySync
        await observePlatformAccounts(intent: intent)
        try checkIntent(intent)
        try await startPrepared(automaticallySync: automaticallySync, readIdentity: true, intent: intent)
    }
    private func checkIntent(_ intent: UInt64) throws {
        try Task.checkCancellation()
        guard requestedRunning, intentGeneration == intent else { throw CancellationError() }
    }
    private func startPrepared(automaticallySync: Bool, readIdentity: Bool, intent: UInt64) async throws {
        try checkIntent(intent)
        for task in stopTasks.values { try await task.value; try checkIntent(intent) }
        guard current == nil else { return }
        if readIdentity { try await refreshPlatformIdentity(intent: intent) }
        try checkIntent(intent)
        try await core.prepareRuntime()
        try checkIntent(intent)
        while creatingSDK {
            let old = version
            try await waitForChange(after: old)
            try checkIntent(intent)
        }
        guard current == nil else { return }
        creatingSDK = true
        defer { creatingSDK = false; changed() }
        let registration = try await core.registerSDK(automatic: automaticallySync)
        let delegate = CloudSDKDelegate(adapter: self, origin: registration.instanceID)
        var created: (any CloudKitEngine)?
        var published = false
        do {
            try checkIntent(intent)
            let engine = try await factory.makeEngine(configuration: .init(instanceID: registration.instanceID, automaticallySync: automaticallySync, serialization: registration.serialization, collectionZone: registration.collectionZone), delegate: delegate)
            created = engine
            try checkIntent(intent)
            owned[registration.instanceID] = Instance(engine: engine, zone: registration.collectionZone)
            current = registration.instanceID
            published = true
            changed()
            let live = await valid(registration.instanceID)
            try checkIntent(intent)
            guard live else {
                throw BridgeError.unavailable(reason: "Cloud registration expired during creation")
            }
            try await consumePendingUpdates()
            try checkIntent(intent)
            if registration.needsRootFetch { try await requestFetch(zones: ["meguri-root", registration.collectionZone]) }
            try checkIntent(intent)
        } catch {
            if published {
                try await stopInstance(registration.instanceID, policyStop: false)
                throw error
            }
            try await core.retireSDK(registration.instanceID, policyStop: false)
            if let created { await created.cancelOperations(); await created.release() }
            try await core.releaseSDK(registration.instanceID)
            throw error
        }
    }
    public func configureAutomatic(_ enabled: Bool) async throws {
        if exchangeRunning { heldAutomatic = enabled; return }
        requestedRunning = true
        intentGeneration &+= 1
        let intent = intentGeneration
        if let origin = current { try await stopInstance(origin, policyStop: false) }
        try checkIntent(intent)
        try await start(automaticallySync: enabled)
    }
    public func stop() async throws {
        requestedRunning = false
        intentGeneration &+= 1
        let stopping = current.flatMap { beginStop($0, policyStop: true) }
        let pendingStops = Array(stopTasks.values)
        let reads = identityReads
        let exchange = exchangeTask?.task
        exchange?.cancel()
        for id in reads { _ = try await finishIdentityRead(id, status: .couldNotDetermine, identity: nil) }
        if let stopping { try await stopping.value }
        for task in pendingStops { try await task.value }
        if let exchange {
            do { _ = try await exchange.value }
            catch is CancellationError { }
            catch { throw error }
        }
    }
    private func beginStop(_ origin: String, policyStop: Bool) -> Task<Void, any Error>? {
        if let task = stopTasks[origin] { return task }
        guard let instance = owned[origin] else { return nil }
        instance.stopping = true
        let task = Task {
            do {
                try await self.drainInstance(origin, policyStop: policyStop)
                self.stopTasks.removeValue(forKey: origin); self.changed()
            } catch {
                self.lastFailure = error
                self.stopTasks.removeValue(forKey: origin); self.changed()
                throw error
            }
        }
        stopTasks[origin] = task
        changed()
        return task
    }
    private func stopInstance(_ origin: String, policyStop: Bool) async throws {
        if let task = beginStop(origin, policyStop: policyStop) { try await task.value }
    }
    private func drainInstance(_ origin: String, policyStop: Bool) async throws {
        guard let instance = owned[origin] else { return }
        try await core.retireSDK(origin, policyStop: policyStop)
        instance.authorityRetired = true; changed()
        while instance.handlers > 0 || instance.applying { let old = version; try await waitForChange(after: old) }
        await instance.engine.cancelOperations()
        while instance.networks > 0 { let old = version; try await waitForChange(after: old) }
        await instance.engine.release()
        try await core.releaseSDK(origin)
        owned.removeValue(forKey: origin)
        if current == origin { current = nil }
        changed()
    }
    public func waitForStopping(instanceID: String) async throws {
        while let instance = owned[instanceID], !instance.authorityRetired { let old = version; try await waitForChange(after: old) }
    }
    public func retainedInstanceIDs() -> Set<String> { Set(owned.keys) }
    public func waitForDeferredUpdates() async throws {
        while owned.values.contains(where: { $0.handlers > 0 || $0.applying }) { let old = version; try await waitForChange(after: old) }
        try await consumePendingUpdates()
    }
    public func waitForIdle() async throws {
        while owned.values.contains(where: { $0.handlers > 0 || $0.networks > 0 || $0.applying || $0.stopping || $0.failed }) || !stopTasks.isEmpty {
            let old = version; try await waitForChange(after: old)
        }
        try await executePendingActions()
    }
    private func observePlatformAccounts(intent: UInt64) async {
        guard !observingPlatform else { return }
        observingPlatform = true
        let stream = await factory.accountChanges()
        guard requestedRunning, intentGeneration == intent else { observingPlatform = false; return }
        guard let stream else { return }
        platformEventsTask = Task { [weak self] in
            for await _ in stream {
                guard !Task.isCancelled, let self else { return }
                do { try await self.platformAccountChanged() }
                catch { await self.recordPlatformFailure(error) }
            }
        }
    }
    private func recordPlatformFailure(_ error: any Error) { lastFailure = error }
    private func readPlatformAndResume(transition: Bool) async throws {
        guard requestedRunning else { return }
        if current == nil { intentGeneration &+= 1 }
        let intent = intentGeneration
        do { try await refreshPlatformIdentity(transition: transition, intent: intent) }
        catch {
            if requestedRunning && intentGeneration == intent { try await executePendingActions() }
            throw error
        }
        try checkIntent(intent)
        try await executePendingActions()
        try checkIntent(intent)
        if current == nil { try await startPrepared(automaticallySync: automatic, readIdentity: false, intent: intent) }
    }
    public func foregroundReturned() async throws { try await readPlatformAndResume(transition: false) }
    public func platformAccountChanged() async throws { try await readPlatformAndResume(transition: true) }
    public func networkChanged(reachable: Bool) async throws {
        let wasReachable = lastReachable
        lastReachable = reachable
        if reachable && !wasReachable { try await readPlatformAndResume(transition: false) }
    }
    public func refreshPlatformIdentity(transition: Bool = false) async throws {
        try await refreshPlatformIdentity(transition: transition, intent: intentGeneration)
    }
    private func finishIdentityRead(_ id: UInt64, status: CloudAccountStatus, identity: Data?) async throws -> Bool {
        defer { identityReads.remove(id) }
        return try await core.completeIdentityRead(id: id, status: status, identity: identity)
    }
    private func cancelIdentityRead(_ id: UInt64) async {
        guard identityReads.contains(id) else { return }
        do { _ = try await finishIdentityRead(id, status: .couldNotDetermine, identity: nil) }
        catch { lastFailure = error }
    }
    private func checkReadIntent(_ intent: UInt64?) throws {
        if let intent { try checkIntent(intent) }
        else { try Task.checkCancellation() }
    }
    private func refreshPlatformIdentity(transition: Bool = false, originSDK: String? = nil, intent: UInt64?) async throws {
        try checkReadIntent(intent)
        let id = try await core.beginIdentityRead(transition: transition, originSDK: originSDK)
        identityReads.insert(id)
        do {
            try checkReadIntent(intent)
            let resolved: CloudResolvedAccount?
            do {
                resolved = try await withTaskCancellationHandler {
                    try await factory.resolveAccount()
                } onCancel: { Task { await self.cancelIdentityRead(id) } }
            } catch {
                _ = try await finishIdentityRead(id, status: .couldNotDetermine, identity: nil)
                try checkReadIntent(intent)
                if originSDK != nil { lastFailure = error; return }
                throw error
            }
            try checkReadIntent(intent)
            let accepted = try await finishIdentityRead(id, status: resolved?.status ?? .couldNotDetermine, identity: resolved?.identity)
            try checkReadIntent(intent)
            guard accepted else {
                throw BridgeError.unavailable(reason: "Cloud account read expired")
            }
        } catch {
            if identityReads.contains(id) { _ = try await finishIdentityRead(id, status: .couldNotDetermine, identity: nil) }
            throw error
        }
    }
    public func accountChanged(status: CloudAccountStatus, identity: Data?) async throws {
        try await core.accountChanged(sdk: nil, status: status, identity: identity)
        try await executePendingActions()
    }
    public func executePendingActions() async throws {
        if let origin = current, !(await valid(origin)) { try await stopInstance(origin, policyStop: false) }
        else { try await consumePendingUpdates() }
    }
    public func consumePendingUpdates() async throws {
        guard let origin = current, let instance = owned[origin], !instance.stopping,
              instance.handlers == 0, !instance.applying else { return }
        instance.applying = true
        defer { instance.applying = false; changed() }
        guard await valid(origin) else { return }
        // One drain owns all lists. Taking each complete list in emission order
        // prevents a later full removal from overtaking a saved-handler add.
        let updates = try await core.takePendingUpdates(sdk: origin)
        for update in updates {
            guard await valid(origin) else { return }
            var applied = update
            let before = await instance.engine.pendingRecordZoneChanges()
            guard await valid(origin) else { return }
            if update.kind == .full {
                applied.removes.formUnion(Set(before.keys).subtracting(update.adds.keys))
                await instance.engine.removePendingDatabaseChanges()
                guard await valid(origin) else { return }
            }
            try await instance.engine.applyPendingUpdate(applied)
            guard await valid(origin) else { return }
            let after = await instance.engine.pendingRecordZoneChanges()
            guard await valid(origin) else { return }
            let added = update.adds.keys.filter { after[$0] == update.adds[$0] }
            try await core.updateApplied(sdk: origin, id: update.id, added: added)
        }
    }
    fileprivate func eventWillDeliver(origin: String) {
        guard let instance = owned[origin] else { return }
        instance.handlers += 1; changed()
    }
    fileprivate func handleEvent(_ event: CloudEngineEvent, origin: String) async {
        guard await valid(origin), let instance = owned[origin] else { return }
        do {
            switch event {
            case .deliveryFailed:
                try await core.trackingFailure(sdk: origin)
                instance.failed = true
            case .databaseChanges: break
            case .stateUpdate(let state): try await core.stateUpdate(sdk: origin, state: state)
            case .fetched(let zone, let records, let deleted): try await core.fetched(sdk: origin, zone: zone, records: records, deleted: deleted)
            case .sent(let id, let saved, let failed, let deleted):
                guard deleted.isEmpty else { throw BridgeError.internal("Cloud deletion acknowledgement is unavailable") }
                try await core.sent(sdk: origin, batchID: id, saved: saved, failed: failed, batchCount: instance.batches[id] ?? UInt32(saved.count + failed.count))
            case .accountChanged(let change, let identity):
                if change == .switchAccounts {
                    try await core.accountChanged(sdk: origin, status: .switchedAccounts, identity: nil)
                } else if identity == nil && change != .signOut {
                    try await refreshPlatformIdentity(transition: true, originSDK: origin, intent: nil)
                } else {
                    try await core.accountChanged(sdk: origin, status: change == .signOut ? .noAccount : .available, identity: identity)
                }
            case .willFetch(let group, let reason): try await core.willFetch(sdk: origin, group: group, manual: reason == .manual)
            case .willFetchZone(let zone): try await core.fetchZone(sdk: origin, zone: zone, will: true, failed: false)
            case .didFetchZone(let zone, let error): try await core.fetchZone(sdk: origin, zone: zone, will: false, failed: error != nil)
            case .didFetch(let group, let reason): try await core.didFetch(sdk: origin, group: group, manual: reason == .manual)
            case .willSend, .didSend: break
            }
            guard await valid(origin) else { return }
            await eventDidPersist(event)
            guard await valid(origin) else { return }
            // No deferred list is applied here. The immutable callback origin
            // survives this suspension and is checked again at commit/return.
        } catch {
            lastFailure = error
            instance.failed = true
        }
    }
    fileprivate func eventHandlerReturned(_ event: CloudEngineEvent, origin: String) async {
        guard let instance = owned[origin] else { return }
        if case .sent = event, await valid(origin), !instance.failed {
            do { try await core.handlerReturned(sdk: origin) }
            catch { lastFailure = error; instance.failed = true }
        }
        instance.handlers = max(0, instance.handlers - 1)
        changed()
        let live = await valid(origin)
        if instance.failed || !live {
            // This owned task starts only after the forwarding handler returned;
            // cancellation may need the SDK call that delivered us to unwind.
            _ = beginStop(origin, policyStop: false)
        } else {
            do { try await consumePendingUpdates() }
            catch { lastFailure = error; instance.failed = true
                _ = beginStop(origin, policyStop: false)
            }
        }
    }
    fileprivate func nextBatch(origin: String, pending: [String: CloudPendingChange], zone: String) async -> CloudEngineBatch? {
        guard await valid(origin), let instance = owned[origin], zone == instance.zone else { return nil }
        do {
            let batch = try await core.buildBatch(sdk: origin, zone: zone, pending: pending)
            guard await valid(origin) else { return nil }
            if let batch { instance.batches[batch.id] = UInt32(batch.saves.count + batch.deletes.count) }
            return batch
        } catch { lastFailure = error; instance.failed = true; _ = beginStop(origin, policyStop: false); return nil }
    }
    fileprivate func fetchOptions(origin: String, options: CloudFetchOptions) async -> CloudFetchOptions {
        guard await valid(origin), let instance = owned[origin] else { return .init(zones: [], priorityZones: [], groupID: options.groupID) }
        let allowed = Set(["meguri-root", instance.zone])
        return .init(zones: options.zones.filter { allowed.contains($0) }, priorityZones: options.priorityZones.filter { allowed.contains($0) }, groupID: options.groupID)
    }
    public func requestSend(zones: [String]) async throws {
        guard let origin = current, let instance = owned[origin], await valid(origin) else { return }
        try await consumePendingUpdates()
        guard await valid(origin), let call = try await core.beginSend(sdk: origin) else { return }
        guard await valid(origin) else { return }
        instance.networks += 1; changed()
        defer { instance.networks -= 1; changed() }
        var sendError: (any Error)?
        do { try await instance.engine.sendChanges(zones: zones.filter { $0 == instance.zone }) }
        catch { sendError = error }
        guard await valid(origin) else { if let sendError { throw sendError }; return }
        try await waitForDeferredUpdates()
        let pending = await instance.engine.pendingRecordZoneChanges()
        guard await valid(origin) else { return }
        try await core.finishSend(sdk: origin, id: call, pending: pending)
        try await consumePendingUpdates()
        if let sendError { throw sendError }
    }
    private func fetch(zones: [String]) async throws -> Bool {
        while fetchBusy { let old = version; try await waitForChange(after: old) }
        guard let origin = current, let instance = owned[origin], await valid(origin) else { return false }
        fetchBusy = true; instance.networks += 1; changed()
        defer { fetchBusy = false; instance.networks -= 1; changed() }
        let scope = zones.filter { $0 == "meguri-root" || $0 == instance.zone }
        for _ in 0..<2 {
            guard await valid(origin) else { return false }
            let id = try await core.beginFetch(sdk: origin, zones: scope)
            guard await valid(origin) else { return false }
            do { try await instance.engine.fetchChanges(options: .init(zones: scope, priorityZones: scope, groupID: "meguri-\(id)")) }
            catch {
                if await valid(origin) { _ = try await core.finishFetch(sdk: origin, id: id) }
                throw error
            }
            guard await valid(origin) else { return false }
            if try await core.finishFetch(sdk: origin, id: id) { return true }
        }
        return false
    }
    public func requestFetch(zones: [String]) async throws { _ = try await fetch(zones: zones) }
    public func syncNow() async throws -> CloudExchangeResult {
        try Task.checkCancellation()
        let owned: (id: UUID, task: Task<CloudExchangeResult, any Error>)
        if let running = exchangeTask { owned = running }
        else {
            let id = UUID()
            let task = Task { try await self.runOwnedExchange() }
            owned = (id, task)
            exchangeTask = owned
            exchangeRunning = true
            changed()
        }
        return try await withTaskCancellationHandler {
            await exchangeDidJoin()
            return try await owned.task.value
        } onCancel: { Task { await self.cancelExchange(id: owned.id) } }
    }
    public func cancelSyncNow() { exchangeTask?.task.cancel() }
    private func cancelExchange(id: UUID) {
        if exchangeTask?.id == id { exchangeTask?.task.cancel() }
    }
    private func runOwnedExchange() async throws -> CloudExchangeResult {
        defer { exchangeTask = nil; exchangeRunning = false; changed() }
        guard let origin = current, let instance = owned[origin], await valid(origin) else { throw BridgeError.internal("Cloud engine is unavailable") }
        let id = try await core.beginExchange(sdk: origin)
        let zones = ["meguri-root", instance.zone]
        var fetched = false
        var operationError: (any Error)?
        do {
            try Task.checkCancellation()
            fetched = try await fetch(zones: zones)
            try Task.checkCancellation()
            try await core.exchangeFetched(sdk: origin, complete: fetched)
            if fetched {
                repeat {
                    try Task.checkCancellation()
                    try await requestSend(zones: [instance.zone])
                } while try await core.sendDue(sdk: origin)
                fetched = try await fetch(zones: zones)
            }
            try Task.checkCancellation()
        } catch { operationError = error }
        let result = try await core.completeExchange(sdk: origin, id: id, fetched: fetched,
                                                     cancelled: operationError is CancellationError,
                                                     failed: operationError != nil && !(operationError is CancellationError))
        exchangeRunning = false
        if let held = heldAutomatic {
            heldAutomatic = nil
            do { try await configureAutomatic(held) }
            catch { if operationError == nil { throw error }; lastFailure = error }
        }
        if let operationError { throw operationError }
        return result
    }
}
