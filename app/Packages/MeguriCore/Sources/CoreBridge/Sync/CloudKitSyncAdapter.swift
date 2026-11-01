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
    func eventBookkeepingCompleted(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {
        await adapter?.eventBookkeepingCompleted(event, origin: origin)
    }
    func nextRecordZoneChangeBatch(pending: [String: CloudPendingChange], zone: String, engine: any CloudKitEngine) async -> CloudEngineBatch? {
        await adapter?.nextBatch(origin: origin, pending: pending, zone: zone)
    }
    func nextFetchChangesOptions(_ options: CloudFetchOptions, engine: any CloudKitEngine) async -> CloudFetchOptions {
        await adapter?.fetchOptions(origin: origin, options: options) ?? .init(zones: [], priorityZones: [], groupID: options.groupID)
    }
}

// Used only after Core accepts the original complete receipt. Keep a normal
// delete absence in diagnostics while allowing the media worker to continue.
struct CloudMediaSendReceipt {
    private struct RecordError: Error { let record: CloudRecordFailure }
    let diagnostic: CloudKitSDKOperationFailure?
    let blocking: CloudKitSDKOperationFailure?
    init(failed: [CloudRecordFailure], failedDeletes: [CloudRecordFailure]) {
        let original = failed + failedDeletes
        let delay = original.compactMap(\.retryAfterMilliseconds).max()
        func failure(_ record: CloudRecordFailure) -> CloudKitSDKOperationFailure {
            .init(underlying: RecordError(record: record), code: record.nativeCode ?? 0,
                  retryAfterMilliseconds: delay)
        }
        diagnostic = original.first { $0.zone == "meguri-media" }.map(failure)
        let errors = failed + failedDeletes.filter { $0.code != .unknownItem }
        blocking = errors.first { $0.zone == "meguri-media" }.map(failure)
    }
}

