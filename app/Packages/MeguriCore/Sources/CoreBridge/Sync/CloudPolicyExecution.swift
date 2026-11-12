import Foundation
import Instrumentation
import Synchronization
internal import MeguriCoreBindings

/// Only CoreCloudPolicyService can obtain this retained Rust capability.
struct CoreCloudPolicyExecution: Sendable {
    let raw: MeguriCoreBindings.CloudPolicyExecution
    init(_ raw: MeguriCoreBindings.CloudPolicyExecution) { self.raw = raw }
    func beginAssetReadPass() async throws -> CoreCloudAssetReadPass {
        let raw = self.raw
        let pass: MeguriCoreBindings.CloudAssetReadPass = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartBeginAssetReadPass, result: .cloudPolicyAssetReadPassResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startBeginAssetReadPass()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CoreCloudAssetReadPass(pass)
    }
    func originalSDK() async throws -> String? {
        let raw = self.raw
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionOriginalSdk) { [raw] in try raw.originalSdk() }
    }
    func validate() async throws {
        let raw = self.raw
        try await CoreCloudPolicyService.call(.cloudPolicyExecutionValidateExecution) { [raw] in try raw.validateExecution() }
    }
    /// Called inside the retained operation task on the FFI lane. Core returns
    /// and releases every lock before the public async SDK invocation.
    func validateForEnqueue() throws {
        let raw = self.raw
        try cloudFinalCoreCall(.cloudPolicyExecutionValidateExecution) { try raw.validateExecution() }
    }
    func registerSDK() async throws -> CloudRegistration {
        let raw = self.raw
        let r: MeguriCoreBindings.CloudEngineRegistration = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartRegisterSdk, result: .cloudPolicyRegistrationResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startRegisterSdk()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudRegistration(instanceID: r.instanceId, serialization: r.serialization,
                                 collectionZone: r.collectionZone, needsRootFetch: r.needsRootFetch)
    }
    func retireOriginal() async throws {
        let raw = self.raw
        let _: Bool = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartRetireOriginal, result: .cloudPolicyRetirementResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startRetireOriginal()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
    }
    func report(zones: [CloudPolicyZone], kind: CloudPolicyOperationKind, started: Bool, error: (any Error)? = nil, saved: UInt64 = 0,
                trackStarted: @escaping @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob = { try Jobs.track($0) }) async throws -> CloudPolicyOutput {
        let raw = self.raw
        let failure = error as? CloudKitSDKOperationFailure
        let facts = MeguriCoreBindings.CloudPolicyOperationFacts(zones: zones.map(\.raw), kind: kind.raw,
            started: started, failed: error != nil, code: failure?.code,
            retryAfterMs: failure?.retryAfterMilliseconds, saved: saved, appCancelled: error is CancellationError)
        if !started {
            let value: MeguriCoreBindings.CloudPolicyOutput = try await CoreCloudPolicyService.runOrdered(
                .cloudPolicyExecutionStartReportCompletion, result: .cloudPolicyOutputResult,
                requiredCompletion: true
            ) { [raw] in
                let job = try raw.startReportCompletion(facts: facts)
                return CloudPolicyJobValue(handle: job.handle, read: { try job.result.result() })
            }
            return CloudPolicyOutput(value)
        }
        let value: MeguriCoreBindings.CloudPolicyOutput = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartReportStarted, result: .cloudPolicyOutputResult,
            track: trackStarted, retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let job = try raw.startReportStarted(facts: facts)
            return CloudPolicyJobValue(handle: job.handle, read: { try job.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    /// Started preparation belongs to the retained operation task. Final
    /// synchronous authorization follows this job and all event waits.
    func reportInvocation(zones: [CloudPolicyZone], kind: CloudPolicyOperationKind) async throws -> CloudPolicyOutput {
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartReportInvocation, result: .cloudPolicyOutputResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startReportInvocation(zones: zones.map(\.raw), kind: kind.raw)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    func continueMediaAfterSend(_ error: any Error) async throws -> Bool {
        let raw = self.raw
        let code = (error as? CloudKitSDKOperationFailure)?.code
        return try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartContinueMediaAfterSend, result: .cloudPolicyMediaContinuationResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startContinueMediaAfterSend(code: code)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
    }
    func beginMedia() async throws -> Bool {
        let raw = self.raw
        return try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartBeginMedia, result: .cloudPolicyMediaResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startBeginMedia()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
    }
    func beginFetch(zones: [String]) async throws -> UInt64 {
        let raw = self.raw
        return try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartBeginFetch, result: .cloudPolicyFetchResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startBeginFetch(zones: zones)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
    }
    func validateControl() async throws {
        let raw = self.raw
        let _: Bool = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartValidateControl, result: .cloudPolicyControlResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startValidateControl()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
    }
    func prepareFlush() async throws -> CloudPolicyOutput {
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartPrepareFlush, result: .cloudPolicyOutputResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startPrepareFlush()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    func beginSend() async throws -> UInt64? {
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPolicySendCall = try await CoreCloudPolicyService.runOrdered(
            .cloudPolicyExecutionStartBeginSend, result: .cloudPolicySendResult,
            retainResourceResultOnTrackingFailure: true
        ) { [raw] in
            let started = try raw.startBeginSend()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return value.call
    }
}

/// The task publishes Core activity without entering the adapter
/// actor again. Its outputs join the action result before ended activity.
final class CloudPolicyInvocationOutputs: Sendable {
    private let values = Mutex<[CloudPolicyOutput]>([])
    func append(_ value: CloudPolicyOutput) { values.withLock { $0.append(value) } }
    func take() -> [CloudPolicyOutput] { values.withLock { let result = $0; $0.removeAll(); return result } }
}

/// First observation wins. A query queued before expiry is still too late if
/// its Ready reply is only observed after the deadline fired.
final class CloudPolicyReadinessDecision: Sendable {
    private enum State { case pending, ready, expired }
    private let state = Mutex(State.pending)
    func observeReady() -> Bool {
        state.withLock { state in
            if case .expired = state { return false }
            state = .ready; return true
        }
    }
    func expire() -> Bool {
        state.withLock { state in
            if case .ready = state { return false }
            state = .expired; return true
        }
    }
}

/// Original stop evidence from one exact Instance, retained after adapter removal.
/// This grants no authority and never calls Core, UI or cancellation handlers.
final class CloudSDKStopWitness: Sendable {
    let sdk: String
    private struct State {
        var stopping = false
        var authorityRetired = false
        var accountRetirement: (CloudPolicyCommand, CoreCloudPolicyExecution)?
    }
    private let state = Mutex(State())
    init(sdk: String) { self.sdk = sdk }
    func update(stopping: Bool, authorityRetired: Bool) {
        state.withLock { $0.stopping = stopping; $0.authorityRetired = authorityRetired }
    }
    var stoppingOrRetired: Bool { state.withLock { $0.stopping || $0.authorityRetired } }
    func recordAccountRetirement(_ command: CloudPolicyCommand, execution: CoreCloudPolicyExecution) {
        guard command.accountRetirement != nil else { return }
        state.withLock { if $0.accountRetirement == nil { $0.accountRetirement = (command, execution) } }
    }
    var accountRetired: Bool { state.withLock { $0.authorityRetired && $0.accountRetirement != nil } }
}

/// Shared by one actual media run and its child work. Cancellation handlers can
/// race actor-owned drains, so origin and first error observation are synchronous.
final class CloudMediaCancellationContext: Sendable {
    @TaskLocal static var current: CloudMediaCancellationContext?
    #if DEBUG
    @TaskLocal static var selectedAction: CloudPolicyAction?
    #endif
    enum Origin { case caller, owned }
    private struct State {
        var origin: Origin?
        var qualified: Bool?
        var genuineFailure = false
        var stopWitness: CloudSDKStopWitness?
    }
    private let state = Mutex(State())
    private let sdk: String
    init(sdk: String) { self.sdk = sdk }

    func attachStopWitness(_ witness: CloudSDKStopWitness) {
        guard witness.sdk == sdk else { return }
        state.withLock { if $0.stopWitness == nil { $0.stopWitness = witness } }
    }

    func cancel(_ origin: Origin, stopWitness: CloudSDKStopWitness? = nil, performing cancellation: () -> Void) {
        state.withLock {
            if $0.origin == nil { $0.origin = origin }
            if origin == .owned, let stopWitness, stopWitness.sdk == sdk, $0.stopWitness == nil {
                $0.stopWitness = stopWitness
            }
        }
        // Task/transport cancellation can synchronously invoke other handlers.
        cancellation()
    }

    @discardableResult func observe(_ error: any Error) -> Bool {
        let cancellation = error is CancellationError
        return state.withLock {
            if $0.qualified == nil { $0.qualified = cancellation && $0.origin == .owned }
            if !cancellation { $0.genuineFailure = true }
            return $0.qualified == true && !$0.genuineFailure
        }
    }

    /// Only a secondary EndRead refusal can use this evidence. Never establish
    /// qualification here or erase a previous source/error decision.
    func observeEndReadReportFailure(_ error: any Error, primary: (any Error)?) -> Bool {
        let expected = primary is CancellationError && (error as? BridgeError) == .cloudPolicy(.staleCommand) && state.withLock {
            $0.origin == .owned && $0.qualified == true && !$0.genuineFailure && $0.stopWitness?.stoppingOrRetired == true
        }
        if !expected { observe(error) }
        return expected
    }
}

/// A coordinator owns this executor alongside its ONE adapter. Outputs include
/// required failure follow-ups. Deferred configuration is not completed work.
public struct CloudPolicyNativeResult: Sendable {
    public let outputs: [CloudPolicyOutput]
    public let error: (any Error)?
    public let originalGuardRefusal: CloudPolicyOriginalGuardRefusal?
    public let dependentFetchRefusal: CloudPolicyDependentFetchRefusal?
    public let deferred: Bool
    public let mediaCancellation: Bool
    public let acknowledgedOutputCount: Int
    public let afterOutputs: (@Sendable () async -> CloudPolicyNativeResult)?
    /// The original cleanup failure is diagnostic evidence, outside app failure cycles.
    public let zoneDeletionError: (any Error)?
    public let cleanupInventoryError: (any Error)?
    init(outputs: [CloudPolicyOutput], error: (any Error)?, deferred: Bool, zoneDeletionError: (any Error)? = nil, cleanupInventoryError: (any Error)? = nil, mediaCancellation: Bool = false, acknowledgedOutputCount: Int = 0, afterOutputs: (@Sendable () async -> CloudPolicyNativeResult)? = nil, originalGuardRefusal: CloudPolicyOriginalGuardRefusal? = nil, dependentFetchRefusal: CloudPolicyDependentFetchRefusal? = nil) {
        self.outputs = outputs; self.error = error; self.deferred = deferred; self.zoneDeletionError = zoneDeletionError; self.cleanupInventoryError = cleanupInventoryError
        self.mediaCancellation = mediaCancellation; self.acknowledgedOutputCount = acknowledgedOutputCount
        self.afterOutputs = afterOutputs; self.originalGuardRefusal = originalGuardRefusal
        self.dependentFetchRefusal = dependentFetchRefusal
    }
}
// Forward the existing coordinator ACK seam through transparent backend
// observers without replacing their execute/result hooks.
public enum CloudPolicyOutputAcknowledgement {
    @TaskLocal public static var current: (@Sendable ([CloudPolicyOutput]) async -> Void)?
    @TaskLocal public static var input: (@Sendable (@escaping @Sendable () async throws -> CloudPolicyOutput) async throws -> CloudPolicyOutput)?
}
// A receipt for an already returned output prefix, not a scheduler or authority.
// Standalone callers retain the full result and their existing execution loop.
private struct CloudPolicyOutputDelivery: Sendable {
    let deliver: (@Sendable ([CloudPolicyOutput]) async -> Void)?
    private(set) var count = 0
    mutating func acknowledge(_ outputs: [CloudPolicyOutput]) async {
        guard let deliver else { return }
        await deliver(Array(outputs.dropFirst(count)))
        count = outputs.count
    }
    mutating func includeAcknowledgedPrefix(_ count: Int) { self.count += count }
    mutating func appendInput(to outputs: inout [CloudPolicyOutput],
                              _ work: @escaping @Sendable () async throws -> CloudPolicyOutput) async throws {
        await acknowledge(outputs)
        if let input = CloudPolicyOutputAcknowledgement.input {
            let output = try await input(work)
            outputs.append(output)
            count = outputs.count
        } else { outputs.append(try await work()) }
    }
}
public protocol CloudPolicyNativeExecuting: Sendable {
    func execute(_ command: CloudPolicyCommand) async -> CloudPolicyNativeResult
}
/// Optional test/coordinator early-end hook after mandatory Core readiness. A typed
/// early end never captures a Runtime exchange and cannot report success.
public protocol CloudPolicyExchangeGate: Sendable {
    func waitBeforeCapture(_ command: CloudPolicyCommand) async throws -> CloudPolicyEarlyEnd?
}
public actor NativeCloudPolicyExecutor: CloudPolicyNativeExecuting {
    private let policy: CoreCloudPolicyService
    private let adapter: CloudKitSyncAdapter
    private let exchangeGate: (any CloudPolicyExchangeGate)?
    private let observeExchangeCapture: (@Sendable (CloudPolicyCommand, UInt64) async -> Void)?
    private let flushDeadline: @Sendable (Int64) async throws -> Void
    private let readinessClock: any CloudPolicyReadinessClock
    private var exchange: UInt64?
    private var nativeExchangeTask: Task<CloudPolicyNativeResult, Never>?
    private final class FlushOwner {
        let id = UUID()
        let execution: CoreCloudPolicyExecution
        let exchange: Task<CloudPolicyNativeResult, Never>?
        let exchangeID: UInt64?
        var finished = false
        let park: CloudQuiescentDependencyWait?
        #if DEBUG
        let observeCall = MaintenanceCallObservation.capture
        #endif
        var cancellation: Task<(any Error)?, Never>?
        init(execution: CoreCloudPolicyExecution, exchange: Task<CloudPolicyNativeResult, Never>?, exchangeID: UInt64?) {
            self.execution = execution; self.exchange = exchange; self.exchangeID = exchangeID
            park = CloudQuiescentDependencyWait.retained()
        }
    }
    private typealias OwnedFlush = (id: UUID, task: Task<CloudPolicyNativeResult, Never>, owner: FlushOwner)
    private var flushTask: OwnedFlush?
    private var heldConfigure: (CloudPolicyCommand, CoreCloudPolicyExecution)?
    private let mediaService: CoreCloudMediaService?
    private let mediaTransport: (any CloudMediaTransport)?
    private var mediaExecutors: [String: CloudMediaExecutor] = [:]
    private var mediaRuns: [String: Task<CloudPolicyNativeResult, Never>] = [:]
    private var mediaRunIDs: [String: UUID] = [:]
    private var mediaRunContexts: [String: CloudMediaCancellationContext] = [:]
    private var mediaDrains: [String: Task<Void, Never>] = [:]
    #if DEBUG
    private var existingMediaDrainObserver: (@Sendable (String) -> Void)? = nil
    func setExistingMediaDrainObserver(_ observer: (@Sendable (String) -> Void)?) {
        existingMediaDrainObserver = observer
    }
    private var mediaEndReadObserver: (@Sendable (String) async -> Void)? = nil
    func setMediaEndReadObserver(_ observer: (@Sendable (String) async -> Void)?) {
        mediaEndReadObserver = observer
    }
    #endif
    private var heldMediaResume: [String: (CloudPolicyCommand, CoreCloudPolicyExecution)] = [:]
    private var mediaHookInstalled = false
    private var quiescing = false
    private let cleanupWorker: CloudCleanupExecutor?
    private var cleanupRun: Task<CloudPolicyNativeResult, Never>?
    private var cleanupSDK: String?
    private let zoneDeletionWorker: CloudZoneDeletionExecutor?
    private var zoneDeletionRun: Task<CloudPolicyNativeResult, Never>?
    private var zoneDeletionSDK: String?

    public init(policy: CoreCloudPolicyService, adapter: CloudKitSyncAdapter, exchangeGate: (any CloudPolicyExchangeGate)? = nil, readinessClock: any CloudPolicyReadinessClock = ContinuousCloudPolicyReadinessClock(), mediaService: CoreCloudMediaService? = nil, mediaTransport: (any CloudMediaTransport)? = nil, ownTransport: (any CloudOwnTransport)? = nil,
                observeExchangeCapture: (@Sendable (CloudPolicyCommand, UInt64) async -> Void)? = nil,
                flushDeadline: @escaping @Sendable (Int64) async throws -> Void = { until in
                    try await Task.sleep(for: .seconds(max(0, Double(until) / 1_000 - Date().timeIntervalSince1970)))
                }) {
        self.policy = policy; self.adapter = adapter; self.exchangeGate = exchangeGate; self.readinessClock = readinessClock
        self.observeExchangeCapture = observeExchangeCapture
        self.mediaService = mediaService; self.mediaTransport = mediaTransport
        self.cleanupWorker = ownTransport.map { CloudCleanupExecutor(transport: $0) }
        self.zoneDeletionWorker = ownTransport.map { CloudZoneDeletionExecutor(transport: $0, freshAccountRead: { try await adapter.refreshControlIdentity() }) }
        self.flushDeadline = flushDeadline
    }
    private func nextPolicyInput(_ work: @escaping @Sendable () async throws -> CloudPolicyOutput) async throws -> CloudPolicyOutput {
        if let input = CloudPolicyOutputAcknowledgement.input { return try await input(work) }
        return try await work()
    }
    private func deferredRefreshAfterFailure() -> @Sendable () async -> CloudPolicyNativeResult {
        let returned = CloudPolicyOutputAcknowledgement.current
        let input = CloudPolicyOutputAcknowledgement.input
        return {
            await CloudPolicyOutputAcknowledgement.$current.withValue(returned) {
                await CloudPolicyOutputAcknowledgement.$input.withValue(input) {
                    await self.refreshAfterFailure()
                }
            }
        }
    }
    private func refreshAfterFailure() async -> CloudPolicyNativeResult {
        let followup = try? await nextPolicyInput { [policy] in try await policy.input(.refresh(.cloud)) }
        return .init(outputs: followup.map { [$0] } ?? [], error: nil, deferred: false,
            acknowledgedOutputCount: CloudPolicyOutputAcknowledgement.input == nil ? 0 : (followup == nil ? 0 : 1))
    }
    public func quiesce() async throws {
        quiescing = true
        heldMediaResume.removeAll()
        async let adapterDrain: Void = adapter.quiesce()
        await drainCleanups()
        await drainZoneDeletions()
        try await adapterDrain
    }
    public func execute(_ command: CloudPolicyCommand) async -> CloudPolicyNativeResult {
        await execute(command, corePrepared: {})
    }
    public func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @Sendable () async -> Void) async -> CloudPolicyNativeResult {
        await execute(command, corePrepared: corePrepared, acknowledgeOutputs: CloudPolicyOutputAcknowledgement.current)
    }
    public func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @Sendable () async -> Void,
                        acknowledgeOutputs: (@Sendable ([CloudPolicyOutput]) async -> Void)?) async -> CloudPolicyNativeResult {
        var delivery = CloudPolicyOutputDelivery(deliver: acknowledgeOutputs)
        var outputs: [CloudPolicyOutput] = []
        #if DEBUG
        var site = DebugCloudDiagnostics.SendFailureSite.executeAdmission
        #endif
        if !mediaHookInstalled {
            mediaHookInstalled = true
            await adapter.setOriginalMediaDrainWithWitness { [weak self] witness in
                let sdk = witness.sdk
                await self?.drainCleanups(sdk: sdk)
                await self?.drainZoneDeletions(sdk: sdk)
                await self?.drainOriginalMedia(sdk, stopWitness: witness)
            }
        }
        do {
            switch command.action {
            case .updatePending, .applyReady, .recordExchange, .recordQuietPoint, .probeTracking, .statusChanged,
                 .stopEngine, .cancelExchange, .cancelBackgroundFlush, .pauseMedia, .cancelFlushTimer, .quitNow:
                break
            default:
                guard !quiescing else { throw CancellationError() }
            }
            switch command.action {
            case .updatePending:
                #if DEBUG
                site = .executeCore
                #endif
                outputs.append(try await policy.executeCoreAttempt(command))
                // Core validates the original command and queues its ordered
                // updates. The adapter validates the SDK and defers application
                // until any active handler has returned, then acknowledges
                // only the additions the SDK actually retained.
                #if DEBUG
                site = .pendingUpdateApplication
                #endif
                try await adapter.consumePendingUpdates()
            case .applyReady, .recordExchange, .recordQuietPoint, .probeTracking, .statusChanged:
                outputs.append(try await policy.executeCore(command))
            case .startEngine, .stopEngine, .configureAutomatic:
                guard let execution = try await policy.prepareExecution(command) else {
                    return .init(outputs: [], error: nil, deferred: false)
                }
                if case .configureAutomatic = command.action, exchange != nil {
                    heldConfigure = (command, execution)
                    return .init(outputs: [], error: nil, deferred: true)
                }
                if try await adapter.executePolicyLifecycle(command, execution: execution, policy: policy) {
                    return .init(outputs: [], error: nil, deferred: true)
                }
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                await delivery.acknowledge(outputs)
                try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
            case .requestFetch(let zones), .requestSend(let zones):
                #if DEBUG
                site = .prepareExecution
                #endif
                if zones.contains(.media), mediaService == nil || mediaTransport == nil { throw BridgeError.cloudPolicy(.coreCapabilityUnavailable) }
                #if DEBUG
                site = .requestExecution
                #endif
                do { try await adapter.executePolicyOperation(command, policy: policy) }
                catch {
                    outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                    throw error
                }
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                await delivery.acknowledge(outputs)
                #if DEBUG
                site = .policyRefresh
                #endif
                try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
            case .runCleanup:
                guard cleanupRun == nil else { throw CloudOwnTransportFailure.busy }
                let task = Task { await self.performCleanup(command, corePrepared: corePrepared) }
                cleanupRun = task
                defer { cleanupRun = nil; cleanupSDK = nil }
                return await withTaskCancellationHandler {
                    if Task.isCancelled { task.cancel() }
                    return await task.value
                } onCancel: { task.cancel() }
            case .deleteZones:
                // An action emitted during a run adds no work, even a rerun.
                guard zoneDeletionRun == nil else { return .init(outputs: [], error: nil, deferred: false) }
                let task = Task { await self.performZoneDeletion(command, worker: zoneDeletionWorker) }
                zoneDeletionRun = task
                defer { zoneDeletionRun = nil; zoneDeletionSDK = nil }
                return await withTaskCancellationHandler {
                    if Task.isCancelled { task.cancel() }
                    return await task.value
                } onCancel: { task.cancel() }
            case .retryAssetEntries:
                guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                let result = await adapter.executePolicyAssetReads(command, execution: execution)
                outputs.append(contentsOf: result.outputs)
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                if let error = result.error { throw error }
            case .readBlobs, .resumeMedia, .pauseMedia, .prioritizeMedia:
                guard let mediaService, let mediaTransport else { throw BridgeError.cloudPolicy(.coreCapabilityUnavailable) }
                let prepared: CoreCloudPolicyExecution?
                if case .resumeMedia = command.action { prepared = try await policy.prepareExecutionAttempt(command) }
                else { prepared = try await policy.prepareExecution(command) }
                guard let permit = prepared else { throw BridgeError.cloudPolicy(.staleCommand) }
                let instance = try await permit.originalSDK()
                if case .pauseMedia = command.action {
                    if let instance { await drainOriginalMedia(instance) }
                    _ = try await permit.beginMedia()
                    try await adapter.waitForDeferredUpdates()
                    await delivery.acknowledge(outputs)
                    try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
                } else if case .prioritizeMedia = command.action {
                    _ = try await permit.beginMedia()
                } else {
                    guard let instance else { throw BridgeError.cloudPolicy(.staleCommand) }
                    if let joining = mediaDrains[instance] { await joining.value }
                    guard !quiescing else { throw CancellationError() }
                    try Task.checkCancellation()
                    if mediaRuns[instance] != nil {
                        if case .resumeMedia = command.action { heldMediaResume[instance] = (command, permit) }
                        return .init(outputs: [], error: nil, deferred: true)
                    }
                    let worker: CloudMediaExecutor
                    if let retained = mediaExecutors[instance] { worker = retained }
                    else {
                        worker = CloudMediaExecutor(core: mediaService, sdk: instance, transport: mediaTransport, freshAccountRead: { try await self.adapter.refreshControlIdentity() })
                        mediaExecutors[instance] = worker
                    }
                    let runID = UUID()
                    let context = CloudMediaCancellationContext(sdk: instance)
                    let running = Task {
                        await CloudMediaCancellationContext.$current.withValue(context) {
                            await self.performMediaRound(command, permit: permit, instance: instance, runID: runID, worker: worker, coreMedia: mediaService, context: context, acknowledgeOutputs: acknowledgeOutputs)
                        }
                    }
                    mediaRuns[instance] = running; mediaRunIDs[instance] = runID; mediaRunContexts[instance] = context
                    return await withTaskCancellationHandler { await running.value }
                        onCancel: { context.cancel(.caller) { running.cancel() } }
                }
            case .backgroundFlush(let until), .quitFlush(let until):
                guard flushTask == nil else { return .init(outputs: [], error: nil, deferred: true) }
                guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                try await execution.validateControl()
                let owner = FlushOwner(execution: execution, exchange: nativeExchangeTask, exchangeID: exchange)
                let id = owner.id
                let task = Task { await self.performFlush(owner: owner, acknowledgeOutputs: acknowledgeOutputs) }
                flushTask = (id, task, owner)
                let deadline = Task<CloudPolicyOutput?, Never> {
                    do {
                        try await self.flushDeadline(until)
                        _ = await self.cancelFlush(id: id)
                        return try await self.nextPolicyInput { [policy] in try await policy.input(.backgroundExpiring) }
                    } catch { return nil }
                }
                let result = await withTaskCancellationHandler { await task.value }
                    onCancel: { Task { _ = await self.cancelFlush(id: id) } }
                deadline.cancel()
                let expired = await deadline.value
                // A completed flush body can race its caller's cancellation.
                // Keep B tracked through the original cancellation's native/Core
                // work as well as its actual task result.
                if let cancellation = owner.cancellation { _ = await cancellation.value }
                if flushTask?.id == id { flushTask = nil }
                if let expired { return .init(outputs: result.outputs + [expired], error: result.error, deferred: result.deferred, acknowledgedOutputCount: result.acknowledgedOutputCount + (CloudPolicyOutputAcknowledgement.input == nil ? 0 : 1)) }
                return result
            case .cancelBackgroundFlush:
                // This control command also joins removal before its first Core call.
                try await flushTask?.owner.park?.revoke()
                guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                try await execution.validateControl()
                if let flushTask { if let failure = await cancelFlush(id: flushTask.id) { throw failure } }
                else { try await adapter.cancelPolicyFlush(execution: execution) }
            case .armFlushTimer, .cancelFlushTimer, .quitNow:
                guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                try await execution.validateControl()
            case .runExchange(let id, _):
                guard exchange == nil else { throw BridgeError.cloudPolicy(.unqualifiedCompletion) }
                exchange = id
                defer { exchange = nil }
                let task = Task {
                    #if DEBUG
                    return await CloudMediaCancellationContext.$selectedAction.withValue(command.action) {
                        await self.performExchange(command, id: id, acknowledgeOutputs: acknowledgeOutputs)
                    }
                    #else
                    return await self.performExchange(command, id: id, acknowledgeOutputs: acknowledgeOutputs)
                    #endif
                }
                nativeExchangeTask = task
                defer { nativeExchangeTask = nil }
                return await withTaskCancellationHandler { await task.value } onCancel: {
                    Task { await self.cancelOriginalExchangeTask(id: id, task: task) }
                }
            case .cancelExchange(let id):
                try await revokeFlushPark(joining: id)
                let execution = try await policy.prepareExecution(command)
                if execution != nil, exchange == id { nativeExchangeTask?.cancel(); await adapter.cancelPolicyExchange() }
            }
            return .init(outputs: outputs, error: nil, deferred: false, acknowledgedOutputCount: delivery.count)
        } catch let caught {
            // Extract only this exact original attempt's error. A failed refresh
            // cannot change the already retained result.
            let attempt = caught as? CloudPolicyAttemptError
            let dependent = caught as? CloudDependentFetchRefusalError
            let error: any Error
            if let attempt { error = attempt.error } else if let dependent { error = dependent.error } else { error = caught }
            let refusal = attempt?.refusal
            #if DEBUG
            switch command.action {
            case .requestSend, .updatePending:
                DebugCloudDiagnostics.sendFailureCapture?.record(site, error: error)
            default: break
            }
            DebugCloudDiagnostics.record(.policyExecute, action: command.action, error: error)
            #endif
            // A failed native event transaction may have stopped tracking. Its
            // real projection supplies StopEngine and any other required work.
            if CloudPolicyOutputAcknowledgement.input != nil {
                switch command.action {
                case .updatePending, .applyReady, .recordExchange, .recordQuietPoint, .probeTracking, .statusChanged:
                    // Return the failed local computation through its original
                    // ACK before the later refresh takes another input turn.
                    return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count,
                        afterOutputs: deferredRefreshAfterFailure(), originalGuardRefusal: refusal)
                default: break
                }
            }
            await delivery.acknowledge(outputs)
            _ = try? await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
            return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count, originalGuardRefusal: refusal, dependentFetchRefusal: dependent?.qualification)
        }
    }
    private func performCleanup(_ command: CloudPolicyCommand, corePrepared: @Sendable () async -> Void) async -> CloudPolicyNativeResult {
        do {
            try Task.checkCancellation()
            guard let worker = cleanupWorker else { throw BridgeError.cloudPolicy(.coreCapabilityUnavailable) }
            guard let permit = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
            cleanupSDK = try await permit.originalSDK()
            try Task.checkCancellation()
            // A failure here is a required-computation failure. Only a pass
            // returned by this exact qualified port permits the observation.
            let pass = try await permit.beginCleanupPass()
            await corePrepared()
            var inventoryFailure: (any Error)?
            do { try await worker.run(pass) }
            catch { inventoryFailure = error }
            let projection = try await nextPolicyInput { [policy] in try await policy.input(.refresh(.cloud)) }
            return .init(outputs: [projection], error: nil, deferred: false, cleanupInventoryError: inventoryFailure, acknowledgedOutputCount: CloudPolicyOutputAcknowledgement.input == nil ? 0 : 1)
        } catch {
            CloudCleanupDiagnostics.record(error)
            if CloudPolicyOutputAcknowledgement.input != nil {
                return .init(outputs: [], error: error, deferred: false,
                    afterOutputs: deferredRefreshAfterFailure())
            }
            // A failed required computation may have stopped tracking. Return
            // its actual projection so lifecycle follow-ups can still drain.
            let followup = try? await nextPolicyInput { [policy] in try await policy.input(.refresh(.cloud)) }
            return .init(outputs: followup.map { [$0] } ?? [], error: error, deferred: false,
                acknowledgedOutputCount: CloudPolicyOutputAcknowledgement.input == nil ? 0 : (followup == nil ? 0 : 1))
        }
    }
    private func drainCleanups(sdk: String) async {
        guard cleanupSDK == nil || cleanupSDK == sdk else { return }
        await drainCleanups()
    }
    public func drainCleanups() async {
        let original = cleanupRun
        original?.cancel()
        await cleanupWorker?.interruptAndDrain()
        _ = await original?.value
    }
    private func performZoneDeletion(_ command: CloudPolicyCommand, worker: CloudZoneDeletionExecutor?) async -> CloudPolicyNativeResult {
        var failure: (any Error)?
        var workerStarted = false
        var category = CloudZoneDeletionLocalFailure.preparation
        do {
            try Task.checkCancellation()
            guard let permit = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
            zoneDeletionSDK = try await permit.originalSDK()
            category = .capabilityUnavailable
            guard let worker else { throw BridgeError.cloudPolicy(.coreCapabilityUnavailable) }
            category = .preparation
            try Task.checkCancellation()
            if let pass = try await permit.beginZoneDeletionPass() {
                workerStarted = true
                try await worker.run(pass)
            }
        } catch {
            if !workerStarted { CloudZoneDeletionDiagnostics.record(category, error: error) }
            failure = error
        }
        // Reset isolation and tracking failures may have changed real policy facts.
        // Refresh after the original worker joins, including its error path.
        do {
            let followup = try await nextPolicyInput { [policy] in try await policy.input(.refresh(.cloud)) }
            return .init(outputs: [followup], error: nil, deferred: false, zoneDeletionError: failure, acknowledgedOutputCount: CloudPolicyOutputAcknowledgement.input == nil ? 0 : 1)
        } catch {
            CloudZoneDeletionDiagnostics.record(.completion, error: error)
            return .init(outputs: [], error: nil, deferred: false, zoneDeletionError: failure ?? error)
        }
    }
    private func drainZoneDeletions(sdk: String) async {
        guard zoneDeletionSDK == nil || zoneDeletionSDK == sdk else { return }
        await drainZoneDeletions()
    }
    public func drainZoneDeletions() async {
        let original = zoneDeletionRun
        original?.cancel()
        await zoneDeletionWorker?.interruptAndDrain()
        _ = await original?.value
    }
    private func revokeFlushPark(joining exchangeID: UInt64) async throws {
        guard let original = flushTask, original.owner.exchangeID == exchangeID else { return }
        try await original.owner.park?.revoke()
    }
    private func cancelOriginalExchangeTask(id: UInt64, task: Task<CloudPolicyNativeResult, Never>) async {
        // Expiry/shutdown may cancel A and B together. A's cancellation must
        // join B's exact revoker too, before its native cancellation handler.
        do { try await revokeFlushPark(joining: id) }
        catch { return }
        task.cancel()
    }
    private func cancelFlush(id: UUID) async -> (any Error)? {
        guard let original = flushTask, original.id == id else { return nil }
        if let cancellation = original.owner.cancellation { return await cancellation.value }
        guard !original.owner.finished else { return nil }
        original.task.cancel()
        #if DEBUG
        // The passive lane capture also belongs to B; cancellation callbacks
        // need not carry the originating task's locals.
        let cancellation = MaintenanceCallObservation.$capture.withValue(original.owner.observeCall) {
            startFlushCancellation(original)
        }
        #else
        let cancellation = startFlushCancellation(original)
        #endif
        original.owner.cancellation = cancellation
        return await cancellation.value
    }
    private func startFlushCancellation(_ original: OwnedFlush) -> Task<(any Error)?, Never> {
        Task { () -> (any Error)? in
            // Task.cancel does not interrupt exchange.value. Join the original
            // ordered revoker before cancelling A or entering any further Core.
            do { try await original.owner.park?.revoke() }
            catch { return error }
            original.owner.exchange?.cancel()
            if let exchangeID = original.owner.exchangeID, self.exchange == exchangeID {
                await self.adapter.cancelPolicyExchange()
            }
            var failure: (any Error)?
            do { try await self.adapter.cancelPolicyFlush(execution: original.owner.execution) }
            catch { failure = error }
            _ = await original.task.value
            return failure
        }
    }
    private func performFlush(owner: FlushOwner,
                              acknowledgeOutputs: (@Sendable ([CloudPolicyOutput]) async -> Void)?) async -> CloudPolicyNativeResult {
        var delivery = CloudPolicyOutputDelivery(deliver: acknowledgeOutputs)
        var outputs: [CloudPolicyOutput] = []
        let execution = owner.execution
        defer { owner.finished = true }
        do {
            if let exchange = owner.exchange {
                // prepareExecution/validateControl have consumed their real Core
                // results. No flush-native work or output prefix exists yet.
                await delivery.acknowledge(outputs)
                try await owner.park?.enter()
                let result = await exchange.value
                try await owner.park?.revoke()
                if let failure = result.error { throw failure }
                try Task.checkCancellation()
            } else {
                try Task.checkCancellation()
                try await delivery.appendInput(to: &outputs) { try await execution.prepareFlush() }
                await delivery.acknowledge(outputs)
                try Task.checkCancellation()
                try await adapter.flushPolicyCollection(execution: execution)
            }
            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
            await delivery.acknowledge(outputs)
            try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
            await delivery.acknowledge(outputs)
            return .init(outputs: outputs, error: nil, deferred: false, acknowledgedOutputCount: delivery.count)
        } catch {
            let failure = error
            do { try await owner.park?.revoke() }
            catch { return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count) }
            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
            await delivery.acknowledge(outputs)
            _ = try? await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
            await delivery.acknowledge(outputs)
            return .init(outputs: outputs, error: failure, deferred: false, acknowledgedOutputCount: delivery.count)
        }
    }
    private func drainOriginalMedia(_ sdk: String, stopWitness: CloudSDKStopWitness? = nil) async {
        if let stopWitness { mediaRunContexts[sdk]?.attachStopWitness(stopWitness) }
        if let draining = mediaDrains[sdk] {
            #if DEBUG
            existingMediaDrainObserver?(sdk)
            #endif
            await draining.value
            return
        }
        heldMediaResume.removeValue(forKey: sdk)
        let original = mediaRuns[sdk]
        if let context = mediaRunContexts[sdk] { context.cancel(.owned, stopWitness: stopWitness) { original?.cancel() } }
        else { original?.cancel() }
        guard let executor = mediaExecutors.removeValue(forKey: sdk) else { return }
        let task = Task { await executor.close() }
        mediaDrains[sdk] = task
        await task.value
        mediaDrains.removeValue(forKey: sdk)
    }
    private func performMediaRound(_ issued: CloudPolicyCommand, permit: CoreCloudPolicyExecution, instance: String, runID: UUID,
                          worker: CloudMediaExecutor, coreMedia: CoreCloudMediaService, context: CloudMediaCancellationContext,
                          acknowledgeOutputs: (@Sendable ([CloudPolicyOutput]) async -> Void)?) async -> CloudPolicyNativeResult {
        defer {
            if mediaRunIDs[instance] == runID {
                mediaRuns.removeValue(forKey: instance); mediaRunIDs.removeValue(forKey: instance); mediaRunContexts.removeValue(forKey: instance)
            }
        }
        var delivery = CloudPolicyOutputDelivery(deliver: acknowledgeOutputs)
        var outputs: [CloudPolicyOutput] = []
        var selected = (issued, permit)
        do {
            repeat {
                let (issued, permit) = selected
                await delivery.acknowledge(outputs)
                try Task.checkCancellation()
                guard try await permit.beginMedia() else { break }
                let onlyEnvelopes: Bool
                if case .readBlobs = issued.action { onlyEnvelopes = true } else { onlyEnvelopes = false }
                await worker.beginRound()
                try await delivery.appendInput(to: &outputs) { try await permit.report(zones: [.media], kind: .read, started: true) }
                await delivery.acknowledge(outputs)
                var readFailure: (any Error)?
                var sendStopped = false
                do {
                    round: while true {
                        try Task.checkCancellation()
                        try await permit.validate()
                        switch try await worker.runNext(metadataOnly: onlyEnvelopes) {
                        case .idle: break round
                        case .waitingForCollectionDrain:
                            guard !onlyEnvelopes else { throw CloudMediaFailure.metadataScope }
                            try await adapter.waitForMediaFetchDrain(core: coreMedia, execution: permit)
                        case .engineSendRequired:
                            guard !onlyEnvelopes else { throw CloudMediaFailure.metadataScope }
                            do {
                                #if DEBUG
                                try await CloudMediaCancellationContext.$selectedAction.withValue(issued.action) {
                                    try await adapter.sendPolicyMedia(execution: permit)
                                }
                                #else
                                try await adapter.sendPolicyMedia(execution: permit)
                                #endif
                            }
                            catch {
                                context.observe(error)
                                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                                await delivery.acknowledge(outputs)
                                if try await permit.continueMediaAfterSend(error) == false { sendStopped = true; throw error }
                            }
                            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                        case .read(let reply):
                            if let error = reply.failure() { throw error }
                        }
                    }
                } catch { context.observe(error); readFailure = error }
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                #if DEBUG
                await mediaEndReadObserver?(instance)
                #endif
                await delivery.acknowledge(outputs)
                let endedFailure = sendStopped ? nil : readFailure
                do { try await delivery.appendInput(to: &outputs) { try await permit.report(zones: [.media], kind: .read, started: false, error: endedFailure) } }
                catch {
                    if context.observeEndReadReportFailure(error, primary: readFailure) {
                        #if DEBUG
                        DebugCloudDiagnostics.record(.policyExecute, action: issued.action, error: error)
                        #endif
                    }
                    if readFailure == nil { readFailure = error }
                }
                if let readFailure { throw readFailure }
                await delivery.acknowledge(outputs)
                try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
                guard let queuedResume = heldMediaResume.removeValue(forKey: instance) else { break }
                selected = queuedResume
            } while true
            return .init(outputs: outputs, error: nil, deferred: false, acknowledgedOutputCount: delivery.count)
        } catch {
            var mediaCancellation = context.observe(error)
            #if DEBUG
            DebugCloudDiagnostics.record(.policyExecute, action: selected.0.action, error: error)
            #endif
            if let queuedResume = heldMediaResume.removeValue(forKey: instance), let mediaTransport {
                let replacement: CloudMediaExecutor
                if let retained = mediaExecutors[instance] { replacement = retained }
                else {
                    replacement = CloudMediaExecutor(core: coreMedia, sdk: instance, transport: mediaTransport, freshAccountRead: { try await self.adapter.refreshControlIdentity() })
                    mediaExecutors[instance] = replacement
                }
                let replacementID = UUID()
                let replacementContext = CloudMediaCancellationContext(sdk: instance)
                await delivery.acknowledge(outputs)
                let pendingRun = Task {
                    await CloudMediaCancellationContext.$current.withValue(replacementContext) {
                        await self.performMediaRound(queuedResume.0, permit: queuedResume.1, instance: instance, runID: replacementID, worker: replacement, coreMedia: coreMedia, context: replacementContext, acknowledgeOutputs: acknowledgeOutputs)
                    }
                }
                mediaRuns[instance] = pendingRun; mediaRunIDs[instance] = replacementID; mediaRunContexts[instance] = replacementContext
                let completed = await pendingRun.value
                outputs.append(contentsOf: completed.outputs)
                delivery.includeAcknowledgedPrefix(completed.acknowledgedOutputCount)
                if let failure = completed.error {
                    mediaCancellation = mediaCancellation && completed.mediaCancellation && failure is CancellationError
                }
            }
            await delivery.acknowledge(outputs)
            do { try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) } }
            catch { mediaCancellation = context.observe(error) && mediaCancellation }
            return .init(outputs: outputs, error: error, deferred: false, mediaCancellation: mediaCancellation, acknowledgedOutputCount: delivery.count)
        }
    }

    private func waitForReadiness(_ command: CloudPolicyCommand) async throws -> CloudPolicyEarlyEnd? {
        let policyChanges = await policy.readinessChanges()
        let nativeChanges = await adapter.readinessChanges()
        let (changes, changeContinuation) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        let policyObserver = Task { for await _ in policyChanges { changeContinuation.yield(()) } }
        let nativeObserver = Task { for await _ in nativeChanges { changeContinuation.yield(()) } }
        defer { policyObserver.cancel(); nativeObserver.cancel(); changeContinuation.finish() }
        let policy = self.policy; let adapter = self.adapter; let clock = readinessClock
        let decision = CloudPolicyReadinessDecision()
        return try await withThrowingTaskGroup(of: CloudPolicyEarlyEnd?.self) { group in
            group.addTask {
                var events = changes.makeAsyncIterator()
                while true {
                    try Task.checkCancellation()
                    switch try await policy.exchangeReadiness(command, nativeReady: await adapter.policySDKReady()) {
                    case .ready:
                        if decision.observeReady() { return nil }
                        switch try await policy.exchangeReadiness(command, nativeReady: false, expired: true) {
                        case .end(let outcome): return CloudPolicyEarlyEnd(outcome)
                        default: throw BridgeError.cloudPolicy(.unqualifiedCompletion)
                        }
                    case .end(let outcome): return CloudPolicyEarlyEnd(outcome)
                    case .wait:
                        guard await events.next() != nil else { throw CancellationError() }
                    }
                }
            }
            group.addTask {
                try await clock.waitForDeadline()
                // Freeze the observed ordering BEFORE any awaited lookup/query.
                guard decision.expire() else { return nil }
                // The core qualifies timeout; Swift owns only the one deadline.
                switch try await policy.exchangeReadiness(command, nativeReady: await adapter.policySDKReady(), expired: true) {
                case .ready: throw BridgeError.cloudPolicy(.unqualifiedCompletion)
                case .end(let outcome): return CloudPolicyEarlyEnd(outcome)
                case .wait: throw BridgeError.cloudPolicy(.unqualifiedCompletion)
                }
            }
            defer { group.cancelAll() }
            return try await group.next()!
        }
    }
    private func performExchange(_ command: CloudPolicyCommand, id: UInt64,
                                 acknowledgeOutputs: (@Sendable ([CloudPolicyOutput]) async -> Void)?) async -> CloudPolicyNativeResult {
        var delivery = CloudPolicyOutputDelivery(deliver: acknowledgeOutputs)
        var outputs: [CloudPolicyOutput] = []
        #if DEBUG
        let diagnostic = DebugCloudDiagnostics.sendFailureCapture
        var site = DebugCloudDiagnostics.SendFailureSite.exchangeReadiness
        #endif
        var reservation: UUID?
        do {
            var captured = false
            var failure: (any Error)?
            do {
                let readiness = try await waitForReadiness(command)
                let early: CloudPolicyEarlyEnd?
                if let readiness { early = readiness }
                else {
                    #if DEBUG
                    site = .exchangeGate
                    #endif
                    early = try await exchangeGate?.waitBeforeCapture(command)
                }
                if let outcome = early {
                    #if DEBUG
                    site = .exchangeEndBeforeCapture
                    #endif
                    try await delivery.appendInput(to: &outputs) { [policy] in try await policy.endBeforeCapture(id, outcome: outcome) }
                } else {
                    #if DEBUG
                    site = .exchangeReservation
                    #endif
                    try Task.checkCancellation()
                    // Readiness retains its original deadline scope. Only
                    // causal native admission moves before Core preparation.
                    let reserved = try await adapter.reservePolicyExchange()
                    reservation = reserved
                    #if DEBUG
                    site = .exchangePreparation
                    #endif
                    guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                    let preparedExchange = try await adapter.preparePolicyExchange(execution: execution)
                    try Task.checkCancellation()
                    #if DEBUG
                    site = .exchangeCapture
                    #endif
                    let capture = try await policy.captureExchange(command)
                    outputs.append(capture.output)
                    await delivery.acknowledge(outputs)
                    if let runtimeID = capture.runtimeExchange {
                        captured = true
                        await observeExchangeCapture?(command, runtimeID)
                        #if DEBUG
                        site = .exchangeRun
                        #endif
                        try await adapter.runPolicyExchange(runtimeID: runtimeID, execution: execution, prepared: preparedExchange)
                    }
                }
            } catch {
                #if DEBUG
                diagnostic?.record(site, error: error)
                DebugCloudDiagnostics.record(.policyExchange, action: command.action, error: error)
                #endif
                failure = error
            }
            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
            await delivery.acknowledge(outputs)
            if captured {
                #if DEBUG
                site = .exchangeCompletion
                #endif
                try await delivery.appendInput(to: &outputs) { [policy] in try await policy.completeExchange(id) }
            } else if let failure {
                #if DEBUG
                site = .exchangeEndBeforeCapture
                #endif
                let outcome: CloudPolicyEarlyEnd = failure is CancellationError ? .cancelled : .failed(.other)
                try await delivery.appendInput(to: &outputs) { [policy] in try await policy.endBeforeCapture(id, outcome: outcome) }
            }
            if let reservation { await adapter.releasePolicyExchange(reservation) }
            reservation = nil
            await delivery.acknowledge(outputs)
            if let held = heldConfigure {
                heldConfigure = nil
                do {
                    let adapter = self.adapter
                    let policy = self.policy
                    #if DEBUG
                    site = .heldConfiguration
                    #endif
                    let configure = Task { try await adapter.executePolicyLifecycle(held.0, execution: held.1, policy: policy) }
                    _ = try await configure.value
                    outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                    await delivery.acknowledge(outputs)
                    #if DEBUG
                    site = .policyRefresh
                    #endif
                    try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
                } catch {
                    #if DEBUG
                    diagnostic?.record(site, error: error)
                    DebugCloudDiagnostics.record(.policyExchange, action: held.0.action, error: error)
                    #endif
                    if failure == nil { failure = error }
                }
            }
            await delivery.acknowledge(outputs)
            return .init(outputs: outputs, error: failure, deferred: false, acknowledgedOutputCount: delivery.count)
        } catch {
            #if DEBUG
            diagnostic?.record(site, error: error)
            DebugCloudDiagnostics.record(.policyExchange, action: command.action, error: error)
            #endif
            if let reservation { await adapter.releasePolicyExchange(reservation) }
            await delivery.acknowledge(outputs)
            _ = try? await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
            await delivery.acknowledge(outputs)
            return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count)
        }
    }
}
