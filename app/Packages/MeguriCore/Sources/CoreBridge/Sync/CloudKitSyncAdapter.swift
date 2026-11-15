import Foundation
import Synchronization

#if DEBUG
// Tests pause at completed Core calls.
enum CloudSDKConstructionBoundary {
    @TaskLocal static var registered: (@Sendable (String) async -> Void)?
    @TaskLocal static var failedRetirement: (@Sendable (String) async -> Void)?
}
#endif

private final class CloudSDKRegistration: Sendable {
    let task: Task<CloudRegistration, any Error>
    init(_ operation: @escaping @Sendable () async throws -> CloudRegistration) {
        task = Task { try await operation() }
    }
}

/// Immutable callback origin. An old delivery never looks up a replacement's id.
private actor CloudSDKDelegate: CloudKitEngineDelegate {
    private weak var adapter: CloudKitSyncAdapter?
    let origin: String
    init(adapter: CloudKitSyncAdapter, origin: String) { self.adapter = adapter; self.origin = origin }
    func eventWillDeliver(engine: any CloudKitEngine) async { await adapter?.eventWillDeliver(origin: origin) }
    func handleEvent(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {
        await adapter?.handleEvent(event, origin: origin, engine: engine)
    }
    func eventHandlerReturned(_ event: CloudEngineEvent, engine: any CloudKitEngine) async {
        await adapter?.eventHandlerReturned(event, origin: origin)
    }
    func savedPendingRemovalsConfirmed(_ receipts: [CloudSavedPendingRemoval], engine: any CloudKitEngine) async {
        await adapter?.savedPendingRemovalsConfirmed(receipts, origin: origin, engine: engine)
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

/// Records a dependent fetch refusal for its original command and SDK.
/// The retained proof grants no execution authority.
public struct CloudPolicyDependentFetchRefusal: Sendable {
    private let command: CloudPolicyCommand
    private let execution: CoreCloudPolicyExecution
    private let original: CloudSDKStopWitness
    private let dependencies: [CloudQuiescentDependencyWait]
    fileprivate init(command: CloudPolicyCommand, execution: CoreCloudPolicyExecution,
                     original: CloudSDKStopWitness, dependencies: [CloudQuiescentDependencyWait]) {
        self.command = command; self.execution = execution
        self.original = original; self.dependencies = dependencies
    }
}
struct CloudDependentFetchRefusalError: Error {
    let error: BridgeError
    let qualification: CloudPolicyDependentFetchRefusal
}

// One original proof wait owns both the first result and its producer. The
// account receipt is attached at its validated original stop/cancel boundary.
fileprivate struct CloudFetchProofCompletion: Sendable {
    struct AccountStop: Sendable {
        let command: CloudPolicyCommand
        let execution: CoreCloudPolicyExecution
    }
    enum Producer: Sendable { case provider, caller(CloudPolicyCancellationContext.Snapshot?), adapterStop, accountStop(AccountStop) }
    let result: Result<Void, any Error>
    let producer: Producer
}

struct CloudFetchProofIssuer: Sendable {
    let command: CloudPolicyCommand
    let execution: CoreCloudPolicyExecution
    let original: CloudSDKStopWitness
}

// Captured from the adapter's retained instance at a real cancellation boundary.
struct CloudAdapterNativeCancellationEdge: Sendable {
    let command: CloudPolicyCommand
    let execution: CoreCloudPolicyExecution
    let sdk: String
    let dependency: CloudQuiescentDependencyWait?
    let original: CloudSDKStopWitness
    let cause: CloudPolicyCancellationContext.Snapshot?
    init(issuer: CloudFetchProofIssuer, dependency: CloudQuiescentDependencyWait? = nil,
         cause: CloudPolicyCancellationContext.Snapshot? = CloudPolicyCancellationContext.current?.capture()) {
        self.command = issuer.command
        self.execution = issuer.execution
        self.original = issuer.original
        self.sdk = issuer.original.sdk
        self.dependency = dependency
        self.cause = cause
    }
}

// Only cancelPolicyFlush can issue this receipt, after the retained SDK returns.
// Matching includes the execution object and instance witness, not an SDK name.
struct CloudFlushCancellationReceipt: Sendable {
    private let execution: CoreCloudPolicyExecution
    private let instance: CloudSDKStopWitness
    fileprivate init(execution: CoreCloudPolicyExecution, instance: CloudSDKStopWitness) {
        self.execution = execution
        self.instance = instance
    }
    func matches(_ issuer: CloudFetchProofIssuer) -> Bool {
        execution.raw === issuer.execution.raw && instance === issuer.original
    }
}

#if DEBUG
// Original-task test boundaries; nil outside focused tests.
enum CloudFetchProofCompletionFault {
    @TaskLocal static var afterOriginalDiscard: (@Sendable () throws -> Void)?
    @TaskLocal static var beforeOriginalPropagation: (@Sendable () async -> Void)?
}
#endif

/// Sealed evidence for one original fetch proof result; grants no authority.
public struct CloudPolicyFetchProofOutcome: Sendable {
    fileprivate let completion: CloudFetchProofCompletion
    fileprivate let issuer: CloudFetchProofIssuer?
    fileprivate let original: CloudSDKStopWitness
    fileprivate let earlierFailure: (any Error)?
    fileprivate let cleanupFailure: (any Error)?
    fileprivate let callerCancelled: Bool
    var error: any Error {
        if let earlierFailure { return earlierFailure }
        if case .failure(let error) = completion.result { return error }
        preconditionFailure("Fetch proof failure requires its original error")
    }
    public var accountStopCancellation: Bool {
        guard earlierFailure == nil, cleanupFailure == nil,
              let issuer, issuer.original === original, error is CancellationError,
              case .accountStop = completion.producer else { return false }
        switch issuer.command.action {
        case .requestFetch: return !callerCancelled
        case .runExchange: return true
        default: return false
        }
    }
    var nativeCancellation: CloudPolicyNativeCancellation? {
        guard callerCancelled, earlierFailure == nil, cleanupFailure == nil,
              case .caller(let cause) = completion.producer, case .failure(let error) = completion.result,
              error is CancellationError, let issuer, issuer.original === original else { return nil }
        return .init(adapterEdge: .init(issuer: issuer, cause: cause))
    }
    #if DEBUG
    var debugOriginalCompletionFailure: (any Error)? { cleanupFailure }
    #endif
    public var preservesFailure: Bool {
        if earlierFailure != nil || cleanupFailure != nil { return true }
        if case .provider = completion.producer { return true }
        return false
    }
}
struct CloudFetchProofError: Error {
    let outcome: CloudPolicyFetchProofOutcome
    var error: any Error { outcome.error }
}

// Specific to the post-control-read proof wait. No other completion uses it.
private final class CloudFetchProofWait: Sendable {
    let issuer: CloudFetchProofIssuer?
    let original: CloudSDKStopWitness
    let cancellationIntent: CloudPolicyCancellationContext?
    private struct State {
        var completion: CloudFetchProofCompletion?
        var waiter: CheckedContinuation<CloudFetchProofCompletion, Never>?
    }
    private let state = Mutex(State())
    init(issuer: CloudFetchProofIssuer?, original: CloudSDKStopWitness,
         cancellationIntent: CloudPolicyCancellationContext?) {
        self.issuer = issuer; self.original = original; self.cancellationIntent = cancellationIntent
    }
    func complete(_ result: Result<Void, any Error>, producer: CloudFetchProofCompletion.Producer) {
        let completion = CloudFetchProofCompletion(result: result, producer: producer)
        let waiter = state.withLock { state in
            guard state.completion == nil else { return Optional<CheckedContinuation<CloudFetchProofCompletion, Never>>.none }
            state.completion = completion
            let waiter = state.waiter; state.waiter = nil
            return waiter
        }
        waiter?.resume(returning: completion)
    }
    func drain() async -> CloudFetchProofCompletion {
        await withCheckedContinuation { continuation in
            let completion: CloudFetchProofCompletion? = state.withLock { state in
                if let completion = state.completion { return completion }
                state.waiter = continuation
                return Optional<CloudFetchProofCompletion>.none
            }
            if let completion { continuation.resume(returning: completion) }
        }
    }
}

public actor CloudKitSyncAdapter {
    private enum StopReason: String {
        case registrationFailure, configureAutomatic, heldPolicyStop, finalStop
        case pendingActionsInvalid, handlerFailed, handlerInvalid, postReturnUpdatesFailed
        case nextBatchFailed, policyStop, policyConfigureAutomatic
    }
    #if DEBUG
    private enum FailurePhase: String {
        case handleEvent, handlerReturned, savedPendingRemovals, postReturnUpdates
        case nextBatchPreparation, nextBatchBuild
    }
    #endif
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
        var zoneLossPending = false
        var lossEvidenceGeneration: UInt64 = 0
        var lossAccountGeneration: UInt64?
        func stagedZoneLoss() {
            zoneLossPending = true
            lossEvidenceGeneration &+= 1
            lossAccountRead = nil
        }
        var lossAccountRequested = false
        var lossAccountRead: UInt64?
        var mediaLossObserved = false
        var lossMediaDrainTask: Task<Void, Never>?
        var lossReadTask: Task<Void, Never>?
        var lossAccountTask: (id: UUID, task: Task<Void, Never>)?
        var sdkFetchActive = false
        var rootReadOperation: (any CloudPreparedOwnOperation)?
        var rootReadAdmission: CloudOwnWorkAdmission?
        var rootReadHandle: CoreCloudFetchRead?
        var blockedFetchCallbacks = 0
        var assetReadActive = false
        var rootReadIdentity: (task: Task<Void, Never>, wait: CloudFetchProofWait)?
        func cancelRootReadIdentity(accountStop: CloudFetchProofCompletion.AccountStop? = nil) {
            rootReadIdentity?.wait.complete(.failure(CancellationError()),
                producer: accountStop.map { .accountStop($0) } ?? .adapterStop)
            rootReadIdentity?.task.cancel()
        }
        var assetWorker: CloudAssetReadExecutor?
        var assetQueue: [AssetReadRequest] = []
        var assetDispatch: Task<Void, Never>?
        var assetPass: (AssetReadRequest, Task<CloudPolicyNativeResult, Never>)?
        var assetDrain: Task<Void, Never>?
        var assetFirstReadJoins = 0
        var assetClosed = false
        let stopWitness: CloudSDKStopWitness
        var firstStopReason: StopReason?
        var stopping = false {
            didSet { stopWitness.update(stopping: stopping, authorityRetired: authorityRetired) }
        }
        var authorityRetired = false {
            didSet { stopWitness.update(stopping: stopping, authorityRetired: authorityRetired) }
        }
        var released = false
        var handlers = 0
        var sentHandlers = 0
        var sentHandlerReturns: UInt64 = 0
        var networks = 0
        var applying = false
        var batches: [UInt64: UInt32] = [:]
        var savedRemovalGuards: [UInt64: (guardValue: CloudGuard, names: Set<String>)] = [:]
        var failed = false
        var retirementFailure: (any Error)?
        #if DEBUG
        private(set) var firstFailure: (phase: FailurePhase, tag: String)?
        var firstStopRecorded = false
        func recordFailure(_ phase: FailurePhase, error: any Error) {
            guard firstFailure == nil else { return }
            // Value uses the existing private failureTag classifier and drops
            // associated payloads. Retain only its fixed failure tag.
            firstFailure = (phase, DebugCloudDiagnostics.Value(.commandResult, error: error).failure)
        }
        func recordDeliveryFailure() {
            if firstFailure == nil { firstFailure = (.handleEvent, "deliveryFailed") }
        }
        #endif
        var operationSaved: UInt64 = 0
        var observationOrdinal: UInt64 = 0
        var mediaSendFailure: CloudKitSDKOperationFailure?
        init(engine: any CloudKitEngine, zone: String, sdk: String) {
            self.engine = engine; self.zone = zone; self.stopWitness = CloudSDKStopWitness(sdk: sdk)
        }
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
    private let observeEvent: (@Sendable (CloudSDKObservation) async -> Void)?
    private let observeBatch: (@Sendable (String, String, CloudEngineBatch) async -> Void)?
    // Scheduling observation only; supplies no validity or Core outcome.
    private let delegateWorkValidated: @Sendable () async -> Void
    private var owned: [String: Instance] = [:]
    private var current: String?
    private var automatic = false
    private var heldAutomatic: Bool?
    private var requestedRunning = false
    private var intentGeneration: UInt64 = 0
    private var creatingSDK = false
    private var creatingSDKRegistration: CloudSDKRegistration?
    private var identityReads: Set<UInt64> = []
    private var identityResolutions: [UInt64: CloudAccountResolution] = [:]
    private var proofAdmission: (@Sendable (CloudOriginalCommandProofYield) async throws -> Void)?
    private var proofOutputAcknowledgement: (@Sendable ([CloudPolicyOutput]) async -> Void)?
    public func setProofAdmission(_ admission: @escaping @Sendable (CloudOriginalCommandProofYield) async throws -> Void,
                                 acknowledgeOutputs: @escaping @Sendable ([CloudPolicyOutput]) async -> Void) {
        proofAdmission = admission
        proofOutputAcknowledgement = acknowledgeOutputs
    }
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
        fileprivate let proofIssuer: CloudFetchProofIssuer
    }
    private var quiescing = false
    private var quiesceTask: Task<Void, any Error>?
    // Retain the issued handle and its original capability, just as held
    // configuration retains its execution. Only explicit final stop consumes it.
    private var heldStops: [(command: CloudPolicyCommand, execution: CoreCloudPolicyExecution, policy: CoreCloudPolicyService)] = []
    private var finalStopTask: Task<Void, any Error>?
    private var stopTasks: [String: Task<Void, any Error>] = [:]
    // All instance control reads retain this exclusion through Core acceptance.
    private var epochReadOrigin: String?
    private var epochPreparationTask: Task<Bool, any Error>?
    private var fetchBusy = false
    private var policyOperationOutputs: [CloudPolicyOutput] = []
    private var version: UInt64 = 0
    private var waiters: [UUID: CheckedContinuation<Void, any Error>] = [:]
    private var readinessObservers: [UUID: AsyncStream<Void>.Continuation] = [:]
    private var originalMediaDrain: (@Sendable (CloudSDKStopWitness) async -> Void)?
    func setOriginalMediaDrain(_ drain: @escaping @Sendable (String) async -> Void) {
        originalMediaDrain = { witness in await drain(witness.sdk) }
    }
    func setOriginalMediaDrainWithWitness(_ drain: @escaping @Sendable (CloudSDKStopWitness) async -> Void) { originalMediaDrain = drain }

    #if DEBUG
    // Scheduling only, after event validation and before its Core mutation.
    private var validatedEventHook: (event: CloudEngineEvent, action: @Sendable () async -> Void)? = nil
    func setValidatedEventHook(_ hook: (event: CloudEngineEvent, action: @Sendable () async -> Void)?) {
        validatedEventHook = hook
    }
    // Scheduling only, after validation and before the pending drain's Core call.
    private var pendingUpdateValidatedHook: (afterSDKMutation: Bool, action: @Sendable () async -> Void)? = nil
    func setPendingUpdateValidatedHook(_ hook: (afterSDKMutation: Bool, action: @Sendable () async -> Void)?) {
        pendingUpdateValidatedHook = hook
    }
    // Join only work already scheduled by the original path; never reconcile.
    func joinOriginalStopTask(instanceID: String) async throws -> Bool {
        if let task = stopTasks[instanceID] { try await task.value; return true }
        return owned[instanceID] == nil
    }
    private var fetchReservationObserver: (@Sendable () async -> Void)?
    func setFetchReservationObserver(_ observer: (@Sendable () async -> Void)?) { fetchReservationObserver = observer }
    private var epochReservationObserver: (@Sendable () async -> Void)?
    func setEpochReservationObserver(_ observer: (@Sendable () async -> Void)?) { epochReservationObserver = observer }
    private var epochAcceptanceObserver: (@Sendable () async -> Void)?
    func setEpochAcceptanceObserver(_ observer: (@Sendable () async -> Void)?) { epochAcceptanceObserver = observer }
    private var nextRetirementError: (any Error)?
    func failNextRetirement(_ error: any Error) { nextRetirementError = error }
    // Scheduling observation only, after the lane-backed call releases Core.
    private var sendAdmissionObserver: (@Sendable (Bool) async -> Void)?
    private var policyOperationOriginObserver: (@Sendable () async -> Void)?
    func setPolicyOperationOriginObserver(_ observer: (@Sendable () async -> Void)?) {
        policyOperationOriginObserver = observer
    }
    // Observe exact Core call IDs at admission and successful finalization.
    private var sendCallObserver: (@Sendable (String, UInt64, Bool) -> Void)?
    // Scheduling observation of the first source error, before cleanup awaits.
    private var sendErrorObserver: (@Sendable () async -> Void)? = nil
    func setSendErrorObserver(_ observer: (@Sendable () async -> Void)?) {
        sendErrorObserver = observer
    }
    private var mediaSendWaits = 0
    func setSendAdmissionObserver(_ observer: (@Sendable (Bool) async -> Void)?) {
        sendAdmissionObserver = observer
    }
    func setSendCallObserver(_ observer: (@Sendable (String, UInt64, Bool) -> Void)?) {
        sendCallObserver = observer
    }
    func waitForMediaSendAdmissionWaiter() async throws {
        while mediaSendWaits == 0 {
            guard policySDKReady() else { throw CancellationError() }
            let old = version
            try await waitForChange(after: old)
        }
    }
    #endif

    public private(set) var lastFailure: (any Error)?

    public init(core: CoreCloudWorkflowService, engineFactory: any CloudKitEngineFactory,
                eventDidPersist: @escaping @Sendable (CloudEngineEvent) async -> Void = { _ in },
                assetReadTransport: (any CloudOwnTransport)? = nil,
                assetReadDidFinish: @escaping @Sendable () async -> Void = {},
                exchangeDidJoin: @escaping @Sendable () async -> Void = {},
                appOwnsAccountObservation: Bool = false,
                accountReadRequested: @escaping @Sendable () async -> Void = {},
                delegateWorkValidated: @escaping @Sendable () async -> Void = {},
                observeEvent: (@Sendable (CloudSDKObservation) async -> Void)? = nil,
                observeBatch: (@Sendable (String, String, CloudEngineBatch) async -> Void)? = nil) {
        self.core = core; factory = engineFactory; self.eventDidPersist = eventDidPersist
        self.exchangeDidJoin = exchangeDidJoin
        self.assetReadTransport = assetReadTransport; self.assetReadDidFinish = assetReadDidFinish
        self.appOwnsAccountObservation = appOwnsAccountObservation
        self.accountReadRequested = accountReadRequested
        self.delegateWorkValidated = delegateWorkValidated
        self.observeEvent = observeEvent; self.observeBatch = observeBatch
    }
    private func changed(readiness: Bool = true) {
        version &+= 1
        if readiness { for observer in readinessObservers.values { observer.yield(()) } }
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
    private func cancelWait(_ id: UUID, error: any Error) { waiters.removeValue(forKey: id)?.resume(throwing: error) }
    private func waitForChange(after old: UInt64, cancellationError: @escaping @Sendable () -> any Error = { CancellationError() }) async throws {
        if Task.isCancelled { throw cancellationError() }
        if version != old { return }
        let id = UUID()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                if Task.isCancelled { continuation.resume(throwing: cancellationError()) }
                else if version != old { continuation.resume() }
                else { waiters[id] = continuation }
            }
        } onCancel: {
            let error = cancellationError()
            Task { await self.cancelWait(id, error: error) }
        }
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
        // A superseded creation retains its original SDK through cleanup.
        // Join it before a replacement identity read can capture that owner.
        while creatingSDK {
            let old = version
            try await waitForChange(after: old)
            try checkIntent(intent)
        }
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
        defer { creatingSDK = false; creatingSDKRegistration = nil; changed() }
        // One owned task performs registration. Media can join its exact result
        // even before this lifecycle continuation receives the new identity.
        let pending = CloudSDKRegistration { [core] in
            if let execution { return try await execution.registerSDK() }
            return try await core.registerSDK(automatic: automaticallySync)
        }
        creatingSDKRegistration = pending
        let registration = try await withTaskCancellationHandler {
            try await pending.task.value
        } onCancel: { pending.task.cancel() }
        #if DEBUG
        if let observe = CloudSDKConstructionBoundary.registered { await observe(registration.instanceID) }
        #endif
        let delegate = CloudSDKDelegate(adapter: self, origin: registration.instanceID)
        var created: (any CloudKitEngine)?
        var published = false
        do {
            try checkIntent(intent)
            let engine = try await factory.makeEngine(configuration: .init(instanceID: registration.instanceID, automaticallySync: automaticallySync, serialization: registration.serialization, collectionZone: registration.collectionZone), delegate: delegate)
            created = engine
            try checkIntent(intent)
            if let execution { try await execution.validate(); try checkIntent(intent) }
            owned[registration.instanceID] = Instance(engine: engine, zone: registration.collectionZone, sdk: registration.instanceID)
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
            creatingSDKRegistration = nil
            if published {
                do { try await stopInstance(registration.instanceID, reason: .registrationFailure, policyStop: false) }
                catch { lastFailure = error }
                throw operationError
            }
            // Close/replacement can already have removed Runtime authority.
            // Native callback/staging ownership must still drain and release.
            do { try await core.retireSDK(registration.instanceID, policyStop: false) }
            catch { lastFailure = error }
            #if DEBUG
            if let observe = CloudSDKConstructionBoundary.failedRetirement { await observe(registration.instanceID) }
            #endif
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
        if let origin = current { try await stopInstance(origin, reason: .configureAutomatic, policyStop: false) }
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
        for (_, instance) in originals {
            instance.lossAccountTask?.task.cancel(); instance.lossReadTask?.cancel()
            instance.rootReadHandle?.cancel(); instance.rootReadAdmission?.setOpen(false); instance.rootReadOperation?.cancel()
            instance.cancelRootReadIdentity()
        }
        for resolution in identityResolutions.values { resolution.cancel() }
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
            for (origin, instance) in originals {
                await self.drainOriginalAssetReads(origin)
                await self.originalMediaDrain?(instance.stopWitness)
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
                try await stopInstance(original, reason: .heldPolicyStop, policyStop: false, execution: held.execution, final: true)
            } else { try await held.execution.retireOriginal() }
            policyOperationOutputs.append(try await held.policy.input(.refresh(.cloud)))
            heldStops.removeFirst()
        }
        let stopping = current.flatMap { beginStop($0, reason: .finalStop, policyStop: true, final: true) }
        suspended = false; suspending = false; changed()
        let pendingStops = Array(stopTasks.values)
        let reads = identityReads
        let exchange = exchangeTask?.task
        exchange?.cancel()
        for id in reads { await cancelIdentityRead(id) }
        if let stopping { try await stopping.value }
        for task in pendingStops { try await task.value }
        if let exchange {
            do { _ = try await exchange.value }
            catch is CancellationError { }
            catch { throw error }
        }
    }
    private func beginStop(_ origin: String, reason: StopReason, policyStop: Bool, execution: CoreCloudPolicyExecution? = nil, final: Bool = false, accountStop: CloudPolicyCommand? = nil) -> Task<Void, any Error>? {
        // Callback/creation error paths must preserve original authority during
        // the same required-effect drain as the issued StopEngine command.
        guard !quiescing || final else { return nil }
        if let task = stopTasks[origin] { return task }
        guard let instance = owned[origin] else { return nil }
        let proofStop: CloudFetchProofCompletion.AccountStop?
        if reason == .policyStop, let accountStop, accountStop.accountRetirement != nil,
           let execution, case .stopEngine(.gateClosed) = accountStop.action,
           current == origin, owned[origin] === instance, !instance.stopping,
           !instance.authorityRetired, !instance.failed, instance.retirementFailure == nil,
           instance.firstStopReason == nil,
           let wait = instance.rootReadIdentity?.wait, let issuer = wait.issuer,
           issuer.original === instance.stopWitness,
           wait.original === instance.stopWitness, wait.original.sdk == origin {
            switch issuer.command.action {
            case .requestFetch, .runExchange:
                proofStop = .init(command: accountStop, execution: execution)
            default: proofStop = nil
            }
        } else { proofStop = nil }
        if instance.firstStopReason == nil { instance.firstStopReason = reason }
        #if DEBUG
        if !instance.firstStopRecorded {
            instance.firstStopRecorded = true
            let phase = instance.firstFailure?.phase.rawValue ?? "none"
            let tag = instance.firstFailure?.tag ?? "none"
            print("cloudNative.firstStop reason=\(reason.rawValue) policyStop=\(policyStop) final=\(final) firstFailurePhase=\(phase) firstFailureTag=\(tag)")
        }
        #endif
        instance.stopping = true
        instance.lossReadTask?.cancel()
        instance.lossAccountTask?.task.cancel()
        instance.lossMediaDrainTask?.cancel()
        instance.rootReadHandle?.cancel(); instance.rootReadAdmission?.setOpen(false)
        instance.cancelRootReadIdentity(accountStop: proofStop)
        let task = Task {
            do {
                try await self.drainInstance(origin, policyStop: policyStop, execution: execution, final: final, accountStop: accountStop)
                self.stopTasks.removeValue(forKey: origin); self.changed()
            } catch {
                if instance.retirementFailure == nil { instance.retirementFailure = error }
                self.lastFailure = error
                self.stopTasks.removeValue(forKey: origin); self.changed()
                throw error
            }
        }
        stopTasks[origin] = task
        changed()
        return task
    }
    private func stopInstance(_ origin: String, reason: StopReason, policyStop: Bool, execution: CoreCloudPolicyExecution? = nil, final: Bool = false, accountStop: CloudPolicyCommand? = nil) async throws {
        if let task = beginStop(origin, reason: reason, policyStop: policyStop, execution: execution, final: final, accountStop: accountStop) { try await task.value }
    }
    private func retireRejectedOriginal(_ origin: String) {
        // Core rejected this captured owner. Schedule its existing drain without
        // joining the SDK from a callback or replacing an earlier real failure.
        _ = beginStop(origin, reason: .handlerInvalid, policyStop: false)
    }
    private func drainInstance(_ origin: String, policyStop: Bool, execution: CoreCloudPolicyExecution?, final: Bool, accountStop: CloudPolicyCommand?) async throws {
        guard let instance = owned[origin] else { return }
        instance.rootReadOperation?.cancel()
        await drainOriginalAssetReads(origin)
        await originalMediaDrain?(instance.stopWitness)
        guard !quiescing || final else { instance.stopping = false; changed(); return }
        if !instance.authorityRetired {
            // If publication inspection or persistence fails, Core still owns the SDK.
            // Keep the instance, return the error, and let stopTasks attempt stop again.
            #if DEBUG
            if let error = nextRetirementError {
                nextRetirementError = nil
                throw error
            }
            #endif
            if let execution { try await execution.retireOriginal() }
            else { try await core.retireSDK(origin, policyStop: policyStop) }
            instance.authorityRetired = true
            if let accountStop, let execution, instance.firstStopReason == .policyStop, !instance.failed,
               instance.retirementFailure == nil {
                instance.stopWitness.recordAccountRetirement(accountStop, execution: execution)
            }
            changed()
        }
        while instance.handlers > 0 || instance.applying { let old = version; try await waitForChange(after: old) }
        await instance.engine.cancelOperations()
        while instance.networks > 0 { let old = version; try await waitForChange(after: old) }
        if !instance.released { await instance.engine.release(); instance.released = true }
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
        let origin = current
        while owned.values.contains(where: { $0.handlers > 0 || $0.applying }) { let old = version; try await waitForChange(after: old) }
        if let origin { try await consumePendingUpdates(origin: origin) }
    }
    private func waitForDeferredUpdates(origin: String) async throws {
        while let instance = owned[origin], instance.handlers > 0 || instance.applying { let old = version; try await waitForChange(after: old) }
        try await consumePendingUpdates(origin: origin)
    }
    public func waitForIdle() async throws {
        while owned.values.contains(where: { $0.handlers > 0 || $0.networks > 0 || $0.applying || $0.stopping || $0.failed || $0.assetDispatch != nil || $0.lossReadTask != nil || $0.lossAccountRequested }) || !stopTasks.isEmpty {
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
        _ = try await refreshPlatformIdentity(transition: transition, intent: intentGeneration)
    }
    private func finishIdentityRead(_ id: UInt64, status: CloudAccountStatus, identity: Data?) async throws -> Bool {
        defer { identityReads.remove(id) }
        return try await core.completeIdentityRead(id: id, status: status, identity: identity)
    }
    private func cancelIdentityRead(_ id: UInt64) async {
        identityResolutions[id]?.cancel()
        guard identityReads.contains(id) else { return }
        do { _ = try await finishIdentityRead(id, status: .couldNotDetermine, identity: nil) }
        catch { lastFailure = error }
    }
    private func checkReadIntent(_ intent: UInt64?) throws {
        if let intent { try checkIntent(intent) }
        else { try Task.checkCancellation() }
    }
    @discardableResult
    private func refreshPlatformIdentity(transition: Bool = false, originSDK: String? = nil, intent: UInt64?) async throws -> Bool {
        try await refreshPlatformIdentityRead(transition: transition, originSDK: originSDK, intent: intent).available
    }
    private func refreshPlatformIdentityRead(transition: Bool = false, originSDK: String? = nil, intent: UInt64?) async throws -> (id: UInt64, available: Bool) {
        try checkReadIntent(intent)
        for resolution in identityResolutions.values { resolution.cancel() }
        let id = try await core.beginIdentityRead(transition: transition, originSDK: originSDK)
        identityReads.insert(id)
        let resolution = CloudAccountResolution()
        identityResolutions[id] = resolution
        defer { identityResolutions.removeValue(forKey: id) }
        do {
            try checkReadIntent(intent)
            let resolved: CloudResolvedAccount?
            do {
                resolved = try await withTaskCancellationHandler {
                    try await resolution.resolve(using: factory)
                } onCancel: { Task { await self.cancelIdentityRead(id) } }
            } catch {
                _ = try await finishIdentityRead(id, status: .couldNotDetermine, identity: nil)
                try checkReadIntent(intent)
                if originSDK != nil { lastFailure = error; return (id, false) }
                throw error
            }
            try checkReadIntent(intent)
            let accepted = try await finishIdentityRead(id, status: resolved?.status ?? .couldNotDetermine, identity: resolved?.identity)
            try checkReadIntent(intent)
            guard accepted else {
                throw BridgeError.unavailable(reason: "Cloud account read expired")
            }
            return (id, resolved?.status == .available && resolved?.identity != nil)
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
        if let origin = current, !(await valid(origin)) { try await stopInstance(origin, reason: .pendingActionsInvalid, policyStop: false) }
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
        let updates: [CloudPendingUpdate]
        #if DEBUG
        if let hook = pendingUpdateValidatedHook, !hook.afterSDKMutation { await hook.action() }
        #endif
        do { updates = try await core.takePendingUpdates(sdk: origin) }
        catch BridgeError.cloudSDKCallbackRejected { retireRejectedOriginal(origin); return }
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
            let mutation = try await instance.engine.applyPendingUpdate(applied)
            guard await valid(origin) else { return }
            let receipts = await instance.engine.takeSavedPendingRemovals()
            await savedPendingRemovalsConfirmed(receipts, origin: origin, engine: instance.engine, retireOnRejection: true)
            guard await valid(origin) else { return }
            // A later application mutation ends any still-unconfirmed original
            // ownership. Its absence must never become a delayed Saved proof.
            let touched = Set(mutation.adds.keys).union(mutation.removes)
            instance.savedRemovalGuards = instance.savedRemovalGuards.filter { $0.value.names.isDisjoint(with: touched) }
            let after = await instance.engine.pendingRecordZoneChanges()
            guard await valid(origin) else { return }
            let added = mutation.adds.keys.filter { after[$0] == mutation.adds[$0] }
            #if DEBUG
            if let hook = pendingUpdateValidatedHook, hook.afterSDKMutation { await hook.action() }
            #endif
            do { try await core.updateApplied(sdk: origin, id: update.id, added: added) }
            catch BridgeError.cloudSDKCallbackRejected { retireRejectedOriginal(origin); return }
        }
    }
    // Hold the SDK's original serial delivery, retaining asset lifetime and
    // order. No event queue grows while Core is suspended.
    public func freezeForSuspend() async throws {
        suspensionEpoch &+= 1
        suspending = true
        for instance in owned.values {
            instance.lossAccountTask?.task.cancel(); instance.lossReadTask?.cancel()
            instance.rootReadHandle?.cancel(); instance.rootReadAdmission?.setOpen(false); instance.rootReadOperation?.cancel()
            instance.cancelRootReadIdentity()
        }
        for resolution in identityResolutions.values { resolution.cancel() }
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
    fileprivate func handleEvent(_ event: CloudEngineEvent, origin: String, engine: any CloudKitEngine) async {
        guard let instance = owned[origin] else { return }
        // Pair even a refused delivery with its original return notification.
        // Other event kinds cannot justify waiting for send admission.
        if case .sent = event { instance.sentHandlers += 1 }
        guard !suspended, await valid(origin), owned[origin] === instance else { return }
        instance.observationOrdinal += 1
        let ordinal = instance.observationOrdinal
        await observeEvent?(.init(origin: origin, ordinal: ordinal, event: event, phase: .delivered))
        guard await valid(origin), owned[origin] === instance else { return }
        #if DEBUG
        if let hook = validatedEventHook, hook.event == event { await hook.action() }
        #endif
        do {
            switch event {
            case .deliveryFailed:
                try await core.trackingFailure(sdk: origin)
                instance.failed = true
                #if DEBUG
                instance.recordDeliveryFailure()
                #endif
            case .databaseChanges: break
            case .fetchedCollectionZoneDeletions(let names):
                try await core.fetchedCollectionZoneDeletions(sdk: origin, names: names)
            case .fetchedZoneLosses(let losses):
                instance.mediaLossObserved = losses.contains { $0.zone == "meguri-media" } || instance.mediaLossObserved
                if try await core.fetchedZoneLosses(sdk: origin, losses: losses) { instance.stagedZoneLoss() }
            case .stateUpdate(let state):
                if !instance.zoneLossPending { try await core.stateUpdate(sdk: origin, state: state) }
            case .fetched(let zone, let records, let deleted):
                try await core.fetched(sdk: origin, zone: zone, records: records, deleted: deleted)
                if zone == "meguri-root", deleted.contains("active") { instance.stagedZoneLoss() }
                #if DEBUG
                // Original admission guards and successful intake precede this
                // process total; it establishes no current-owner/convergence proof.
                if zone == instance.zone {
                    DebugCloudDiagnostics.fetchRecorder.record(.coreC, count: UInt64(records.count))
                }
                #endif
            case .sent(let id, let saved, let failed, let deleted, let failedDeletes):
                let removalGuard = try await core.currentGuard()
                guard await valid(origin), owned[origin] === instance else { return }
                let actualSaved = try await core.sent(sdk: origin, batchID: id, saved: saved, failed: failed, deleted: deleted, failedDeletes: failedDeletes, batchCount: instance.batches[id] ?? UInt32(saved.count + failed.count + deleted.count + failedDeletes.count))
                if !saved.isEmpty || !deleted.isEmpty { instance.savedRemovalGuards[id] = (removalGuard, Set(saved.map(\.name) + deleted)) }
                instance.operationSaved += actualSaved
                if (failed + failedDeletes).contains(where: { Self.lossCause($0.code) != nil }) { instance.stagedZoneLoss() }
                instance.mediaLossObserved = (failed + failedDeletes).contains { $0.zone == "meguri-media" && Self.lossCause($0.code) != nil } || instance.mediaLossObserved
                let receipt = CloudMediaSendReceipt(failed: failed, failedDeletes: failedDeletes)
                if let diagnostic = receipt.diagnostic {
                    lastFailure = diagnostic
                    #if DEBUG
                    instance.recordFailure(.handleEvent, error: diagnostic)
                    #endif
                }
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
            case .willFetch(let group, let reason):
                repeat {
                    guard await valid(origin), owned[origin] === instance else { throw CancellationError() }
                    #if DEBUG
                    await fetchReservationObserver?()
                    #endif
                    if epochReadOrigin == origin {
                        instance.blockedFetchCallbacks += 1; changed(readiness: false)
                        defer { instance.blockedFetchCallbacks -= 1; changed(readiness: false) }
                        while epochReadOrigin == origin { let old = version; try await waitForChange(after: old) }
                    } else {
                        // No suspension between this last ownership check and acquisition.
                        instance.sdkFetchActive = true; changed()
                        break
                    }
                } while true
                try await core.willFetch(sdk: origin, group: group, manual: reason == .manual)
            case .willFetchZone(let zone): try await core.fetchZone(sdk: origin, zone: zone, will: true, failed: false)
            case .didFetchZone(let zone, let error):
                try await core.fetchZone(sdk: origin, zone: zone, will: false, failed: error != nil)
                if let error, let cause = Self.lossCause(error) {
                    if try await core.fetchedZoneLosses(sdk: origin, losses: [.init(zone: zone, cause: cause)]) { instance.stagedZoneLoss() }
                }
            case .didFetch(let group, let reason):
                defer { instance.sdkFetchActive = false; changed() }
                try await core.didFetch(sdk: origin, group: group, manual: reason == .manual)
            case .willSend, .didSend: break
            }
            guard await valid(origin) else { return }
            if let observeEvent {
                let pending: Set<String>?
                if case .stateUpdate = event { pending = try await engine.pendingNamesForStateEvidence() }
                else { pending = nil }
                guard await valid(origin), owned[origin] === instance else { return }
                await observeEvent(.init(origin: origin, ordinal: ordinal, event: event, phase: .committed, pendingNames: pending))
            }
            guard await valid(origin), owned[origin] === instance else { return }
            await eventDidPersist(event)
            guard await valid(origin) else { return }
            // No deferred list is applied here. The immutable callback origin
            // survives this suspension and is checked again at commit/return.
        } catch BridgeError.cloudSDKCallbackRejected {
            // Authority can end during the async Core call after valid(). Core
            // refused this old callback; retain any earlier real failure.
        } catch {
            lastFailure = error
            instance.failed = true
            #if DEBUG
            instance.recordFailure(.handleEvent, error: error)
            #endif
        }
    }
    fileprivate func eventHandlerReturned(_ event: CloudEngineEvent, origin: String) async {
        guard let instance = owned[origin] else { return }
        if case .sent = event, await valid(origin), !instance.failed {
            do {
                try await core.handlerReturned(sdk: origin)
                let receipts = await instance.engine.takeSavedPendingRemovals()
                await savedPendingRemovalsConfirmed(receipts, origin: origin, engine: instance.engine)
            }
            catch BridgeError.cloudSDKCallbackRejected { }
            catch {
                lastFailure = error; instance.failed = true
                #if DEBUG
                instance.recordFailure(.handlerReturned, error: error)
                #endif
            }
        }
        if !instance.failed, await valid(origin), owned[origin] === instance {
            await observeEvent?(.init(origin: origin, ordinal: instance.observationOrdinal, event: event, phase: .returned))
        }
        if case .sent = event {
            instance.sentHandlers = max(0, instance.sentHandlers - 1)
            instance.sentHandlerReturns &+= 1
        }
        instance.handlers = max(0, instance.handlers - 1)
        changed()
        let live = await valid(origin)
        if instance.failed || !live {
            // This owned task starts only after the forwarding handler returned;
            // cancellation may need the SDK call that delivered us to unwind.
            _ = beginStop(origin, reason: instance.failed ? .handlerFailed : .handlerInvalid, policyStop: false)
        } else {
            #if DEBUG
            // The existing public drain selects current, which may have changed
            // while this original callback was suspended. Attribute its error
            // only to the instance whose updates it actually consumes.
            let pendingInstance = current.flatMap { owned[$0] }
            #endif
            do { try await consumePendingUpdates() }
            catch { lastFailure = error; instance.failed = true
                #if DEBUG
                pendingInstance?.recordFailure(.postReturnUpdates, error: error)
                #endif
                _ = beginStop(origin, reason: .postReturnUpdatesFailed, policyStop: false)
            }
        }
    }
    fileprivate func savedPendingRemovalsConfirmed(_ receipts: [CloudSavedPendingRemoval], origin: String,
                                                   engine: any CloudKitEngine, retireOnRejection: Bool = false) async {
        guard let instance = owned[origin], !instance.failed, engine.instanceID == origin,
              (instance.engine as AnyObject) === (engine as AnyObject) else { return }
        for receipt in receipts {
            guard receipt.instanceID == origin, !receipt.names.isEmpty,
                  let original = instance.savedRemovalGuards[receipt.requestID], receipt.names == original.names,
                  await valid(origin), owned[origin] === instance, !instance.failed else { continue }
            do {
                guard try await core.currentGuard() == original.guardValue,
                      await valid(origin), owned[origin] === instance else { continue }
                try await core.savedPendingRemoved(receipt)
                instance.savedRemovalGuards.removeValue(forKey: receipt.requestID)
            } catch BridgeError.cloudSDKCallbackRejected {
                // Keep the receipt unconfirmed; the original SDK no longer owns it.
                // Standalone pending drains have no later event-return cleanup.
                if retireOnRejection { retireRejectedOriginal(origin) }
            } catch {
                lastFailure = error; instance.failed = true
                #if DEBUG
                instance.recordFailure(.savedPendingRemovals, error: error)
                #endif
            }
        }
    }

    // Runs only after the original engine has updated its event slot/lifetime.
    // This notification only marks/enqueues; returnedWork must never join us.
    fileprivate func eventBookkeepingCompleted(_ event: CloudEngineEvent, origin: String) {
        if let instance = owned[origin], instance.zoneLossPending, !instance.stopping, !instance.failed {
            Task { await self.requestLossAccountRead(origin: origin) }
        }
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
            while epochReadOrigin == origin { let old = version; try await waitForChange(after: old) }
            guard assetReadLive(origin, instance: instance) else { throw CancellationError() }
            instance.assetReadActive = true; changed(readiness: false)
            defer { instance.assetReadActive = false; changed(readiness: false) }
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
        guard await valid(origin), let instance = owned[origin], !instance.zoneLossPending, (zone == instance.zone || zone == "meguri-media") else { return nil }
        await delegateWorkValidated()
        guard !quiescing, !suspended, !suspending, current == origin, owned[origin] === instance, !instance.stopping else { return nil }
        #if DEBUG
        var failurePhase = FailurePhase.nextBatchPreparation
        #endif
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
            #if DEBUG
            failurePhase = .nextBatchBuild
            #endif
            let batch = try await core.buildBatch(sdk: origin, zone: zone, pending: pending)
            guard await valid(origin), !quiescing, !suspended, !suspending else { return nil }
            if let batch {
                instance.batches[batch.id] = UInt32(batch.saves.count + batch.deletes.count)
                await observeBatch?(origin, zone, batch)
                guard await valid(origin), owned[origin] === instance, !quiescing, !suspended, !suspending else { return nil }
            }
            do { try batch?.finalAdmission?() } catch { return nil }
            return batch
        } catch BridgeError.cloudSDKCallbackRejected {
            retireRejectedOriginal(origin)
            return nil
        } catch {
            lastFailure = error; instance.failed = true
            #if DEBUG
            instance.recordFailure(failurePhase, error: error)
            #endif
            _ = beginStop(origin, reason: .nextBatchFailed, policyStop: false); return nil
        }
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
    private func checkMediaSendOwner(_ origin: String, instance: Instance) throws {
        try Task.checkCancellation()
        guard !quiescing, !suspended, !suspending else { throw CancellationError() }
        guard current == origin, owned[origin] === instance, !instance.stopping, !instance.failed else {
            throw BridgeError.cloudPolicy(.staleCommand)
        }
    }
    private func finishSend(origin: String, call: UInt64, pending: [String: CloudPendingChange]) async throws {
        try await core.finishSend(sdk: origin, id: call, pending: pending)
        #if DEBUG
        sendCallObserver?(origin, call, true)
        #endif
    }
    private func send(zones: [String], origin: String, execution: CoreCloudPolicyExecution? = nil,
                      requireMediaAdmission: Bool = false) async throws {
        #if DEBUG
        let diagnostic = DebugCloudDiagnostics.sendFailureCapture
        let mediaSendPhase = DebugCloudDiagnostics.mediaSendPhase
        var site = DebugCloudDiagnostics.SendFailureSite.sendAdmission
        #endif
        guard !quiescing, !suspended, !suspending else {
            #if DEBUG
            diagnostic?.record(.sendAdmission, error: CancellationError())
            #endif
            throw CancellationError()
        }
        guard let instance = owned[origin], await valid(origin) else {
            if execution != nil {
                #if DEBUG
                diagnostic?.record(.sendAdmission, error: BridgeError.cloudPolicy(.staleCommand))
                #endif
                throw BridgeError.cloudPolicy(.staleCommand)
            }; return
        }
        #if DEBUG
        do { try await consumePendingUpdates(origin: origin) }
        catch { diagnostic?.record(.pendingUpdateApplication, error: error); throw error }
        #else
        try await consumePendingUpdates(origin: origin)
        #endif
        guard await valid(origin) else {
            #if DEBUG
            diagnostic?.record(.sendAdmission, error: BridgeError.cloudPolicy(.staleCommand))
            #endif
            throw BridgeError.cloudPolicy(.staleCommand)
        }
        var call: UInt64?
        var finalizationAttempted = false
        var networkStarted = false
        // Keep the original network lifetime through exact-call cleanup.
        defer { if networkStarted { instance.networks -= 1; changed() } }
        var operationError: (any Error)?
        sendWork: do {
            repeat {
                if requireMediaAdmission { try checkMediaSendOwner(origin, instance: instance) }
                let returned = instance.sentHandlerReturns
                #if DEBUG
                site = .beginSend
                #endif
                if let execution { call = try await execution.beginSend() }
                else { call = try await core.beginSend(sdk: origin) }
                #if DEBUG
                if let call { sendCallObserver?(origin, call, false) }
                site = .admissionValidation
                #endif
                if requireMediaAdmission { try checkMediaSendOwner(origin, instance: instance) }
                #if DEBUG
                await sendAdmissionObserver?(call != nil)
                #endif
                if requireMediaAdmission { try checkMediaSendOwner(origin, instance: instance) }
                guard requireMediaAdmission else { break }
                if call != nil { break }
                guard let execution else { throw BridgeError.cloudPolicy(.staleCommand) }
                try await execution.validate()
                try checkMediaSendOwner(origin, instance: instance)
                // Manual requests obey the sync gate, exchange state and eight-call
                // exchange budget, which also counts calls that have returned.
                // Refused admission starts no SDK operation. A sent handler's return
                // permits an admission retry; without an active or newly returned
                // sent handler, retain the upload and report busy.
                guard instance.sentHandlers > 0 || instance.sentHandlerReturns != returned else {
                    throw CloudMediaFailure.busy
                }
                while instance.sentHandlers > 0 {
                    #if DEBUG
                    mediaSendWaits += 1; changed()
                    defer { mediaSendWaits -= 1; changed() }
                    #endif
                    let old = version
                    try await waitForChange(after: old)
                    try checkMediaSendOwner(origin, instance: instance)
                    try await execution.validate()
                    try checkMediaSendOwner(origin, instance: instance)
                }
                // Return can race beginSend, its observer or waiter registration.
                // The retained return counter and versioned waiter cover all three.
                // Only recheck admission after an actual handler return; unrelated
                // changed() notifications above never cause another beginSend.
                // prepareSend still joins SDK event bookkeeping before enqueue.
            } while true
            guard let call else { break sendWork }
            #if DEBUG
            site = .pendingUpdateApplication
            #endif
            if zones.contains("meguri-media") { try await consumePendingUpdates(origin: origin) }
            guard await valid(origin) else { break sendWork }
            instance.networks += 1; networkStarted = true; changed()
            let mediaScope = zones.contains("meguri-media")
            let reportedZones: [CloudPolicyZone] = mediaScope ? [.media] : [.collection]
            var sendError: (any Error)?
            let invocationOutputs = CloudPolicyInvocationOutputs()
            do {
                #if DEBUG
                site = .sendValidation
                #endif
                if let execution { try await execution.validate() }
                guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
                instance.operationSaved = 0
                instance.mediaSendFailure = nil
                #if DEBUG
                site = .sdkAuthorization
                #endif
                let validateSDK = try await core.finalSDKAuthorization(origin, media: mediaScope)
                #if DEBUG
                site = .sdkPreparation
                #endif
                let prepared = try await instance.engine.prepareSend(zones: zones.filter { $0 == instance.zone || $0 == "meguri-media" })
                #if DEBUG
                site = .sendValidation
                #endif
                guard current == origin, owned[origin] === instance, !instance.stopping else { throw BridgeError.cloudPolicy(.staleCommand) }
                guard !quiescing else { throw CancellationError() }
                #if DEBUG
                site = .enqueue
                #endif
                let operation = try prepared.enqueue(prepare: {
                    #if DEBUG
                    do {
                        if let execution { invocationOutputs.append(try await execution.reportInvocation(zones: reportedZones, kind: .send)) }
                    } catch { diagnostic?.record(.invocationReport, error: error, mediaSendPhase: mediaSendPhase); throw error }
                    #else
                    if let execution { invocationOutputs.append(try await execution.reportInvocation(zones: reportedZones, kind: .send)) }
                    #endif
                }, validate: {
                    #if DEBUG
                    do { try validateSDK() }
                    catch { diagnostic?.record(.enqueueSDKAuthorization, error: error, mediaSendPhase: mediaSendPhase); throw error }
                    do { try execution?.validateForEnqueue() }
                    catch { diagnostic?.record(.enqueueExecutionValidation, error: error, mediaSendPhase: mediaSendPhase); throw error }
                    #else
                    try validateSDK()
                    try execution?.validateForEnqueue()
                    #endif
                })
                #if DEBUG
                site = .drain
                #endif
                try await operation.drain()
                #if DEBUG
                site = .postDrainValidation
                #endif
                if let execution { try await execution.validate() }
            }
            catch {
                CloudMediaCancellationContext.current?.observe(error)
                sendError = error
                #if canImport(CloudKit)
                let losses = try NativeCloudKitSendError.zoneLosses(error, zones: zones.filter { $0 == instance.zone || $0 == "meguri-media" })
                if !losses.isEmpty, await valid(origin) {
                    instance.mediaLossObserved = losses.contains { $0.zone == "meguri-media" } || instance.mediaLossObserved
                    do {
                        if try await core.fetchedZoneLosses(sdk: origin, losses: losses) { instance.stagedZoneLoss() }
                    } catch BridgeError.cloudSDKCallbackRejected {
                        // Preserve sendError: this rejects only the old loss report.
                    }
                    await requestLossAccountRead(origin: origin)
                }
                #endif
                #if DEBUG
                diagnostic?.record(site, error: error)
                if CloudMediaCancellationContext.current != nil, let action = CloudMediaCancellationContext.selectedAction {
                    DebugCloudDiagnostics.record(.policyExecute, action: action, error: error)
                }
                await sendErrorObserver?()
                #endif
            }
            policyOperationOutputs.append(contentsOf: invocationOutputs.take())

            if let failure = instance.mediaSendFailure {
                CloudMediaCancellationContext.current?.observe(failure)
                if sendError == nil {
                    #if DEBUG
                    diagnostic?.record(.completion, error: failure)
                    #endif
                    sendError = failure
                }
            }
            // SDK delivery has returned. Join its original first-read/apply work
            // before finishSend observes pending changes and decides another round.
            await joinOriginalFirstAssetReads(instance)
            if sendError == nil, Task.isCancelled {
                let cancellation = CancellationError()
                CloudMediaCancellationContext.current?.observe(cancellation)
                #if DEBUG
                diagnostic?.record(.completion, error: cancellation)
                #endif
                sendError = cancellation
            }
            if let sendError { operationError = sendError }
            #if DEBUG
            site = .completion
            #endif
            if await valid(origin) {
                try await reportOperation(execution, zones: reportedZones, kind: .send, started: false, error: sendError, saved: instance.operationSaved)
            }
            guard await valid(origin) else { if let sendError { throw sendError }; break sendWork }
            #if DEBUG
            site = .deferredUpdates
            #endif
            try await waitForDeferredUpdates(origin: origin)
            #if DEBUG
            site = .sendValidation
            #endif
            if let execution { try await execution.validate() }
            guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
            #if DEBUG
            site = .deferredUpdates
            #endif
            try await waitForDeferredUpdates(origin: origin)
            #if DEBUG
            site = .sendValidation
            #endif
            if let execution { try await execution.validate() }
            guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
            let pending = await instance.engine.pendingRecordZoneChanges()
            guard await valid(origin) else { break sendWork }
            finalizationAttempted = true
            #if DEBUG
            site = .finish
            #endif
            try await finishSend(origin: origin, call: call, pending: pending)
            #if DEBUG
            site = .pendingUpdateApplication
            #endif
            try await consumePendingUpdates(origin: origin)
            if let sendError { throw sendError }
        } catch {
            #if DEBUG
            diagnostic?.record(site, error: error)
            #endif
            CloudMediaCancellationContext.current?.observe(error)
            if operationError == nil { operationError = error }
        }
        // Admission owns only this original call, even when later work returns
        // early or throws. Normal finalization consumes its single attempt too.
        if requireMediaAdmission, let call, !finalizationAttempted {
            finalizationAttempted = true
            let pending = await instance.engine.pendingRecordZoneChanges()
            do { try await finishSend(origin: origin, call: call, pending: pending) }
            catch {
                #if DEBUG
                diagnostic?.record(.finish, error: error)
                #endif
                CloudMediaCancellationContext.current?.observe(error)
                lastFailure = error
                if operationError == nil { operationError = error }
            }
        }
        if let operationError { throw operationError }
    }
    private func fetch(zones: [String], origin pinnedOrigin: String? = nil, execution: CoreCloudPolicyExecution? = nil, reservedFetch: Bool = false, proofIssuer: CloudFetchProofIssuer? = nil) async throws -> Bool {
        guard !quiescing, !suspended, !suspending else { throw CancellationError() }
        let requestedOrigin = pinnedOrigin ?? current
        if !reservedFetch, fetchBusy {
            // Join actual native/removal bookkeeping from earlier stages, then
            // ACK their outputs before offering this exact dependency park.
            if let origin = requestedOrigin, let instance = owned[origin] {
                try await instance.engine.waitForEventBookkeeping()
                try await consumePendingUpdates(origin: origin)
                try await instance.engine.waitForEventBookkeeping()
            }
            let outputs = takePolicyOperationOutputs()
            if let acknowledge = CloudPolicyOutputAcknowledgement.current ?? proofOutputAcknowledgement {
                await acknowledge(outputs)
            } else if !outputs.isEmpty {
                policyOperationOutputs.insert(contentsOf: outputs, at: 0)
                throw BridgeError.unavailable(reason: "Cloud outputs require original acknowledgement")
            }
            _ = try await waitForQuiescentFetchDependency()
        }
        guard !quiescing else { throw CancellationError() }
        guard let origin = requestedOrigin, let instance = owned[origin], await valid(origin) else {
            if execution != nil { throw BridgeError.cloudPolicy(.staleCommand) }; return false
        }
        while epochReadOrigin == origin { let old = version; try await waitForChange(after: old) }
        guard current == origin, owned[origin] === instance, !instance.stopping else { throw CancellationError() }
        fetchBusy = true; instance.networks += 1; changed()
        defer { if !reservedFetch { fetchBusy = false }; instance.sdkFetchActive = false; instance.networks -= 1; changed() }
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
                let operation = try prepared.enqueue(prepare: {
                    if let execution { invocationOutputs.append(try await execution.reportInvocation(zones: [.root, .collection], kind: .fetch)) }
                }, validate: {
                    try validateSDK()
                    try execution?.validateForEnqueue()
                })
                try await operation.drain()
                if let execution { try await execution.validate() }
            }
            catch {
                #if canImport(CloudKit)
                let losses = try NativeCloudKitSendError.zoneLosses(error, zones: scope)
                if !losses.isEmpty, await valid(origin) {
                    instance.mediaLossObserved = losses.contains { $0.zone == "meguri-media" } || instance.mediaLossObserved
                    do {
                        if try await core.fetchedZoneLosses(sdk: origin, losses: losses) { instance.stagedZoneLoss() }
                    } catch BridgeError.cloudSDKCallbackRejected {
                        // The original failed SDK operation still throws below.
                    }
                    await requestLossAccountRead(origin: origin)
                }
                #endif
                policyOperationOutputs.append(contentsOf: invocationOutputs.take())
                if await valid(origin) {
                    _ = try await core.finishFetch(sdk: origin, id: id, successful: false)
                    try await reportOperation(execution, zones: [.root, .collection], kind: .fetch, started: false, error: error)
                }
                throw error
            }
            guard await valid(origin) else { return false }
            if instance.zoneLossPending { return false }
            policyOperationOutputs.append(contentsOf: invocationOutputs.take())
            let plan: CloudFetchReadPlan
            let drained: Bool
            do {
                plan = try await core.fetchReadPlan(sdk: origin, id: id)
                // All original fetch callbacks have returned. Keep fetchBusy held
                // through inference so another manual fetch cannot pass these reads.
                if let transport = assetReadTransport {
                    for read in plan.reads {
                        guard await valid(origin), !quiescing, !suspended, !suspending else { return false }
                        let next = try await performFetchRead(read, transport: transport, origin: origin, instance: instance, execution: execution, proofIssuer: proofIssuer)
                        if let next {
                            _ = try await performFetchRead(next, transport: transport, origin: origin, instance: instance, execution: execution, proofIssuer: proofIssuer)
                        }
                    }
                }
                guard await valid(origin), !quiescing, !suspended, !suspending else { return false }
                drained = try await core.finishFetch(sdk: origin, id: id)
                try await reportOperation(execution, zones: [.root, .collection], kind: .fetch, started: false)
            } catch {
                let proof = error as? CloudFetchProofError
                do {
                    if await valid(origin) {
                        _ = try await core.finishFetch(sdk: origin, id: id, successful: false)
                        try await reportOperation(execution, zones: [.root, .collection], kind: .fetch, started: false, error: proof?.error ?? error)
                    }
                } catch {
                    if let proof {
                        throw CloudFetchProofError(outcome: .init(completion: proof.outcome.completion,
                            issuer: proof.outcome.issuer, original: proof.outcome.original,
                            earlierFailure: proof.outcome.earlierFailure, cleanupFailure: error,
                            callerCancelled: Task.isCancelled))
                    }
                    throw error
                }
                throw error
            }
            // Post-API finalization and own reads can emit RootConfirmed's full
            // list after the last callback drain. Apply and ACK this owner's
            // lists before returning, without finalizing the fetch a second time.
            guard await valid(origin), owned[origin] === instance, !quiescing, !suspended, !suspending else { return false }
            try Task.checkCancellation()
            try await waitForDeferredUpdates(origin: origin)
            guard await valid(origin), owned[origin] === instance, !quiescing, !suspended, !suspending else { return false }
            try Task.checkCancellation()
            // P4's repeat repairs a joined/short-circuited call only. An own
            // qualified call whose read failed waits for the next event trigger.
            if plan.qualified { return drained }
        }
        return false
    }
    private func performFetchRead(_ read: CoreCloudFetchRead, transport: any CloudOwnTransport,
                                  origin: String, instance: Instance,
                                  execution: CoreCloudPolicyExecution?, proofIssuer: CloudFetchProofIssuer? = nil,
                                  epochPreparation: Bool = false) async throws -> CoreCloudFetchRead? {
        try Task.checkCancellation()
        guard await valid(origin), !quiescing, !suspended, !suspending,
              current == origin, owned[origin] === instance, !instance.stopping else { throw CancellationError() }
        if !epochPreparation {
            while instance.sdkFetchActive || epochReadOrigin != nil || instance.assetReadActive {
                let old = version; try await waitForChange(after: old)
            }
            guard current == origin, owned[origin] === instance, !instance.stopping else { throw CancellationError() }
            epochReadOrigin = origin; changed(readiness: false)
        }
        defer { if !epochPreparation && epochReadOrigin == origin { epochReadOrigin = nil; changed(readiness: false) } }
        let admission = CloudOwnWorkAdmission()
        instance.rootReadAdmission = admission; instance.rootReadHandle = read
        defer { instance.rootReadAdmission = nil; instance.rootReadHandle = nil }
        let target = try await read.target()
        let validateRead = read.finalAuthorization()
        if let execution { try await execution.validate() }
        guard await valid(origin), !quiescing, !suspended, !suspending,
              current == origin, owned[origin] === instance, !instance.stopping else { throw CancellationError() }
        let operation = try transport.prepare(.read(target))
        instance.rootReadOperation = operation
        defer { instance.rootReadOperation = nil }
        var transportError: (any Error)?
        var nativeFailure: CloudFetchReadNativeFailure?
        let result: CloudControlRead
        do {
            let context = CloudMediaCancellationContext.current
            #if DEBUG
            let selectedAction = CloudMediaCancellationContext.selectedAction
            let maintenanceCapture = MaintenanceCallObservation.capture
            let beforeValidation = CloudFetchReadWorkObservation.beforeValidation
            #endif
            // Own and join the off-actor handoff. Blocking maintenance must
            // leave the adapter free to cancel this exact read on control.
            // Only original Sendable capabilities cross into this worker.
            let handoff: @Sendable () throws -> Void = {
                try Task.checkCancellation()
                try validateRead(); try execution?.validateForEnqueue()
                try Task.checkCancellation()
                // No application await after final validation; the native
                // mutex contains only admission bookkeeping and queue add.
                try admission.withPermission { allowed in
                    guard allowed else { throw CancellationError() }
                    guard operation.enqueue() else { throw CloudOwnTransportFailure.queueRejected }
                }
            }
            let worker = Task.detached(priority: Task.currentPriority) {
                try CloudMediaCancellationContext.$current.withValue(context) {
                    #if DEBUG
                    try CloudMediaCancellationContext.$selectedAction.withValue(selectedAction) {
                        try MaintenanceCallObservation.$capture.withValue(maintenanceCapture) {
                            try CloudFetchReadWorkObservation.$beforeValidation.withValue(beforeValidation) {
                                try handoff()
                            }
                        }
                    }
                    #else
                    try handoff()
                    #endif
                }
            }
            result = try await withTaskCancellationHandler {
                try await worker.value
                // Even cancellation after enqueue joins the original native
                // callback before either read ownership or its SDK is released.
                let reply: CloudOwnTransportReply
                do { reply = try await operation.reply() }
                catch {
                    // Only the original native reply's explicit shape refusal
                    // supplies this fact. Core still decides its acceptance.
                    if epochPreparation, let native = error as? NativeCloudKitFailure,
                       case .unsupportedRecord = native { nativeFailure = .malformedRecord }
                    throw error
                }
                guard case .read(let result) = reply else { throw CloudOwnTransportFailure.unexpectedReply }
                return result
            } onCancel: { read.cancel(); admission.setOpen(false); operation.cancel(); worker.cancel() }
        } catch {
            operation.cancel()
            transportError = error
            // A throwing original read is a failure, never a missing record.
            result = .failed
        }
        let evidence = operation.controlReadEvidence() ?? transportError.map {
            CloudZoneDeletionRead(read: .failed, terminal: .evidence($0))
        }
        var live = await valid(origin) && !quiescing && !suspended && !suspending && !Task.isCancelled
        let mustRefresh = target.name == "active" || result == .missing || result == .zoneMissing
            || [evidence?.failure?.code, evidence?.terminal?.code].contains(28)
            || evidence?.failure?.encryptedDataReset == true || evidence?.terminal?.encryptedDataReset == true
        if live && mustRefresh {
            do { try await refreshFetchReadIdentity(instance: instance, issuer: proofIssuer) }
            catch {
                let proof = error as? CloudFetchProofError
                let originalError = proof?.error ?? error
                #if DEBUG
                DebugCloudDiagnostics.fetchReadFailureCapture?.record(
                    targetActive: target.name == "active", transportError: transportError != nil,
                    resultFailed: result == .failed, evidenceFailure: evidence?.failure != nil,
                    evidenceTerminal: evidence?.terminal != nil, taskCancelled: Task.isCancelled,
                    stopping: instance.stopping, policyStop: instance.firstStopReason == .policyStop,
                    retirementFailure: instance.retirementFailure != nil,
                    accountRetired: instance.stopWitness.accountRetired, cancellationError: originalError is CancellationError)
                #endif
                // Earlier native failures outrank the proof's cancellation. Keep
                // the SDK numeric facts even after account authority is gone.
                let earlierFailure: (any Error)?
                if let transportError { earlierFailure = transportError }
                else if result == .failed || evidence?.failure != nil || evidence?.terminal != nil {
                    if let facts = evidence?.failure ?? evidence?.terminal, let code = facts.code {
                        earlierFailure = CloudKitSDKOperationFailure(underlying: CloudOwnTransportFailure.missingReply,
                            code: code, retryAfterMilliseconds: facts.retryAfterMilliseconds)
                    } else { earlierFailure = CloudOwnTransportFailure.missingReply }
                } else { earlierFailure = nil }
                var cleanupFailure: (any Error)?
                do {
                    _ = try await read.complete(result, discard: true, evidence: evidence, nativeFailure: nativeFailure)
                    #if DEBUG
                    try CloudFetchProofCompletionFault.afterOriginalDiscard?()
                    #endif
                }
                catch { cleanupFailure = error }
                if let proof, proofIssuer != nil {
                    throw CloudFetchProofError(outcome: .init(completion: proof.outcome.completion,
                        issuer: proof.outcome.issuer, original: proof.outcome.original,
                        earlierFailure: earlierFailure, cleanupFailure: cleanupFailure,
                        callerCancelled: Task.isCancelled || proof.outcome.callerCancelled))
                }
                throw earlierFailure ?? originalError
            }
        }
        let stillCurrent = await valid(origin)
        live = live && stillCurrent && !quiescing && !suspended && !suspending && !Task.isCancelled
        if let execution, live {
            do { try await execution.validate() }
            catch { _ = try? await read.complete(result, discard: true, evidence: evidence, nativeFailure: nativeFailure); throw error }
        }
        let next = try await read.complete(result, discard: !live, evidence: evidence, nativeFailure: nativeFailure)
        #if DEBUG
        if epochPreparation { await epochAcceptanceObserver?() }
        #endif
        if let transportError { throw transportError }
        if result == .failed, live {
            if let facts = evidence?.failure ?? evidence?.terminal, let code = facts.code {
                throw CloudKitSDKOperationFailure(underlying: CloudOwnTransportFailure.missingReply,
                    code: code, retryAfterMilliseconds: facts.retryAfterMilliseconds)
            }
            throw CloudOwnTransportFailure.missingReply
        }
        return next
    }

    private static func lossCause(_ code: CloudEngineErrorCode) -> CloudZoneLossCause? {
        switch code {
        case .zoneNotFound: .deleted
        case .userDeletedZone: .purged
        case .encryptedDataReset: .encryptedDataReset
        default: nil
        }
    }
    private func requestLossAccountRead(origin: String) async {
        guard let instance = owned[origin], instance.zoneLossPending,
              !instance.stopping, !quiescing, !suspended, !suspending,
              instance.lossAccountGeneration != instance.lossEvidenceGeneration else { return }
        // Only a newly staged Core loss requires another proof. Cursor and
        // callback bookkeeping retain the exact read already serving this loss.
        let generation = instance.lossEvidenceGeneration
        instance.lossAccountGeneration = generation
        if instance.mediaLossObserved, instance.lossMediaDrainTask == nil {
            instance.networks += 1
            instance.lossMediaDrainTask = Task.detached(priority: Task.currentPriority) {
                await self.drainLossMedia(origin: origin)
            }
        }
        instance.lossAccountRequested = true; changed()
        if appOwnsAccountObservation { accountReadPending = true; await accountReadRequested() }
        else {
            instance.lossAccountTask?.task.cancel()
            let taskKey = UUID()
            instance.networks += 1
            let task = Task.detached(priority: Task.currentPriority) {
                await self.runLossAccountRead(origin: origin, taskKey: taskKey, generation: generation)
            }
            instance.lossAccountTask = (taskKey, task)
        }
    }
    private func drainLossMedia(origin: String) async {
        guard let instance = owned[origin] else { return }
        defer { instance.networks -= 1; instance.lossMediaDrainTask = nil; changed() }
        // Let all original result/ACK bookkeeping return before using the
        // existing cancellation/drain owner. Collection SDK authority stays live.
        do { try await instance.engine.waitForEventBookkeeping() } catch { lastFailure = error }
        await originalMediaDrain?(instance.stopWitness)
    }
    private func runLossAccountRead(origin: String, taskKey: UUID, generation: UInt64) async {
        guard let instance = owned[origin] else { return }
        defer {
            instance.networks -= 1
            if instance.lossAccountTask?.id == taskKey { instance.lossAccountTask = nil; instance.lossAccountRequested = false }
            changed()
        }
        do {
            // Original SDK result handlers and their pending-removal receipts
            // finish before account proof can revoke the original authority.
            while instance.handlers > 0 || instance.applying { let old = version; try await waitForChange(after: old) }
            try await instance.engine.waitForEventBookkeeping()
            try Task.checkCancellation()
            guard owned[origin] === instance, !instance.stopping,
                  instance.lossAccountTask?.id == taskKey, instance.lossEvidenceGeneration == generation else { return }
            let result = try await refreshPlatformIdentityRead(intent: nil)
            guard instance.lossAccountTask?.id == taskKey, !Task.isCancelled,
                  owned[origin] === instance, !instance.stopping,
                  instance.lossEvidenceGeneration == generation else { return }
            instance.lossAccountRead = result.id
            await accountReadCompleted(id: result.id, available: result.available)
            if !(await valid(origin)), owned[origin] === instance,
               instance.lossAccountTask?.id == taskKey, instance.lossEvidenceGeneration == generation,
               !Task.isCancelled {
                // Keep stop ownership here without awaiting its completion.
                // The drain waits until this read releases its network count.
                _ = beginStop(origin, reason: .handlerInvalid, policyStop: false)
            }
        } catch {
            // A superseded/retired read cannot replace its successor's error.
            // Errors of the still-owned original read remain observable.
            if owned[origin] === instance, current == origin, !instance.stopping,
               instance.lossAccountTask?.id == taskKey, instance.lossEvidenceGeneration == generation {
                lastFailure = error
            }
        }
    }
    // Called only after the ordered lane's original Core-issued read completed.
    // Wake/schedule only: never join an SDK operation on that account lane.
    func accountReadBegan(id: UInt64) {
        for instance in owned.values where instance.zoneLossPending && !instance.stopping {
            instance.lossAccountRead = id
        }
    }
    func accountReadCompleted(id: UInt64, available: Bool) async {
        for (origin, instance) in owned {
            guard instance.zoneLossPending, instance.lossAccountRead == id, !instance.stopping else { continue }
            guard available, await valid(origin), owned[origin] === instance else {
                if instance.lossAccountRead == id { instance.lossAccountRequested = false; changed() }
                continue
            }
            guard instance.lossAccountRead == id else { continue }
            do {
                let read = try await core.knownLossRead(sdk: origin, identityRead: id)
                guard instance.lossAccountRead == id, owned[origin] === instance, !instance.stopping else { continue }
                if let read {
                    guard instance.lossReadTask == nil else { continue }
                    instance.lossReadTask = Task { await self.runKnownLossRead(read, origin: origin, instance: instance) }
                } else { instance.zoneLossPending = false; changed() }
            } catch {
                guard instance.lossAccountRead == id, owned[origin] === instance else { continue }
                lastFailure = error
            }
            instance.lossAccountRequested = false; changed()
        }
    }
    private func runKnownLossRead(_ read: CoreCloudFetchRead, origin: String, instance: Instance) async {
        var endedWithoutAcceptance = false
        do {
            while fetchBusy { let old = version; try await waitForChange(after: old) }
            guard await valid(origin), !quiescing, !suspended, !suspending, !instance.stopping,
                  let transport = assetReadTransport else { throw CancellationError() }
            fetchBusy = true; instance.networks += 1; changed()
            do {
                defer { fetchBusy = false; instance.networks -= 1; changed() }
                _ = try await performFetchRead(read, transport: transport, origin: origin, instance: instance, execution: nil)
            }
        } catch { lastFailure = error; endedWithoutAcceptance = true }
        instance.lossReadTask = nil; changed()
        // No self-join: the read has released both network counters first.
        var mustStop = endedWithoutAcceptance
        if !mustStop { mustStop = !(await valid(origin)) }
        if mustStop {
            if let stop = beginStop(origin, reason: .handlerInvalid, policyStop: false) {
                do { try await stop.value } catch { lastFailure = error }
            }
        }
        // After joining the read and SDK shutdown, refresh isolation and status
        // on the ordered input lane.
        await assetReadDidFinish()
    }
    private func waitForReadProofBookkeeping(_ instance: Instance) async throws {
        guard epochReadOrigin == instance.engine.instanceID else {
            try await instance.engine.waitForEventBookkeeping(); return
        }
        let engine = instance.engine
        let origin = engine.instanceID
        try await withThrowingTaskGroup(of: Void.self) { group in
            group.addTask { try await engine.waitForEventBookkeeping() }
            group.addTask { try await self.waitForBlockedFetch(origin) }
            defer { group.cancelAll() }
            _ = try await group.next()
        }
    }
    private func waitForBlockedFetch(_ origin: String) async throws {
        while let instance = owned[origin], !instance.stopping, instance.blockedFetchCallbacks == 0 {
            let old = version; try await waitForChange(after: old)
        }
        try Task.checkCancellation()
    }
    #if DEBUG
    func waitForBlockedFetchForTesting(_ origin: String) async throws { try await waitForBlockedFetch(origin) }
    #endif
    public func refreshControlIdentity() async throws {
        try await refreshControlIdentity(proof: CloudOriginalCommandProofYield())
    }
    private func refreshControlIdentity(proof: CloudOriginalCommandProofYield) async throws {
        try Task.checkCancellation()
        guard !quiescing, !suspended, !suspending else { throw CancellationError() }
        if appOwnsAccountObservation {
            guard let admit = CloudOriginalCommandProofYield.admission ?? proofAdmission else {
                throw BridgeError.unavailable(reason: "Ordered cloud account admission unavailable")
            }
            // Called only after the original native read's terminal. Drain the
            // actual SDK callbacks/removal receipts before offering a proof yield.
            if let origin = current, let instance = owned[origin] {
                try await waitForReadProofBookkeeping(instance)
                try Task.checkCancellation()
                try await consumePendingUpdates(origin: origin)
                try await waitForReadProofBookkeeping(instance)
            }
            let outputs = takePolicyOperationOutputs()
            guard let acknowledge = CloudPolicyOutputAcknowledgement.current ?? proofOutputAcknowledgement else {
                guard outputs.isEmpty else {
                    policyOperationOutputs.insert(contentsOf: outputs, at: 0)
                    throw BridgeError.unavailable(reason: "Cloud outputs require original acknowledgement")
                }
                try await admit(proof)
                try Task.checkCancellation()
                return
            }
            await acknowledge(outputs)
            try Task.checkCancellation()
            try await admit(proof)
            try Task.checkCancellation()
            return
        }
        guard try await refreshPlatformIdentity(intent: nil) else {
            throw BridgeError.unavailable(reason: "Cloud account proof unavailable")
        }
    }
    #if DEBUG
    private var fetchProofCompletionObserver: (@Sendable () async -> Void)?
    func setFetchProofCompletionObserver(_ observer: (@Sendable () async -> Void)?) {
        fetchProofCompletionObserver = observer
    }
    #endif
    private func refreshFetchReadIdentity(instance: Instance, issuer: CloudFetchProofIssuer?) async throws {
        let cancellationIntent = CloudPolicyCancellationContext.current
        let wait = CloudFetchProofWait(issuer: issuer, original: instance.stopWitness, cancellationIntent: cancellationIntent)
        let proof = CloudOriginalCommandProofYield(cancellationIntent: cancellationIntent, originalRevocation: { cause in
            guard let issuer, issuer.original === wait.original else { return }
            wait.complete(.failure(CancellationError()), producer: .caller(cause))
        }, originalFailure: { error in
            wait.complete(.failure(error), producer: .provider)
        })
        let task = Task {
            let result: Result<Void, any Error>
            do {
                guard self.owned.values.contains(where: { $0 === instance }), !instance.stopping else {
                    throw CancellationError()
                }
                // This lookup is a proof read, not an engine account event;
                // do not advance the no-state point of the retained FetchRead.
                try await self.refreshControlIdentity(proof: proof)
                result = .success(())
            } catch { result = .failure(error) }
            #if DEBUG
            await CloudFetchProofCompletionFault.beforeOriginalPropagation?()
            #endif
            wait.complete(result, producer: .provider)
        }
        instance.rootReadIdentity = (task, wait)
        defer { instance.rootReadIdentity = nil }
        try await withTaskCancellationHandler {
            let completion = await wait.drain()
            #if DEBUG
            await fetchProofCompletionObserver?()
            #endif
            if case .failure = completion.result {
                throw CloudFetchProofError(outcome: .init(completion: completion, issuer: issuer,
                    original: instance.stopWitness, earlierFailure: nil, cleanupFailure: nil,
                    callerCancelled: Task.isCancelled || {
                        if case .caller = completion.producer { return true }
                        return false
                    }()))
            }
            try Task.checkCancellation()
        } onCancel: {
            let observation = cancellationIntent?.capture()
            let terminal = CloudFetchProofCompletion.Producer.caller(observation)
            wait.complete(.failure(CancellationError()), producer: terminal)
            task.cancel()
        }
    }

    public func requestFetch(zones: [String]) async throws { _ = try await fetch(zones: zones) }
    public func syncNow() async throws -> CloudExchangeResult {
        guard !quiescing else { throw CancellationError() }
        try Task.checkCancellation()
        // A policy owner reserves BEFORE facade capture. Public callers wait
        // for its actual worker and join it, never create a competing capture.
        while policyReservation != nil && exchangeTask == nil {
            if let preparation = epochPreparationTask { _ = try await preparation.value }
            let old = version
            if policyReservation != nil && exchangeTask == nil { try await waitForChange(after: old) }
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
            if quiescing {
                exchangeTask?.task.cancel()
                heldStops.append((command, execution, policy))
                return true
            }
            // Record the validated account-stop producer on the original proof
            // before cancellation can win its first-completion race.
            let stopping = original.flatMap {
                beginStop($0, reason: .policyStop, policyStop: false, execution: execution, accountStop: command)
            }
            exchangeTask?.task.cancel()
            if let stopping { try await stopping.value }
            else if original == nil { try await execution.retireOriginal() }
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
                if let original { try await stopInstance(original, reason: .policyConfigureAutomatic, policyStop: false, execution: execution) }
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
    // Call only between this command's actual completed/ACKed stages. The
    // busy owner's callbacks stay live and continue to fence its own admission.
    private struct DependencyCancellation: Error {
        let error: CancellationError
        let cause: CloudPolicyCancellationContext.Snapshot?
    }
    private func waitForQuiescentFetchDependency(issuer: CloudFetchProofIssuer? = nil) async throws -> [CloudQuiescentDependencyWait] {
        let cancellationIntent = CloudPolicyCancellationContext.current
        var joined: [CloudQuiescentDependencyWait] = []
        while fetchBusy {
            let parked = try await CloudQuiescentDependencyWait.park()
            do {
                while fetchBusy {
                    let old = version
                    try await waitForChange(after: old, cancellationError: {
                        let observation = cancellationIntent?.capture()
                        return DependencyCancellation(error: CancellationError(), cause: observation)
                    })
                }
            } catch {
                try await parked?.revoke()
                if let cancellation = error as? DependencyCancellation {
                    let original = cancellation.error
                    if let issuer, let parked {
                        throw CloudNativeCancellationError(error: original, proof:
                            .init(adapterEdge: .init(issuer: issuer, dependency: parked, cause: cancellation.cause)))
                    }
                    throw original
                }
                throw error
            }
            try await parked?.revoke()
            do { try Task.checkCancellation() }
            catch {
                if let issuer, let parked {
                    throw CloudNativeCancellationError(error: error, proof:
                        .init(adapterEdge: .init(issuer: issuer, dependency: parked)))
                }
                throw error
            }
            if let parked { joined.append(parked) }
        }
        return joined
    }
    private func reserveUnadmittedPolicyFetch(issuer: CloudFetchProofIssuer?) async throws -> [CloudQuiescentDependencyWait] {
        let joined = try await waitForQuiescentFetchDependency(issuer: issuer)
        try Task.checkCancellation()
        guard !quiescing, !suspended, !suspending else { throw CancellationError() }
        fetchBusy = true
        return joined
    }
    func executePolicyOperation(_ command: CloudPolicyCommand, policy: CoreCloudPolicyService) async throws {
        let originals = owned
        let reservedFetch: Bool
        if case .requestFetch = command.action { reservedFetch = true } else { reservedFetch = false }
        var issuer: (execution: CoreCloudPolicyExecution, instance: Instance)?
        if reservedFetch, fetchBusy {
            // Read-only issuer proof: its Core job/result/ACK and original-SDK
            // read finish before a dependent park. No fetch/admission is begun.
            guard let prepared = try await policy.prepareExecution(command) else { return }
            let origin = try await prepared.originalSDK()
            try await prepared.validate()
            if let origin, let instance = originals[origin], !instance.stopping, !instance.authorityRetired, !instance.failed {
                issuer = (prepared, instance)
            }
        }
        let dependencies: [CloudQuiescentDependencyWait]
        if reservedFetch {
            let proof = issuer.map { CloudFetchProofIssuer(command: command, execution: $0.execution, original: $0.instance.stopWitness) }
            dependencies = try await reserveUnadmittedPolicyFetch(issuer: proof)
        }
        else { dependencies = [] }
        defer { if reservedFetch { fetchBusy = false; changed() } }

        // Preserve the original post-park disposition and guard checks,
        // including NoOperation. The earlier capability is evidence only.
        let execution: CoreCloudPolicyExecution
        do {
            guard let prepared = try await policy.prepareExecution(command) else { return }
            execution = prepared
        } catch {
            guard !Task.isCancelled, let issuer, !dependencies.isEmpty,
                  !issuer.instance.failed, issuer.instance.firstStopReason == .policyStop,
                  issuer.instance.retirementFailure == nil,
                  issuer.instance.stopWitness.accountRetired,
                  let refusal = error as? BridgeError, refusal == .cloudPolicy(.staleCommand)
            else { throw error }
            // This is the first failure after a completed valid proof and
            // accepted park/revocation, before any Core fetch or native effect.
            throw CloudDependentFetchRefusalError(error: refusal, qualification:
                .init(command: command, execution: issuer.execution, original: issuer.instance.stopWitness, dependencies: dependencies))
        }
        #if DEBUG
        await policyOperationOriginObserver?()
        #endif
        #if DEBUG
        let original: String?
        do { original = try await execution.originalSDK() }
        catch {
            if case .requestSend = command.action {
                DebugCloudDiagnostics.sendFailureCapture?.record(.originalSDK, error: error)
            }
            throw error
        }
        #else
        let original = try await execution.originalSDK()
        #endif
        guard let origin = original else {
            try await execution.validateControl()
            return // P4: no engine for this binding when the action starts.
        }
        guard let instance = owned[origin] ?? originals[origin] else {
            // Runtime registration precedes the asynchronous native factory.
            // This original has no ready SDK object, so P4 permits no network
            // work. Preserve account/binding/generation revocation checks.
            try await execution.validateControl()
            return
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
        do {
            switch command.action {
            case .requestFetch(let zones):
                _ = try await fetch(zones: scope(zones), origin: origin, execution: execution, reservedFetch: reservedFetch,
                    proofIssuer: .init(command: command, execution: execution, original: instance.stopWitness))
            case .requestSend(let zones): try await send(zones: scope(zones), origin: origin, execution: execution)
            default: throw BridgeError.cloudPolicy(.notNativeAction)
            }
        } catch {
            // P4 permits a scheduling change to retire this task's original
            // engine. That refusal supplies no transport result. The original
            // command's control guard must still pass; a changed account,
            // binding, generation or actual SDK failure is not this case.
            guard case .requestSend = command.action,
                  await schedulingRetired(instance, execution: execution, error: error) else { throw error }
        }
    }
    private func schedulingRetired(_ instance: Instance, execution: CoreCloudPolicyExecution, error: any Error) async -> Bool {
        guard !Task.isCancelled else { return false }
        guard error is CancellationError || (error as? BridgeError) == .cloudPolicy(.staleCommand)
                || (error as? BridgeError) == .cloudConnection(.authorityChanged) else { return false }
        func ownedStop() -> Bool {
            (instance.firstStopReason == .policyConfigureAutomatic || instance.firstStopReason == .configureAutomatic)
                && (instance.stopping || instance.authorityRetired) && !instance.failed
                && !quiescing && !suspended && !suspending
        }
        guard ownedStop() else { return false }
        do { try await execution.validateControl() } catch { return false }
        return ownedStop()
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
    @discardableResult
    func cancelPolicyFlush(execution: CoreCloudPolicyExecution) async throws -> CloudFlushCancellationReceipt? {
        try await execution.validateControl()
        guard let origin = try await execution.originalSDK(), let instance = owned[origin] else { return nil }
        let wasEligible = current == origin && !instance.stopping && !instance.authorityRetired && !instance.failed
        await instance.engine.cancelOperations()
        guard wasEligible, owned[origin] === instance, current == origin,
              !instance.stopping, !instance.authorityRetired, !instance.failed else { return nil }
        return CloudFlushCancellationReceipt(execution: execution, instance: instance.stopWitness)
    }
    // Pin B's own issued command and SDK before its dependency park. This
    // read grants no native admission and cannot borrow A's command witness.
    func retainFlushIssuer(command: CloudPolicyCommand, execution: CoreCloudPolicyExecution) async throws -> CloudFetchProofIssuer? {
        guard let origin = try await execution.originalSDK() else { return nil }
        guard let instance = owned[origin], current == origin, !instance.stopping,
              !instance.authorityRetired, !instance.failed else { return nil }
        return .init(command: command, execution: execution, original: instance.stopWitness)
    }
    // Core registration precedes the async factory. A selected namespace in
    // that precise phase has no native engine to run P4 media work yet. Validate
    // the original control guard before returning this no-operation decision.
    func policyMediaAwaitsCreation(_ sdk: String, execution: CoreCloudPolicyExecution) async throws -> Bool {
        guard let pending = creatingSDKRegistration else { return false }
        let registration = try await pending.task.value
        func awaitingCreation() -> Bool {
            creatingSDKRegistration === pending && registration.instanceID == sdk
                && current == nil && owned[sdk] == nil
        }
        guard awaitingCreation() else { return false }
        try await execution.validateControl()
        return awaitingCreation()
    }
    func sendPolicyMedia(execution: CoreCloudPolicyExecution) async throws {
        guard let origin = try await execution.originalSDK(), let instance = owned[origin], await valid(origin) else {
            throw BridgeError.cloudPolicy(.staleCommand)
        }
        // Core qualifies both sends; the same original SDK serves collection first.
        do {
            #if DEBUG
            try await DebugCloudDiagnostics.$mediaSendPhase.withValue(.mediaCollectionSend) {
                try await send(zones: [instance.zone], origin: origin, execution: execution, requireMediaAdmission: true)
            }
            #else
            try await send(zones: [instance.zone], origin: origin, execution: execution, requireMediaAdmission: true)
            #endif
        }
        catch {
            #if DEBUG
            DebugCloudDiagnostics.sendFailureCapture?.record(.mediaCollectionSend, error: error)
            #endif
            throw error
        }
        do { try Task.checkCancellation() }
        catch {
            #if DEBUG
            DebugCloudDiagnostics.sendFailureCapture?.record(.mediaCancellationCheck, error: error)
            #endif
            throw error
        }
        do {
            #if DEBUG
            try await DebugCloudDiagnostics.$mediaSendPhase.withValue(.mediaSend) {
                try await send(zones: ["meguri-media"], origin: origin, execution: execution, requireMediaAdmission: true)
            }
            #else
            try await send(zones: ["meguri-media"], origin: origin, execution: execution, requireMediaAdmission: true)
            #endif
        }
        catch {
            #if DEBUG
            DebugCloudDiagnostics.sendFailureCapture?.record(.mediaSend, error: error)
            #endif
            throw error
        }
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
        repeat {
            while exchangeTask != nil || policyReservation != nil {
                let parked = try await CloudQuiescentDependencyWait.park()
                do {
                    while exchangeTask != nil || policyReservation != nil {
                        if let running = exchangeTask {
                            let old = version
                            // Join the exact prior worker and its original result.
                            _ = try? await running.task.value
                            if exchangeTask?.id == running.id { try await waitForChange(after: old) }
                        } else { let old = version; try await waitForChange(after: old) }
                        try Task.checkCancellation()
                    }
                } catch {
                    try? await parked?.revoke()
                    throw error
                }
                try await parked?.revoke()
            }
            _ = try await waitForQuiescentFetchDependency()
        } while exchangeTask != nil || policyReservation != nil
        try Task.checkCancellation()
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
    func preparePolicyExchange(command: CloudPolicyCommand, execution: CoreCloudPolicyExecution) async throws -> PreparedPolicyExchange {
        guard !quiescing else { throw CancellationError() }
        guard let origin = try await execution.originalSDK() else { throw BridgeError.cloudPolicy(.staleCommand) }
        _ = try await waitForQuiescentFetchDependency()
        try await waitForDeferredUpdates(origin: origin)
        try await execution.validate()
        guard await valid(origin) else { throw BridgeError.cloudPolicy(.staleCommand) }
        guard let instance = owned[origin] else { throw BridgeError.cloudPolicy(.staleCommand) }
        return PreparedPolicyExchange(origin: origin, zone: instance.zone,
            proofIssuer: .init(command: command, execution: execution, original: instance.stopWitness))
    }
    func runPolicyExchange(runtimeID: UInt64, execution: CoreCloudPolicyExecution, prepared: PreparedPolicyExchange) async throws {
        // Preparation pinned this original before capture. No awaited lookup
        // or busy admission can throw between capture and installing its worker.
        let origin = prepared.origin
        let id = UUID()
        let task = Task {
            return try await self.runCapturedExchange(origin: origin, zone: prepared.zone, id: runtimeID, execution: execution,
                proofIssuer: prepared.proofIssuer)
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
    func recordAccountExchangeCancellation(_ command: CloudPolicyCommand, execution: CoreCloudPolicyExecution,
                                           intent: CloudPolicyCancellationContext) async throws {
        guard command.accountRetirement != nil, case .cancelExchange(let exchangeID) = command.action else { return }
        let origin = try await execution.originalSDK()
        try await execution.validateControl()
        guard !Task.isCancelled, let origin, current == origin, exchangeOrigin == origin,
              let policyWorkerID, exchangeTask?.id == policyWorkerID,
              let instance = owned[origin], !instance.stopping, !instance.authorityRetired,
              !instance.failed, instance.retirementFailure == nil, instance.firstStopReason == nil,
              let wait = instance.rootReadIdentity?.wait, let issuer = wait.issuer,
              wait.cancellationIntent === intent,
              issuer.original === instance.stopWitness, wait.original === instance.stopWitness,
              wait.original.sdk == origin,
              case .runExchange(let originalID, _) = issuer.command.action, originalID == exchangeID
        else { return }
        intent.beforeCancellation {
            wait.complete(.failure(CancellationError()), producer: .accountStop(.init(command: command, execution: execution)))
        }
    }
    private func cancelExchange(id: UUID) {
        if exchangeTask?.id == id { exchangeTask?.task.cancel() }
    }
    func retainPreparationOutput(_ output: CloudPolicyOutput) { policyOperationOutputs.append(output) }

    /// Owned by the active exchange or policy task; callers await its read and reply.
    func prepareSyncNowEpoch(execution: CoreCloudPolicyExecution? = nil, request: CoreCloudSyncNowRequest? = nil) async throws -> Bool {
        if let task = epochPreparationTask { return try await task.value }
        let task = Task { try await self.runEpochPreparation(execution: execution, request: request) }
        epochPreparationTask = task; changed(readiness: false)
        defer { epochPreparationTask = nil; changed(readiness: false) }
        return try await withTaskCancellationHandler { try await task.value }
            onCancel: { task.cancel() }
    }
    private func runEpochPreparation(execution: CoreCloudPolicyExecution?, request: CoreCloudSyncNowRequest?) async throws -> Bool {
        try Task.checkCancellation()
        guard let origin = current, let instance = owned[origin], await valid(origin) else { return false }
        repeat {
            _ = try await waitForQuiescentFetchDependency()
            while instance.sdkFetchActive || instance.assetReadActive || epochReadOrigin != nil {
                let old = version; try await waitForChange(after: old)
            }
            try await waitForDeferredUpdates(origin: origin)
            #if DEBUG
            // Observe the real post-drain, pre-reservation suspension boundary.
            // Callbacks arriving here retain their normal handler bookkeeping.
            await epochReservationObserver?()
            #endif
        } while fetchBusy || instance.sdkFetchActive || instance.assetReadActive || epochReadOrigin != nil
        try Task.checkCancellation()
        guard current == origin, owned[origin] === instance, !instance.stopping,
              !quiescing, !suspended, !suspending else { throw CancellationError() }
        fetchBusy = true; epochReadOrigin = origin; instance.networks += 1; changed(readiness: false)
        defer { fetchBusy = false; epochReadOrigin = nil; instance.networks -= 1; changed(readiness: false) }
        #if DEBUG
        // Scheduling observation only: the original execution is already issued.
        if execution != nil { await policyOperationOriginObserver?() }
        #endif
        let read: CoreCloudFetchRead?
        if let execution { read = try await execution.syncNowEpochRead() }
        else if let request { read = try await request.epochRead() }
        else { read = try await core.syncNowEpochRead(sdk: origin) }
        guard let read else { return false }
        guard let transport = assetReadTransport else {
            throw CloudEpochPreparationError(outcome: .waiting(.other, retryAfterMilliseconds: nil), original: nil)
        }
        do {
            if let next = try await performFetchRead(read, transport: transport, origin: origin, instance: instance, execution: execution, epochPreparation: true) {
                _ = try await performFetchRead(next, transport: transport, origin: origin, instance: instance, execution: execution, epochPreparation: true)
            }
            try Task.checkCancellation()
            guard await valid(origin), !quiescing, !suspended, !suspending else { throw CancellationError() }
        } catch {
            let outcome: CloudPolicyEarlyEnd
            if let bridge = error as? BridgeError, case .cloudConnection(.tracking) = bridge {
                outcome = .failed(.localStorageFull)
            } else {
                outcome = try await read.preparationOutcome(error: error) ?? .cancelled
            }
            throw CloudEpochPreparationError(outcome: outcome, original: error)
        }
        if let outcome = try await read.preparationOutcome() {
            throw CloudEpochPreparationError(outcome: outcome, original: nil)
        }
        return true
    }
    private func runOwnedExchange() async throws -> CloudExchangeResult {
        defer { exchangeTask = nil; exchangeOrigin = nil; exchangeRunning = false; changed() }
        if current == nil { try await start(automaticallySync: automatic) }
        guard let admittedOrigin = current else { throw BridgeError.internal("Cloud engine is unavailable") }
        let request = try await core.syncNowRequest(sdk: admittedOrigin)
        _ = try await prepareSyncNowEpoch(request: request)
        try Task.checkCancellation()
        guard let origin = current, origin == admittedOrigin, let instance = owned[origin], await valid(origin) else { throw BridgeError.internal("Cloud engine is unavailable") }
        let id = try await request.beginExchange()
        exchangeOrigin = origin
        return try await runCapturedExchange(origin: origin, zone: instance.zone, id: id)
    }
    private func runCapturedExchange(origin: String, zone: String, id: UInt64, execution: CoreCloudPolicyExecution? = nil,
                                     proofIssuer: CloudFetchProofIssuer? = nil) async throws -> CloudExchangeResult {
        let zones = ["meguri-root", zone]
        #if DEBUG
        let diagnostic = DebugCloudDiagnostics.sendFailureCapture
        var site = DebugCloudDiagnostics.SendFailureSite.exchangeFetch
        #endif
        var fetched = false
        var operationError: (any Error)?
        do {
            try Task.checkCancellation()
            fetched = try await fetch(zones: zones, origin: origin, execution: execution, proofIssuer: proofIssuer)
            try Task.checkCancellation()
            if fetched {
                #if DEBUG
                site = .exchangeFetched
                #endif
                try await core.exchangeFetched(sdk: origin, complete: true)
                repeat {
                    try Task.checkCancellation()
                    #if DEBUG
                    site = .exchangeSend
                    #endif
                    try await send(zones: [zone], origin: origin, execution: execution)
                    #if DEBUG
                    site = .exchangeSendDue
                    #endif
                } while try await core.sendDue(sdk: origin)
                #if DEBUG
                site = .exchangeFetch
                #endif
                fetched = try await fetch(zones: zones, origin: origin, execution: execution, proofIssuer: proofIssuer)
            }
            try Task.checkCancellation()
        } catch {
            #if DEBUG
            diagnostic?.record(site, error: error)
            #endif
            operationError = error
        }
        // Account from the exact original error while retaining its sealed
        // outcome for return after required Core completion succeeds.
        let cancelled = cloudOperationWasCancelled((operationError as? CloudFetchProofError)?.error ?? operationError)
        #if DEBUG
        let result: CloudExchangeResult
        do {
            result = try await core.completeExchange(sdk: origin, id: id, fetched: fetched,
                cancelled: cancelled, failed: operationError != nil && !cancelled)
        } catch { diagnostic?.record(.nativeExchangeCompletion, error: error); throw error }
        #else
        let result = try await core.completeExchange(sdk: origin, id: id, fetched: fetched,
                                                     cancelled: cancelled, failed: operationError != nil && !cancelled)
        #endif
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