public actor CloudKitSyncAdapter {
    private final class AssetReadRequest {
        let id: UUID
        let retry: (CloudPolicyCommand, CoreCloudPolicyExecution)?
        let completion: CheckedContinuation<CloudPolicyNativeResult, Never>?
        var cancelled = false
        var drained = false
        var joins: [CheckedContinuation<Void, Never>] = []
        init(id: UUID = UUID(), retry: (CloudPolicyCommand, CoreCloudPolicyExecution)? = nil,
             completion: CheckedContinuation<CloudPolicyNativeResult, Never>? = nil) {
            self.id = id; self.retry = retry; self.completion = completion
        }
    }
    private actor AssetReadActivity {
        var started = false
        var failures: [CloudAssetReadFailureFacts] = []
        func observe(_ event: CloudAssetReadObservation) {
            switch event {
            case .started: started = true
            case .failure(let facts): failures.append(facts)
            }
        }
        func snapshot() -> (Bool, [CloudAssetReadFailureFacts]) { (started, failures) }
    }
    private final class Instance {
        let engine: any CloudKitEngine
        let zone: String
        var assetWorker: CloudAssetReadExecutor?
        var assetQueue: [AssetReadRequest] = []
        var assetDispatch: Task<Void, Never>?
        var assetPass: (AssetReadRequest, Task<CloudPolicyNativeResult, Never>)?
        var assetDrain: Task<Void, Never>?
        var assetFirstReadJoins = 0
        var assetClosed = false
        var stopping = false
        var authorityRetired = false
        var released = false
        var handlers = 0
        var networks = 0
        var applying = false
        var batches: [UInt64: UInt32] = [:]
        var failed = false
        var operationSaved: UInt64 = 0
        var mediaSendFailure: CloudKitSDKOperationFailure?
        init(engine: any CloudKitEngine, zone: String) { self.engine = engine; self.zone = zone }
    }
    private let core: CoreCloudWorkflowService
    private let factory: any CloudKitEngineFactory
    private let assetReadTransport: (any CloudOwnTransport)?
    private let assetReadDidFinish: @Sendable () async -> Void
    private let appOwnsAccountObservation: Bool
    private let accountReadRequested: @Sendable () async -> Void
    private var accountReadPending = false
    private var suspendedDeliveries = 0
    private var suspended = false
    private var suspensionEpoch: UInt64 = 0
    private var suspending = false
    private let exchangeDidJoin: @Sendable () async -> Void
    private let eventDidPersist: @Sendable (CloudEngineEvent) async -> Void
    // Scheduling observation only; supplies no validity or Core outcome.
    private let delegateWorkValidated: @Sendable () async -> Void
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
    private var exchangeOrigin: String?
    private var policyWorkerID: UUID?
    private var policyReservation: UUID?
    private var policyReservationWaiters = 0
    struct PreparedPolicyExchange: Sendable {
        fileprivate let origin: String
        fileprivate let zone: String
    }
    private var quiescing = false
    private var quiesceTask: Task<Void, any Error>?
    // Retain the issued handle and its original capability, just as held
    // configuration retains its execution. Only explicit final stop consumes it.
    private var heldStops: [(command: CloudPolicyCommand, execution: CoreCloudPolicyExecution, policy: CoreCloudPolicyService)] = []
    private var finalStopTask: Task<Void, any Error>?
    private var stopTasks: [String: Task<Void, any Error>] = [:]
    private var fetchBusy = false
    private var policyOperationOutputs: [CloudPolicyOutput] = []
    private var version: UInt64 = 0
    private var waiters: [UUID: CheckedContinuation<Void, any Error>] = [:]
    private var readinessObservers: [UUID: AsyncStream<Void>.Continuation] = [:]
    private var originalMediaDrain: (@Sendable (String) async -> Void)?
    func setOriginalMediaDrain(_ drain: @escaping @Sendable (String) async -> Void) { originalMediaDrain = drain }

    public private(set) var lastFailure: (any Error)?

    public init(core: CoreCloudWorkflowService, engineFactory: any CloudKitEngineFactory,
                eventDidPersist: @escaping @Sendable (CloudEngineEvent) async -> Void = { _ in },
                assetReadTransport: (any CloudOwnTransport)? = nil,
                assetReadDidFinish: @escaping @Sendable () async -> Void = {},
                exchangeDidJoin: @escaping @Sendable () async -> Void = {},
                appOwnsAccountObservation: Bool = false,
                accountReadRequested: @escaping @Sendable () async -> Void = {},
                delegateWorkValidated: @escaping @Sendable () async -> Void = {}) {
        self.core = core; factory = engineFactory; self.eventDidPersist = eventDidPersist
        self.exchangeDidJoin = exchangeDidJoin
        self.assetReadTransport = assetReadTransport; self.assetReadDidFinish = assetReadDidFinish
        self.appOwnsAccountObservation = appOwnsAccountObservation
        self.accountReadRequested = accountReadRequested
        self.delegateWorkValidated = delegateWorkValidated
    }
    private func changed() {
        version &+= 1
        for observer in readinessObservers.values { observer.yield(()) }
        let pending = waiters; waiters.removeAll()
        for waiter in pending.values { waiter.resume() }
    }
    func policySDKReady() -> Bool {
        guard !quiescing, !creatingSDK, let current, let instance = owned[current] else { return false }
        return !instance.stopping
    }
    func readinessChanges() -> AsyncStream<Void> {
        let id = UUID()
        return AsyncStream(bufferingPolicy: .bufferingNewest(1)) { continuation in
            readinessObservers[id] = continuation
            continuation.onTermination = { [weak self] _ in Task { await self?.removeReadinessObserver(id) } }
        }
    }
    private func removeReadinessObserver(_ id: UUID) { readinessObservers.removeValue(forKey: id) }
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
        guard !quiescing else { throw CancellationError() }
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
        guard !quiescing, requestedRunning, intentGeneration == intent else { throw CancellationError() }
    }
    private func startPrepared(automaticallySync: Bool, readIdentity: Bool, intent: UInt64, execution: CoreCloudPolicyExecution? = nil) async throws {
        try checkIntent(intent)
        for task in stopTasks.values { try await task.value; try checkIntent(intent) }
        guard current == nil else {
            if execution != nil { throw BridgeError.cloudPolicy(.staleCommand) }; return
        }
        if readIdentity && !appOwnsAccountObservation { try await refreshPlatformIdentity(intent: intent) }
        try checkIntent(intent)
        try await core.prepareRuntime()
        try checkIntent(intent)
        while creatingSDK {
            let old = version
            try await waitForChange(after: old)
            try checkIntent(intent)
        }
        guard current == nil else {
            if execution != nil { throw BridgeError.cloudPolicy(.staleCommand) }; return
        }
        creatingSDK = true
        defer { creatingSDK = false; changed() }
        let registration: CloudRegistration
        if let execution { registration = try await execution.registerSDK() }
        else { registration = try await core.registerSDK(automatic: automaticallySync) }
        let delegate = CloudSDKDelegate(adapter: self, origin: registration.instanceID)
        var created: (any CloudKitEngine)?
        var published = false
        do {
            try checkIntent(intent)
            let engine = try await factory.makeEngine(configuration: .init(instanceID: registration.instanceID, automaticallySync: automaticallySync, serialization: registration.serialization, collectionZone: registration.collectionZone), delegate: delegate)
            created = engine
            try checkIntent(intent)
            if let execution { try await execution.validate(); try checkIntent(intent) }
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
            if registration.needsRootFetch {
                if let execution {
                    _ = try await fetch(zones: ["meguri-root", registration.collectionZone], origin: registration.instanceID, execution: execution)
                } else {
                    try await requestFetch(zones: ["meguri-root", registration.collectionZone])
                }
            }
            try checkIntent(intent)
        } catch {
            let operationError = error
            if published {
                do { try await stopInstance(registration.instanceID, policyStop: false) }
                catch { lastFailure = error }
                throw operationError
            }
            // Close/replacement can already have removed Runtime authority.
            // Native callback/staging ownership must still drain and release.
            do { try await core.retireSDK(registration.instanceID, policyStop: false) }
            catch { lastFailure = error }
            if let created { await created.cancelOperations(); await created.release() }
            do { try await core.releaseSDK(registration.instanceID) }
            catch { lastFailure = error }
            throw operationError
        }
    }
    public func configureAutomatic(_ enabled: Bool) async throws {
        guard !quiescing else { throw CancellationError() }
        if exchangeRunning { heldAutomatic = enabled; return }
        requestedRunning = true
        intentGeneration &+= 1
        let intent = intentGeneration
        if let origin = current { try await stopInstance(origin, policyStop: false) }
        try checkIntent(intent)
        try await start(automaticallySync: enabled)
    }
    /// One-way admission barrier for owner shutdown. Keep callback validation,
    /// pending bookkeeping and the original SDK authority until final stop.
    public func quiesce() async throws {
        if let quiesceTask { try await quiesceTask.value; return }
        quiescing = true
        requestedRunning = false
        intentGeneration &+= 1
        platformEventsTask?.cancel()
        let platform = platformEventsTask
        let originals = Array(owned)
        if finalStopTask == nil {
            for (_, instance) in originals where !instance.authorityRetired { instance.stopping = false }
        }
        let exchange = exchangeTask?.task
        exchange?.cancel()
        // As in final stop, wake a delivery held by a prior suspension so
        // cancelOperations can join it under the original Core validation.
        suspended = false; suspending = false
        changed()
        let task = Task {
            let policyCancellation = Task { await self.cancelPolicyExchange() }
            // Signal cancellation and original asset/media drains together:
            // an SDK operation can itself be joining that bookkeeping.
            let cancellations = originals.map { _, instance in
                let engine = instance.engine
                return Task { await engine.cancelOperations() }
            }
            for (origin, _) in originals {
                await self.drainOriginalAssetReads(origin)
                await self.originalMediaDrain?(origin)
            }
            await policyCancellation.value
            for cancellation in cancellations { await cancellation.value }
            await platform?.value
            self.platformEventsTask = nil; self.observingPlatform = false
            // Exchange errors belong to its original command result, which
            // the coordinator joins and acknowledges before final close.
            if let exchange { _ = await exchange.result }
            while originals.contains(where: { $0.value.handlers > 0 || $0.value.networks > 0 || $0.value.applying }) {
                let old = self.version; try await self.waitForChange(after: old)
            }
        }
        quiesceTask = task
        do { try await task.value }
        catch { quiesceTask = nil; throw error }
    }
    public func stop() async throws {
        if let finalStopTask { try await finalStopTask.value; return }
        let task = Task { try await self.finishStop() }
        finalStopTask = task
        defer { finalStopTask = nil }
        try await task.value
    }
    private func finishStop() async throws {
        if let quiesceTask { try await quiesceTask.value }
        requestedRunning = false
        platformEventsTask?.cancel()
        await platformEventsTask?.value
        platformEventsTask = nil; observingPlatform = false
        intentGeneration &+= 1
        if quiescing {
            for task in Array(stopTasks.values) { try await task.value }
        }
        // The coordinator has joined command results and required control
        // effects before entering here. Never wait for this from StopEngine:
        // that command itself belongs to the coordinator's drain.
        while let held = heldStops.first {
            if let original = try await held.execution.originalSDK(), owned[original] != nil {
                try await stopInstance(original, policyStop: false, execution: held.execution, final: true)
            } else { try await held.execution.retireOriginal() }
            policyOperationOutputs.append(try await held.policy.input(.refresh(.cloud)))
            heldStops.removeFirst()
        }
        let stopping = current.flatMap { beginStop($0, policyStop: true, final: true) }
        suspended = false; suspending = false; changed()
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
    private func beginStop(_ origin: String, policyStop: Bool, execution: CoreCloudPolicyExecution? = nil, final: Bool = false) -> Task<Void, any Error>? {
        // Callback/creation error paths must preserve original authority during
        // the same required-effect drain as the issued StopEngine command.
        guard !quiescing || final else { return nil }
        if let task = stopTasks[origin] { return task }
        guard let instance = owned[origin] else { return nil }
        instance.stopping = true
        let task = Task {
            do {
                try await self.drainInstance(origin, policyStop: policyStop, execution: execution, final: final)
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
    private func stopInstance(_ origin: String, policyStop: Bool, execution: CoreCloudPolicyExecution? = nil, final: Bool = false) async throws {
        if let task = beginStop(origin, policyStop: policyStop, execution: execution, final: final) { try await task.value }
    }
    private func drainInstance(_ origin: String, policyStop: Bool, execution: CoreCloudPolicyExecution?, final: Bool) async throws {
        guard let instance = owned[origin] else { return }
        await drainOriginalAssetReads(origin)
        await originalMediaDrain?(origin)
        guard !quiescing || final else { instance.stopping = false; changed(); return }
        var retirementError: (any Error)?
        if !instance.authorityRetired {
            do {
                if let execution { try await execution.retireOriginal() }
                else { try await core.retireSDK(origin, policyStop: policyStop) }
            } catch {
                if quiescing { throw error }
                retirementError = error
            }
            instance.authorityRetired = true; changed()
        }
        while instance.handlers > 0 || instance.applying { let old = version; try await waitForChange(after: old) }
        await instance.engine.cancelOperations()
        while instance.networks > 0 { let old = version; try await waitForChange(after: old) }
        if !instance.released { await instance.engine.release(); instance.released = true }
        do { try await core.releaseSDK(origin) }
        catch {
            if quiescing { throw error }
            if retirementError == nil { retirementError = error }
        }
        owned.removeValue(forKey: origin)
        if current == origin { current = nil }
        changed()
        if let retirementError { throw retirementError }
    }
    public func waitForStopping(instanceID: String) async throws {
        while let instance = owned[instanceID], !instance.authorityRetired { let old = version; try await waitForChange(after: old) }
    }
    public func retainedInstanceIDs() -> Set<String> { Set(owned.keys) }
    public func waitForDeferredUpdates() async throws {
        let origin = current
        while owned.values.contains(where: { $0.handlers > 0 || $0.applying }) { let old = version; try await waitForChange(after: old) }
        if let origin { try await consumePendingUpdates(origin: origin) }
    }
    private func waitForDeferredUpdates(origin: String) async throws {
        while let instance = owned[origin], instance.handlers > 0 || instance.applying { let old = version; try await waitForChange(after: old) }
        try await consumePendingUpdates(origin: origin)
    }
    public func waitForIdle() async throws {
        while owned.values.contains(where: { $0.handlers > 0 || $0.networks > 0 || $0.applying || $0.stopping || $0.failed || $0.assetDispatch != nil }) || !stopTasks.isEmpty {
            let old = version; try await waitForChange(after: old)
        }
        try await executePendingActions()
    }
    private func observePlatformAccounts(intent: UInt64) async {
        guard !appOwnsAccountObservation, !observingPlatform else { return }
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
        guard let origin = current else { return }
        try await consumePendingUpdates(origin: origin)
    }
    private func consumePendingUpdates(origin: String) async throws {
        guard let instance = owned[origin], !instance.stopping,
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
    // Hold the SDK's original serial delivery, retaining asset lifetime and
    // order. No event queue grows while Core is suspended.
    public func freezeForSuspend() async throws {
        suspensionEpoch &+= 1
        suspending = true
        for origin in Array(owned.keys) { await drainOriginalAssetReads(origin, permanently: false) }
        while owned.values.contains(where: { $0.handlers > 0 || $0.networks > 0 || $0.applying }) {
            let old = version; try await waitForChange(after: old)
        }
        suspended = true; suspending = false; changed()
    }
    public func takeAccountReadRequest() -> Bool {
        let requested = accountReadPending; accountReadPending = false; return requested
    }
    @discardableResult public func resumeDeliveries() async -> Bool {
        await resumeDeliveries(ifPermitted: { true })
    }
    func resumeDeliveries(ifPermitted: @Sendable () -> Bool) async -> Bool {
        // Local resume rebinds an unverified owner. Only Core's guarded SDK
        // validation after actual identity completion can open this gate.
        let intent = intentGeneration
        let epoch = suspensionEpoch
        if let origin = current {
            guard await valid(origin), intent == intentGeneration else { return false }
        } else {
            guard owned.isEmpty else { return false }
        }
        guard !quiescing, !suspending, epoch == suspensionEpoch, ifPermitted(), !Task.isCancelled else { return false }
        for instance in owned.values where !instance.stopping {
            instance.assetClosed = false; instance.assetDrain = nil
        }
        suspended = false; suspending = false; changed()
        return true
    }
    func waitForSuspendedDelivery() async throws {
        while suspendedDeliveries == 0 { let old = version; try await waitForChange(after: old) }
    }
    fileprivate func eventWillDeliver(origin: String) async {
        let held = suspended
        if held { suspendedDeliveries += 1; changed() }
        defer { if held { suspendedDeliveries -= 1; changed() } }
        while suspended {
            guard let instance = owned[origin], !instance.stopping else { return }
            let old = version
            do { try await waitForChange(after: old) } catch { return }
        }
        guard let instance = owned[origin] else { return }
        instance.handlers += 1; changed()
    }
    fileprivate func handleEvent(_ event: CloudEngineEvent, origin: String) async {
        guard !suspended, await valid(origin), let instance = owned[origin] else { return }
        do {
            switch event {
            case .deliveryFailed:
                try await core.trackingFailure(sdk: origin)
                instance.failed = true
            case .databaseChanges: break
            case .stateUpdate(let state): try await core.stateUpdate(sdk: origin, state: state)
            case .fetched(let zone, let records, let deleted): try await core.fetched(sdk: origin, zone: zone, records: records, deleted: deleted)
            case .sent(let id, let saved, let failed, let deleted, let failedDeletes):
                let actualSaved = try await core.sent(sdk: origin, batchID: id, saved: saved, failed: failed, deleted: deleted, failedDeletes: failedDeletes, batchCount: instance.batches[id] ?? UInt32(saved.count + failed.count + deleted.count + failedDeletes.count))
                instance.operationSaved += actualSaved
                let receipt = CloudMediaSendReceipt(failed: failed, failedDeletes: failedDeletes)
                if let diagnostic = receipt.diagnostic { lastFailure = diagnostic }
                // A prior blocking result in this same SDK operation remains
                // blocking even when a later callback reports normal absence.
                if let failure = receipt.blocking { instance.mediaSendFailure = failure }
            case .accountChanged(let change, let identity):
                if change == .switchAccounts {
                    try await core.accountChanged(sdk: origin, status: .switchedAccounts, identity: nil)
                } else if identity == nil && change != .signOut {
                    if appOwnsAccountObservation { accountReadPending = true; await accountReadRequested() }
                    else { try await refreshPlatformIdentity(transition: true, originSDK: origin, intent: nil) }
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
    // Runs only after the original engine has updated its event slot/lifetime.
    // This notification only marks/enqueues; returnedWork must never join us.
    fileprivate func eventBookkeepingCompleted(_ event: CloudEngineEvent, origin: String) {
        guard case .sent = event, !quiescing, let instance = owned[origin], !instance.failed,
              !instance.stopping, !instance.assetClosed, !suspended, !suspending,
              current == origin, assetReadTransport != nil else { return }
        if !instance.assetQueue.contains(where: { $0.retry == nil }) {
            let request = AssetReadRequest()
            assetRequestIDs[request.id] = (origin, request)
            instance.assetQueue.append(request)
        }
        startAssetReads(origin, instance: instance)
    }

    private func startAssetReads(_ origin: String, instance: Instance) {
        guard instance.assetDispatch == nil, !instance.assetClosed else { return }
        instance.assetDispatch = Task { await self.runAssetReadQueue(origin, instance: instance) }
        changed()
    }

    func executePolicyAssetReads(_ command: CloudPolicyCommand, execution: CoreCloudPolicyExecution) async -> CloudPolicyNativeResult {
        do {
            guard let origin = try await execution.originalSDK(), let instance = owned[origin],
                  !quiescing, !suspended, !suspending, !instance.assetClosed, assetReadTransport != nil,
                  await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
            try Task.checkCancellation()
            guard owned[origin] === instance, !instance.stopping, !instance.assetClosed,
                  !quiescing, !suspended, !suspending else { throw CancellationError() }
            // Install before suspending, retaining THIS command and execution.
            let requestID = UUID()
            return await withTaskCancellationHandler {
                await withCheckedContinuation { completion in
                    let request = AssetReadRequest(id: requestID, retry: (command, execution), completion: completion)
                    assetRequestIDs[requestID] = (origin, request)
                    instance.assetQueue.append(request)
                    changed()
                    startAssetReads(origin, instance: instance)
                }
            } onCancel: { Task { await self.cancelAssetRequest(requestID) } }
        } catch { return .init(outputs: [], error: error, deferred: false) }
    }
    private var assetRequestIDs: [UUID: (String, AssetReadRequest)] = [:]
    private func cancelAssetRequest(_ id: UUID) {
        guard let (origin, request) = assetRequestIDs[id], let instance = owned[origin] else { return }
        request.cancelled = true
        if instance.assetPass?.0 === request { instance.assetPass?.1.cancel() }
        else if request.retry == nil, let index = instance.assetQueue.firstIndex(where: { $0 === request }) {
            instance.assetQueue.remove(at: index)
            assetRequestIDs.removeValue(forKey: id)
            finishAssetRequest(request)
            changed()
        }
    }
    private func cancelAssetRequests(_ ids: [UUID]) {
        for id in ids { cancelAssetRequest(id) }
    }
    private func finishAssetRequest(_ request: AssetReadRequest) {
        request.drained = true
        let joins = request.joins; request.joins.removeAll()
        for join in joins { join.resume() }
    }
    private func joinOriginalFirstAssetReads(_ instance: Instance) async {
        let requests = ([instance.assetPass?.0].compactMap { $0 } + instance.assetQueue).filter { $0.retry == nil }
        guard !requests.isEmpty else { return }
        let ids = requests.map(\.id)
        instance.assetFirstReadJoins += 1; changed()
        defer { instance.assetFirstReadJoins -= 1; changed() }
        await withTaskCancellationHandler {
            if Task.isCancelled { cancelAssetRequests(ids) }
            for request in requests where !request.drained {
                await withCheckedContinuation { request.joins.append($0) }
            }
        } onCancel: { Task { await self.cancelAssetRequests(ids) } }
    }

    // Deterministic owner observations for focused tests; these wait on actual
    // queue/admission state and grant no Core or SDK authority.
    func waitForQueuedAssetReads(sdk: String, count: Int) async throws {
        while let instance = owned[sdk], instance.assetQueue.filter({ $0.retry != nil }).count < count {
            let old = version; try await waitForChange(after: old)
        }
    }
    func waitForAssetReadAdmissionClosed(sdk: String) async throws {
        while let instance = owned[sdk], !instance.assetClosed {
            let old = version; try await waitForChange(after: old)
        }
    }
    func waitForFirstAssetReadJoin(sdk: String) async throws {
        while let instance = owned[sdk], instance.assetFirstReadJoins == 0 {
            let old = version; try await waitForChange(after: old)
        }
    }

    private func runAssetReadQueue(_ origin: String, instance: Instance) async {
        defer { instance.assetDispatch = nil; changed() }
        while !instance.assetQueue.isEmpty {
            let request = instance.assetQueue.removeFirst()
            let result: CloudPolicyNativeResult
            if request.cancelled || instance.assetClosed || instance.stopping || Task.isCancelled {
                result = .init(outputs: [], error: CancellationError(), deferred: false)
            } else {
                let task = Task { await self.performAssetRead(origin, instance: instance, request: request) }
                instance.assetPass = (request, task)
                result = await task.value
                instance.assetPass = nil
            }
            assetRequestIDs = assetRequestIDs.filter { $0.value.1 !== request }
            request.completion?.resume(returning: result)
            if request.retry == nil, let error = result.error { lastFailure = error }
            // The normal coordinator lane consumes background activity outputs.
            // It may queue a refresh, but this worker never starts a retry from it.
            await assetReadDidFinish()
            finishAssetRequest(request)
        }
    }

    private func assetReadLive(_ origin: String, instance: Instance) -> Bool {
        current == origin && owned[origin] === instance && !instance.stopping &&
            !instance.assetClosed && !suspended && !suspending
    }

    private func performAssetRead(_ origin: String, instance: Instance, request: AssetReadRequest) async -> CloudPolicyNativeResult {
        var pass: CoreCloudAssetReadPass?
        let activity = AssetReadActivity()
        var failure: (any Error)?
        do {
            try await instance.engine.waitForEventBookkeeping()
            try Task.checkCancellation()
            guard await valid(origin), assetReadLive(origin, instance: instance) else { throw CancellationError() }
            try Task.checkCancellation()
            if let (_, execution) = request.retry {
                try await execution.validate()
                guard await valid(origin), assetReadLive(origin, instance: instance) else { throw CancellationError() }
                try Task.checkCancellation()
                pass = try await execution.beginAssetReadPass()
            } else { pass = try await core.beginFirstAssetReadPass(sdk: origin) }
            guard let originalPass = pass else { throw BridgeError.cloudPolicy(.staleCommand) }
            try Task.checkCancellation()
            guard await valid(origin), assetReadLive(origin, instance: instance) else { throw CancellationError() }
            try Task.checkCancellation()
            let worker: CloudAssetReadExecutor
            if let retained = instance.assetWorker { worker = retained }
            else {
                guard let assetReadTransport else { throw BridgeError.cloudPolicy(.coreCapabilityUnavailable) }
                worker = CloudAssetReadExecutor(transport: assetReadTransport)
                instance.assetWorker = worker
            }
            _ = try await worker.run(originalPass) { event in
                await activity.observe(event)
                if case .started = event { await self.reportAssetReadStart(originalPass) }
            }
        } catch { failure = error }
        // A pass can be cancelled after begin but before executor entry. Finish
        // is idempotent and always addresses the original capability.
        if let pass {
            do { try await pass.finish() } catch { if failure == nil { failure = error } }
            let (started, failures) = await activity.snapshot()
            if started {
                do {
                    policyOperationOutputs.append(contentsOf: try await pass.reportActivity(started: false,
                        failures: failures, failed: failure != nil,
                        appCancelled: cloudOperationWasCancelled(failure) || Task.isCancelled || instance.assetClosed))
                } catch { if failure == nil { failure = error } }
            }
        }
        if await valid(origin), assetReadLive(origin, instance: instance) {
            do { try await consumePendingUpdates(origin: origin) }
            catch { if failure == nil { failure = error } }
        }
        return .init(outputs: [], error: failure, deferred: false)
    }
    private func reportAssetReadStart(_ pass: CoreCloudAssetReadPass) async {
        do { policyOperationOutputs.append(contentsOf: try await pass.reportActivity(started: true)) }
        catch { lastFailure = error }
    }
    private func drainOriginalAssetReads(_ origin: String, permanently: Bool = true) async {
        guard let instance = owned[origin] else { return }
        if let joining = instance.assetDrain {
            await joining.value
            if permanently { await instance.assetWorker?.close() }
            return
        }
        instance.assetClosed = true; changed()
        instance.assetPass?.1.cancel()
        instance.assetDispatch?.cancel()
        let dispatch = instance.assetDispatch
        let worker = instance.assetWorker
        let drain = Task {
            if permanently { await worker?.close() }
            else { await worker?.interruptAndDrain() }
            await dispatch?.value
        }
        instance.assetDrain = drain
        await drain.value
    }

    fileprivate func nextBatch(origin: String, pending: [String: CloudPendingChange], zone: String) async -> CloudEngineBatch? {
        guard await valid(origin), let instance = owned[origin], (zone == instance.zone || zone == "meguri-media") else { return nil }
        await delegateWorkValidated()
        guard !quiescing, !suspended, !suspending, current == origin, owned[origin] === instance, !instance.stopping else { return nil }
        do {
            var pending = pending
            if let reads = instance.assetDispatch {
                await reads.value
                guard await valid(origin), !quiescing, !suspended, !suspending, assetReadLive(origin, instance: instance) else { return nil }
                try await consumePendingUpdates(origin: origin)
                guard await valid(origin), !quiescing, !suspended, !suspending else { return nil }
                pending = await instance.engine.pendingRecordZoneChanges()
                guard await valid(origin), !quiescing, !suspended, !suspending else { return nil }
            }
            let batch = try await core.buildBatch(sdk: origin, zone: zone, pending: pending)
            guard await valid(origin), !quiescing, !suspended, !suspending else { return nil }
            if let batch { instance.batches[batch.id] = UInt32(batch.saves.count + batch.deletes.count) }
            return batch
        } catch { lastFailure = error; instance.failed = true; _ = beginStop(origin, policyStop: false); return nil }
    }
    fileprivate func fetchOptions(origin: String, options: CloudFetchOptions) async -> CloudFetchOptions {
        guard await valid(origin), let instance = owned[origin] else { return .init(zones: [], priorityZones: [], groupID: options.groupID) }
        await delegateWorkValidated()
        guard !quiescing, !suspended, !suspending, current == origin, owned[origin] === instance, !instance.stopping else { return .init(zones: [], priorityZones: [], groupID: options.groupID) }
        let allowed = Set(["meguri-root", instance.zone])
        return .init(zones: options.zones.filter { allowed.contains($0) }, priorityZones: options.priorityZones.filter { allowed.contains($0) }, groupID: options.groupID)
    }
    public func takePolicyOperationOutputs() -> [CloudPolicyOutput] {
        let outputs = policyOperationOutputs; policyOperationOutputs.removeAll(); return outputs
    }
    private func reportOperation(_ execution: CoreCloudPolicyExecution?, zones: [CloudPolicyZone], kind: CloudPolicyOperationKind, started: Bool, error: (any Error)? = nil, saved: UInt64 = 0) async throws {
        guard let execution else { return }
        policyOperationOutputs.append(try await execution.report(zones: zones, kind: kind, started: started, error: error, saved: saved))
    }
    public func requestSend(zones: [String]) async throws {
        guard let origin = current else { return }
        try await send(zones: zones, origin: origin)
    }
    private func send(zones: [String], origin: String, execution: CoreCloudPolicyExecution? = nil) async throws {
        guard !quiescing, !suspended, !suspending else { throw CancellationError() }
        guard let instance = owned[origin], await valid(origin) else {
            if execution != nil { throw BridgeError.cloudPolicy(.staleCommand) }; return
        }
        try await consumePendingUpdates(origin: origin)
        guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
        let call: UInt64?
        if let execution { call = try await execution.beginSend() }
        else { call = try await core.beginSend(sdk: origin) }
        guard let call else { return }
        if zones.contains("meguri-media") { try await consumePendingUpdates(origin: origin) }
        guard await valid(origin) else { return }
        instance.networks += 1; changed()
        defer { instance.networks -= 1; changed() }
        let mediaScope = zones.contains("meguri-media")
        let reportedZones: [CloudPolicyZone] = mediaScope ? [.media] : [.collection]
        var sendError: (any Error)?
        let invocationOutputs = CloudPolicyInvocationOutputs()
        do {
            if let execution { try await execution.validate() }
            guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
            instance.operationSaved = 0
            instance.mediaSendFailure = nil
            let validateSDK = try await core.finalSDKAuthorization(origin)
            let prepared = try await instance.engine.prepareSend(zones: zones.filter { $0 == instance.zone || $0 == "meguri-media" })
            guard current == origin, owned[origin] === instance, !instance.stopping else { throw BridgeError.cloudPolicy(.staleCommand) }
            guard !quiescing else { throw CancellationError() }
            let operation = try prepared.enqueue {
                try validateSDK()
                try execution?.validateForEnqueue()
                if let execution { invocationOutputs.append(try execution.reportInvocation(zones: reportedZones, kind: .send)) }
            }
            try await operation.drain()
            if let execution { try await execution.validate() }
        }
        catch { sendError = error }
        policyOperationOutputs.append(contentsOf: invocationOutputs.take())

        if sendError == nil { sendError = instance.mediaSendFailure }
        // SDK delivery has returned. Join its original first-read/apply work
        // before finishSend observes pending changes and decides another round.
        await joinOriginalFirstAssetReads(instance)
        if sendError == nil, Task.isCancelled { sendError = CancellationError() }
        if await valid(origin) {
            try await reportOperation(execution, zones: reportedZones, kind: .send, started: false, error: sendError, saved: instance.operationSaved)
        }
        guard await valid(origin) else { if let sendError { throw sendError }; return }
        try await waitForDeferredUpdates(origin: origin)
        if let execution { try await execution.validate() }
        guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
        try await waitForDeferredUpdates(origin: origin)
        if let execution { try await execution.validate() }
        guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
        let pending = await instance.engine.pendingRecordZoneChanges()
        guard await valid(origin) else { return }
        try await core.finishSend(sdk: origin, id: call, pending: pending)
        try await consumePendingUpdates(origin: origin)
        if let sendError { throw sendError }
    }
    private func fetch(zones: [String], origin pinnedOrigin: String? = nil, execution: CoreCloudPolicyExecution? = nil) async throws -> Bool {
        guard !quiescing, !suspended, !suspending else { throw CancellationError() }
        let requestedOrigin = pinnedOrigin ?? current
        while fetchBusy { let old = version; try await waitForChange(after: old) }
        guard !quiescing else { throw CancellationError() }
        guard let origin = requestedOrigin, let instance = owned[origin], await valid(origin) else {
            if execution != nil { throw BridgeError.cloudPolicy(.staleCommand) }; return false
        }
        fetchBusy = true; instance.networks += 1; changed()
        defer { fetchBusy = false; instance.networks -= 1; changed() }
        let scope = zones.filter { $0 == "meguri-root" || $0 == instance.zone }
        for _ in 0..<2 {
            guard !quiescing else { throw CancellationError() }
            guard await valid(origin) else { return false }
            let id: UInt64
            if let execution { id = try await execution.beginFetch(zones: scope) }
            else { id = try await core.beginFetch(sdk: origin, zones: scope) }
            guard await valid(origin) else { return false }
            let invocationOutputs = CloudPolicyInvocationOutputs()
            do {
                if let execution { try await execution.validate() }
                guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
                let validateSDK = try await core.finalSDKAuthorization(origin)
                let prepared = try await instance.engine.prepareFetch(options: .init(zones: scope, priorityZones: scope, groupID: "meguri-\(id)"))
                guard current == origin, owned[origin] === instance, !instance.stopping else { throw BridgeError.cloudPolicy(.staleCommand) }
                guard !quiescing else { throw CancellationError() }
                let operation = try prepared.enqueue {
                    try validateSDK()
                    try execution?.validateForEnqueue()
                    if let execution { invocationOutputs.append(try execution.reportInvocation(zones: [.root, .collection], kind: .fetch)) }
                }
                try await operation.drain()
                if let execution { try await execution.validate() }
            }
            catch {
                policyOperationOutputs.append(contentsOf: invocationOutputs.take())
                if await valid(origin) {
                    _ = try await core.finishFetch(sdk: origin, id: id)
                    try await reportOperation(execution, zones: [.root, .collection], kind: .fetch, started: false, error: error)
                }
                throw error
            }
            guard await valid(origin) else { return false }
            policyOperationOutputs.append(contentsOf: invocationOutputs.take())
            let drained = try await core.finishFetch(sdk: origin, id: id)
            try await reportOperation(execution, zones: [.root, .collection], kind: .fetch, started: false)
            if drained { return true }
        }
        return false
    }
    public func requestFetch(zones: [String]) async throws { _ = try await fetch(zones: zones) }
    public func syncNow() async throws -> CloudExchangeResult {
        guard !quiescing else { throw CancellationError() }
        try Task.checkCancellation()
        // A policy owner reserves BEFORE facade capture. Public callers wait
        // for its actual worker and join it, never create a competing capture.
        while policyReservation != nil && exchangeTask == nil {
            let old = version; try await waitForChange(after: old)
        }
        guard !quiescing else { throw CancellationError() }
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
    /// True means retained work, not a completed EngineStopped transition.
    func executePolicyLifecycle(_ command: CloudPolicyCommand, execution: CoreCloudPolicyExecution, policy: CoreCloudPolicyService) async throws -> Bool {
        let original = try await execution.originalSDK()
        switch command.action {
        case .stopEngine:
            try await execution.validateControl()
            requestedRunning = false
            intentGeneration &+= 1
            exchangeTask?.task.cancel()
            if quiescing {
                heldStops.append((command, execution, policy))
                return true
            }
            if let original { try await stopInstance(original, policyStop: false, execution: execution) }
            else { try await execution.retireOriginal() }
            if quiescing, let original, let instance = owned[original], !instance.authorityRetired {
                heldStops.append((command, execution, policy))
                return true
            }
            // A superseded factory owns its own registration until its catch
            // path cancels/releases it. Replacement waits on creatingSDK.
            while creatingSDK { let old = version; try await waitForChange(after: old) }
        case .startEngine(let automatic), .configureAutomatic(let automatic):
            guard !quiescing else { throw CancellationError() }
            requestedRunning = true
            intentGeneration &+= 1
            let intent = intentGeneration
            self.automatic = automatic
            if case .configureAutomatic = command.action {
                if let original { try await stopInstance(original, policyStop: false, execution: execution) }
                else { try await execution.retireOriginal() }
            }
            try checkIntent(intent)
            await observePlatformAccounts(intent: intent)
            try checkIntent(intent)
            try await startPrepared(automaticallySync: automatic, readIdentity: true, intent: intent, execution: execution)
        default: throw BridgeError.cloudPolicy(.notNativeAction)
        }
        return false
    }
    func executePolicyOperation(_ command: CloudPolicyCommand, execution: CoreCloudPolicyExecution) async throws {
        guard let origin = try await execution.originalSDK(), let instance = owned[origin] else {
            throw BridgeError.cloudPolicy(.staleCommand)
        }
        func scope(_ zones: [CloudPolicyZone]) -> [String] {
            zones.map { zone in
                switch zone {
                case .root: return "meguri-root"
                case .collection: return instance.zone
                case .media: return "meguri-media"
                }
            }
        }
        switch command.action {
        case .requestFetch(let zones): _ = try await fetch(zones: scope(zones), origin: origin, execution: execution)
        case .requestSend(let zones): try await send(zones: scope(zones), origin: origin, execution: execution)
        default: throw BridgeError.cloudPolicy(.notNativeAction)
        }
    }
    func flushPolicyCollection(execution: CoreCloudPolicyExecution) async throws {
        guard !suspended, !suspending, let origin = try await execution.originalSDK(), let instance = owned[origin] else {
            throw BridgeError.cloudPolicy(.staleCommand)
        }
        try await waitForDeferredUpdates(origin: origin)
        try await execution.validate()
        try await send(zones: [instance.zone], origin: origin, execution: execution)
        try await waitForDeferredUpdates(origin: origin)
    }
    func cancelPolicyFlush(execution: CoreCloudPolicyExecution) async throws {
        try await execution.validateControl()
        guard let origin = try await execution.originalSDK(), let instance = owned[origin] else { return }
        await instance.engine.cancelOperations()
    }
    func sendPolicyMedia(execution: CoreCloudPolicyExecution) async throws {
        guard let origin = try await execution.originalSDK(), let instance = owned[origin], await valid(origin) else {
            throw BridgeError.cloudPolicy(.staleCommand)
        }
        // Core qualifies both sends; the same original SDK serves collection first.
        try await send(zones: [instance.zone], origin: origin, execution: execution)
        try await send(zones: ["meguri-media"], origin: origin, execution: execution)
    }
    func waitForMediaFetchDrain(core: CoreCloudMediaService, execution: CoreCloudPolicyExecution) async throws {
        guard let origin = try await execution.originalSDK() else { throw BridgeError.cloudPolicy(.staleCommand) }
        while true {
            let old = version
            try await execution.validate()
            guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
            if try await core.downloadsBlocked(sdk: origin) == false { return }
            try await waitForChange(after: old)
        }
    }

    func waitForPolicyReservationWaiter() async throws {
        while policyReservationWaiters == 0 { let old = version; try await waitForChange(after: old) }
    }
    func reservePolicyExchange() async throws -> UUID {
        guard !quiescing else { throw CancellationError() }
        policyReservationWaiters += 1; changed()
        defer { policyReservationWaiters -= 1; changed() }
        while exchangeTask != nil || policyReservation != nil {
            if let running = exchangeTask {
                let old = version
                // Preserve the prior worker's actual result; a new policy
                // exchange starts only after its original completion/drain.
                _ = try? await running.task.value
                if exchangeTask?.id == running.id { try await waitForChange(after: old) }
            } else { let old = version; try await waitForChange(after: old) }
            try Task.checkCancellation()
        }
        guard !quiescing else { throw CancellationError() }
        let id = UUID(); policyReservation = id; exchangeRunning = true; changed()
        return id
    }
    func releasePolicyExchange(_ reservation: UUID) {
        guard policyReservation == reservation else { return }
        policyReservation = nil
        if exchangeTask == nil { exchangeRunning = false }
        changed()
    }
    func preparePolicyExchange(execution: CoreCloudPolicyExecution) async throws -> PreparedPolicyExchange {
        guard !quiescing else { throw CancellationError() }
        guard let origin = try await execution.originalSDK() else { throw BridgeError.cloudPolicy(.staleCommand) }
        while fetchBusy { let old = version; try await waitForChange(after: old) }
        try await waitForDeferredUpdates(origin: origin)
        try await execution.validate()
        guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
        guard let instance = owned[origin] else { throw BridgeError.cloudPolicy(.staleCommand) }
        return PreparedPolicyExchange(origin: origin, zone: instance.zone)
    }
    func runPolicyExchange(runtimeID: UInt64, execution: CoreCloudPolicyExecution, prepared: PreparedPolicyExchange) async throws {
        // Preparation pinned this original before capture. No awaited lookup
        // or busy admission can throw between capture and installing its worker.
        let origin = prepared.origin
        let id = UUID()
        let task = Task {
            return try await self.runCapturedExchange(origin: origin, zone: prepared.zone, id: runtimeID, execution: execution)
        }
        exchangeTask = (id, task); policyWorkerID = id; exchangeOrigin = origin; exchangeRunning = true; changed()
        defer { exchangeTask = nil; policyWorkerID = nil; exchangeOrigin = nil; exchangeRunning = false; changed() }
        _ = try await withTaskCancellationHandler { try await task.value }
            onCancel: { Task { await self.cancelPolicyExchange() } }
    }
    func cancelPolicyExchange() async {
        guard let policyWorkerID, exchangeTask?.id == policyWorkerID else { return }
        exchangeTask?.task.cancel()
        // Snapshot the original objects before suspension; cancellation never
        // switches to the replacement's fresh current SDK.
        let engine = exchangeOrigin.flatMap { owned[$0]?.engine }
        await engine?.cancelOperations()
    }
    private func cancelExchange(id: UUID) {
        if exchangeTask?.id == id { exchangeTask?.task.cancel() }
    }
    private func runOwnedExchange() async throws -> CloudExchangeResult {
        defer { exchangeTask = nil; exchangeOrigin = nil; exchangeRunning = false; changed() }
        guard let origin = current, let instance = owned[origin], await valid(origin) else { throw BridgeError.internal("Cloud engine is unavailable") }
        let id = try await core.beginExchange(sdk: origin)
        exchangeOrigin = origin
        return try await runCapturedExchange(origin: origin, zone: instance.zone, id: id)
    }
    private func runCapturedExchange(origin: String, zone: String, id: UInt64, execution: CoreCloudPolicyExecution? = nil) async throws -> CloudExchangeResult {
        let zones = ["meguri-root", zone]
        var fetched = false
        var operationError: (any Error)?
        do {
            try Task.checkCancellation()
            fetched = try await fetch(zones: zones, origin: origin, execution: execution)
            try Task.checkCancellation()
            try await core.exchangeFetched(sdk: origin, complete: fetched)
            if fetched {
                repeat {
                    try Task.checkCancellation()
                    try await send(zones: [zone], origin: origin, execution: execution)
                } while try await core.sendDue(sdk: origin)
                fetched = try await fetch(zones: zones, origin: origin, execution: execution)
            }
            try Task.checkCancellation()
        } catch { operationError = error }
        let result = try await core.completeExchange(sdk: origin, id: id, fetched: fetched,
                                                     cancelled: cloudOperationWasCancelled(operationError),
                                                     failed: operationError != nil && !cloudOperationWasCancelled(operationError))
        exchangeRunning = false
        if execution == nil, let held = heldAutomatic {
            heldAutomatic = nil
            do { try await configureAutomatic(held) }
            catch { if operationError == nil { throw error }; lastFailure = error }
        }
        if let operationError { throw operationError }
        return result
    }
}
