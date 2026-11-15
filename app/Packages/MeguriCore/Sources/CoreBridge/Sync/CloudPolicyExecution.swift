import Foundation
import Instrumentation
import Synchronization
internal import MeguriCoreBindings

#if DEBUG
// Module-private test hold for an issued ResumeMedia whose real preparation
// and pinned SDK lookup have returned, before it joins a drain or registers a
// run/context. Backend before/afterExecution cannot suspend in this interval.
enum CloudMediaRegistrationBoundary {
    struct Observation: Sendable {
        let command: CloudPolicyCommand
        let sdk: String
        let runRegistered: Bool
        let contextRegistered: Bool
    }
    @TaskLocal static var afterOriginalLookup: (@Sendable (Observation) async -> Void)?
}
#endif

/// Carries only the adapter reservation back from the readiness worker. The
/// existing readiness task group joins that worker before cleanup takes it.
private final class CloudExchangeReservation: Sendable {
    private let value = Mutex<UUID?>(nil)
    func store(_ id: UUID) { value.withLock { $0 = id } }
    func take() -> UUID? { value.withLock { let id = $0; $0 = nil; return id } }
}

/// Only CoreCloudPolicyService can obtain this retained Rust capability.
struct CoreCloudPolicyExecution: Sendable {
    let raw: MeguriCoreBindings.CloudPolicyExecution
    init(_ raw: MeguriCoreBindings.CloudPolicyExecution) { self.raw = raw }
    func syncNowEpochRead() async throws -> CoreCloudFetchRead? {
        let raw = self.raw
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionSyncNowEpochRead) {
            try raw.syncNowEpochRead().map(CoreCloudFetchRead.init)
        }
    }
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

/// Outcome evidence retains the issued command, execution and SDK observation.
/// The coordinator separately checks its retained backend and request ownership.
public struct CloudPolicyNativeCancellation: Sendable {
    private let command: CloudPolicyCommand
    private let execution: CoreCloudPolicyExecution
    private let sdk: String
    private let dependency: CloudQuiescentDependencyWait?
    private let original: CloudSDKStopWitness?
    private let observation: CloudPolicyCancellationContext.Snapshot?
    fileprivate init(command: CloudPolicyCommand, execution: CoreCloudPolicyExecution, sdk: String,
                     dependency: CloudQuiescentDependencyWait? = nil,
                     original: CloudSDKStopWitness? = nil,
                     cause: CloudPolicyCancellationContext.Snapshot? = CloudPolicyCancellationContext.current?.capture()) {
        self.command = command
        self.execution = execution
        self.sdk = sdk
        self.dependency = dependency
        self.original = original
        self.observation = cause
    }
    init(adapterEdge edge: CloudAdapterNativeCancellationEdge) {
        self.init(command: edge.command, execution: edge.execution, sdk: edge.sdk,
                  dependency: edge.dependency, original: edge.original, cause: edge.cause)
    }
    public func matches(_ candidate: CloudPolicyCommand) -> Bool { candidate.raw === command.raw }
    public func matchesIntent(_ context: CloudPolicyCancellationContext) -> Bool {
        guard let observation, observation.ownerID == context.id else { return false }
        switch observation.request?.source {
        case .some(.expiry), .some(.replacement): return true
        default: return false
        }
    }
    public func ownsDeadline(_ context: CloudPolicyCancellationContext) -> Bool {
        observation?.ownerID == context.id && observation?.request?.source == .deadline
    }
    fileprivate func dependentFlush(issuer: CloudFetchProofIssuer, dependency: CloudQuiescentDependencyWait,
                                    cause: CloudPolicyCancellationContext.Snapshot,
                                    receipt: CloudFlushCancellationReceipt) -> CloudPolicyNativeCancellation? {
        guard receipt.matches(issuer), let original, original === issuer.original,
              sdk == original.sdk, let observation,
              cause.sharesEligibleRequest(with: observation) else { return nil }
        let edge = CloudAdapterNativeCancellationEdge(issuer: issuer, dependency: dependency, cause: cause)
        return CloudPolicyNativeCancellation(adapterEdge: edge)
    }
}
struct CloudNativeCancellationError: Error {
    let error: any Error
    let proof: CloudPolicyNativeCancellation
}

/// A coordinator owns this executor alongside its ONE adapter. Outputs include
/// required failure follow-ups. Deferred configuration is not completed work.
public struct CloudPolicyNativeResult: Sendable {
    public let outputs: [CloudPolicyOutput]
    public let error: (any Error)?
    /// Diagnostic only when required follow-up work replaces the original fault.
    public let priorError: (any Error)?
    public let originalGuardRefusal: CloudPolicyOriginalGuardRefusal?
    public let dependentFetchRefusal: CloudPolicyDependentFetchRefusal?
    public let fetchProofOutcome: CloudPolicyFetchProofOutcome?
    public let nativeCancellation: CloudPolicyNativeCancellation?
    public let deferred: Bool
    public let mediaCancellation: Bool
    public let acknowledgedOutputCount: Int
    public let afterOutputs: (@Sendable () async -> CloudPolicyNativeResult)?
    /// The original cleanup failure is diagnostic evidence, outside app failure cycles.
    public let zoneDeletionError: (any Error)?
    public let cleanupInventoryError: (any Error)?
    init(outputs: [CloudPolicyOutput], error: (any Error)?, deferred: Bool, zoneDeletionError: (any Error)? = nil, cleanupInventoryError: (any Error)? = nil, mediaCancellation: Bool = false, acknowledgedOutputCount: Int = 0, afterOutputs: (@Sendable () async -> CloudPolicyNativeResult)? = nil, originalGuardRefusal: CloudPolicyOriginalGuardRefusal? = nil, dependentFetchRefusal: CloudPolicyDependentFetchRefusal? = nil, fetchProofOutcome: CloudPolicyFetchProofOutcome? = nil, nativeCancellation: CloudPolicyNativeCancellation? = nil, priorError: (any Error)? = nil) {
        self.outputs = outputs; self.error = error; self.deferred = deferred; self.zoneDeletionError = zoneDeletionError; self.cleanupInventoryError = cleanupInventoryError
        self.mediaCancellation = mediaCancellation; self.acknowledgedOutputCount = acknowledgedOutputCount
        self.afterOutputs = afterOutputs; self.originalGuardRefusal = originalGuardRefusal
        self.dependentFetchRefusal = dependentFetchRefusal; self.fetchProofOutcome = fetchProofOutcome
        self.nativeCancellation = nativeCancellation; self.priorError = priorError
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
    private var nativeExchangeCancellation: CloudPolicyCancellationContext?
    private final class FlushCancellation: Sendable {
        private enum Completion { case dependency, cancelled(CloudPolicyCancellationContext.Snapshot) }
        private struct State {
            var finished = false
            var cause: CloudPolicyCancellationContext.Snapshot?
            var originalCompletion: Completion?
        }
        private let state = Mutex(State())
        func request(_ cause: CloudPolicyCancellationContext.Snapshot) {
            state.withLock {
                guard !$0.finished else { return }
                if $0.cause == nil { $0.cause = cause }
                // This is the actual owner callback/deadline/control boundary.
                // Publish its ordering before any asynchronous cancellation hop.
                if $0.originalCompletion == nil { $0.originalCompletion = .cancelled(cause) }
            }
        }
        var cause: CloudPolicyCancellationContext.Snapshot? { state.withLock { $0.cause } }
        func originalCancelled(_ cause: CloudPolicyCancellationContext.Snapshot) {
            state.withLock { if $0.originalCompletion == nil { $0.originalCompletion = .cancelled(cause) } }
        }
        func dependencyCompleted() {
            state.withLock { if $0.originalCompletion == nil { $0.originalCompletion = .dependency } }
        }
        var originalCause: CloudPolicyCancellationContext.Snapshot? {
            state.withLock {
                if case .some(.cancelled(let cause)) = $0.originalCompletion { return cause }
                return nil
            }
        }
        func finish() { state.withLock { $0.finished = true } }
    }
    private final class FlushDependencyResult: Sendable {
        private struct Storage {
            var result: Result<CloudPolicyNativeResult, any Error>?
            var waiter: CheckedContinuation<CloudPolicyNativeResult, any Error>?
        }
        private let storage = Mutex(Storage())
        func finish(_ result: Result<CloudPolicyNativeResult, any Error>) {
            let waiter = storage.withLock { value -> CheckedContinuation<CloudPolicyNativeResult, any Error>? in
                guard value.result == nil else { return nil }
                value.result = result
                let waiter = value.waiter
                value.waiter = nil
                return waiter
            }
            waiter?.resume(with: result)
        }
        func wait() async throws -> CloudPolicyNativeResult {
            try await withCheckedThrowingContinuation { continuation in
                let ready = storage.withLock { value -> Result<CloudPolicyNativeResult, any Error>? in
                    if let result = value.result { return result }
                    value.waiter = continuation
                    return nil
                }
                if let ready { continuation.resume(with: ready) }
            }
        }
    }
    private final class FlushOwner {
        let id = UUID()
        let execution: CoreCloudPolicyExecution
        let exchange: Task<CloudPolicyNativeResult, Never>?
        let exchangeID: UInt64?
        let issuer: CloudFetchProofIssuer?
        let intent: CloudPolicyCancellationContext
        let exchangeIntent: CloudPolicyCancellationContext?
        let cancellationEdge = FlushCancellation()
        let dependencyResult = FlushDependencyResult()
        var nativeReceipt: CloudFlushCancellationReceipt?
        // A's completed outcome, retained only after B's ordered revoke and
        // required output/refresh prefix. It never serves as B's command proof.
        var dependencyCancellation: CloudPolicyNativeCancellation?
        let park: CloudQuiescentDependencyWait?
        #if DEBUG
        let observeCall = MaintenanceCallObservation.capture
        #endif
        var cancellation: Task<(any Error)?, Never>?
        init(execution: CoreCloudPolicyExecution, issuer: CloudFetchProofIssuer?,
             exchange: Task<CloudPolicyNativeResult, Never>?, exchangeID: UInt64?, exchangeIntent: CloudPolicyCancellationContext?) {
            self.execution = execution; self.exchange = exchange; self.exchangeID = exchangeID
            self.issuer = issuer; self.exchangeIntent = exchangeIntent
            intent = CloudPolicyCancellationContext.current ?? CloudPolicyCancellationContext()
            park = CloudQuiescentDependencyWait.retained()
        }
    }
    private typealias OwnedFlush = (id: UUID, task: Task<CloudPolicyNativeResult, Never>, owner: FlushOwner)
    private var flushTask: OwnedFlush?
    // A failed ordered removal outlives B. Every later cancellation of the same
    // predecessor must still join this fence until A actually completes.
    private var failedFlushRevocations: [UInt64: CloudQuiescentDependencyWait] = [:]
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
                    try await Task.sleep(for: .seconds(max(0, Double(until) / 1_000 - Date().timeIntervalSince1970)), tolerance: .zero)
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
    private func deferredRefreshAfterFailure(_ originalError: any Error) -> @Sendable () async -> CloudPolicyNativeResult {
        let returned = CloudPolicyOutputAcknowledgement.current
        let input = CloudPolicyOutputAcknowledgement.input
        return {
            await CloudPolicyOutputAcknowledgement.$current.withValue(returned) {
                await CloudPolicyOutputAcknowledgement.$input.withValue(input) {
                    await self.refreshAfterFailure(originalError)
                }
            }
        }
    }
    private func refreshAfterFailure(_ originalError: any Error) async -> CloudPolicyNativeResult {
        do {
            let followup = try await nextPolicyInput { [policy] in try await policy.input(.refresh(.cloud)) }
            return .init(outputs: [followup], error: nil, deferred: false,
                acknowledgedOutputCount: CloudPolicyOutputAcknowledgement.input == nil ? 0 : 1)
        } catch {
            return .init(outputs: [], error: error, deferred: false, priorError: originalError)
        }
    }
    public func quiesce() async throws {
        quiescing = true
        // Keep the exact predecessor live if ordered removal has failed.
        // Adapter quiescence itself cancels proof, worker and SDK operations.
        if let exchange { try await revokeFlushPark(joining: exchange) }
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
        var requiredRefreshStarted = false
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
                #if DEBUG
                if case .resumeMedia = command.action { site = .mediaQuiescing }
                #endif
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
                if case .stopEngine = command.action, let exchange {
                    try await revokeFlushPark(joining: exchange)
                }
                if try await adapter.executePolicyLifecycle(command, execution: execution, policy: policy) {
                    return .init(outputs: [], error: nil, deferred: true)
                }
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                await delivery.acknowledge(outputs)
                requiredRefreshStarted = true
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
                requiredRefreshStarted = true
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
                #if DEBUG
                site = .mediaPreparation
                #endif
                let prepared: CoreCloudPolicyExecution?
                if case .resumeMedia = command.action { prepared = try await policy.prepareExecutionAttempt(command) }
                else { prepared = try await policy.prepareExecution(command) }
                guard let permit = prepared else {
                    // P4: a still-current ResumeMedia with no running SDK is
                    // Core's NoOperation, independent of cancellation.
                    if case .resumeMedia = command.action { return .init(outputs: [], error: nil, deferred: false) }
                    throw BridgeError.cloudPolicy(.staleCommand)
                }
                #if DEBUG
                site = .mediaOriginalSDK
                #endif
                let instance = try await permit.originalSDK()
                if case .pauseMedia = command.action {
                    if let instance { await drainOriginalMedia(instance) }
                    _ = try await permit.beginMedia()
                    try await adapter.waitForDeferredUpdates()
                    await delivery.acknowledge(outputs)
                    requiredRefreshStarted = true
                    try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
                } else if case .prioritizeMedia = command.action {
                    _ = try await permit.beginMedia()
                } else {
                    guard let instance else { throw BridgeError.cloudPolicy(.staleCommand) }
                    #if DEBUG
                    if case .resumeMedia = command.action, let observe = CloudMediaRegistrationBoundary.afterOriginalLookup {
                        await observe(.init(command: command, sdk: instance,
                            runRegistered: mediaRuns[instance] != nil, contextRegistered: mediaRunContexts[instance] != nil))
                    }
                    #endif
                    if case .resumeMedia = command.action,
                       try await adapter.policyMediaAwaitsCreation(instance, execution: permit) {
                        return .init(outputs: [], error: nil, deferred: false)
                    }
                    if let joining = mediaDrains[instance] { await joining.value }
                    #if DEBUG
                    site = .mediaQuiescing
                    #endif
                    guard !quiescing else { throw CancellationError() }
                    #if DEBUG
                    site = .mediaPreNetwork
                    #endif
                    // Only this explicit check, after successful original
                    // preparation and SDK lookup, proves pre-network cancellation.
                    do { try Task.checkCancellation() }
                    catch {
                        throw CloudNativeCancellationError(error: error, proof:
                            .init(command: command, execution: permit, sdk: instance))
                    }
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
                let issuer = try await adapter.retainFlushIssuer(command: command, execution: execution)
                let owner = FlushOwner(execution: execution, issuer: issuer, exchange: nativeExchangeTask,
                    exchangeID: exchange, exchangeIntent: nativeExchangeCancellation)
                let id = owner.id
                let cancellationEdge = owner.cancellationEdge
                let intent = owner.intent
                let task = Task { await self.performFlush(owner: owner, acknowledgeOutputs: acknowledgeOutputs) }
                flushTask = (id, task, owner)
                let deadline = Task<Result<CloudPolicyOutput, any Error>?, Never> {
                    do {
                        try await self.flushDeadline(until)
                        try Task.checkCancellation()
                    } catch { return nil }
                    let observation = intent.capture(deadlineReached: true)
                    cancellationEdge.request(observation)
                    if let failure = await self.cancelFlush(id: id) { return .failure(failure) }
                    do { return .success(try await self.nextPolicyInput { [policy] in try await policy.input(.backgroundExpiring) }) }
                    catch { return .failure(error) }
                }
                let result = await withTaskCancellationHandler { await task.value }
                    onCancel: {
                        let observation = intent.capture()
                        cancellationEdge.request(observation)
                        Task { _ = await self.cancelFlush(id: id) }
                    }
                deadline.cancel()
                let expired: CloudPolicyOutput?
                let deadlineFailure: (any Error)?
                switch await deadline.value {
                case .some(.success(let output)): expired = output; deadlineFailure = nil
                case .some(.failure(let error)): expired = nil; deadlineFailure = error
                case nil: expired = nil; deadlineFailure = nil
                }
                // A completed flush body can race its caller's cancellation.
                // Keep B tracked through the original cancellation's native/Core
                // work as well as its actual task result.
                // Join even when the synchronous request raced the actor hop
                // and the original body has already returned. Cleanup failure
                // replaces cancellation eligibility, never the output prefix.
                let cancellationFailure: (any Error)?
                if cancellationEdge.cause != nil { cancellationFailure = await cancelFlush(id: id) }
                else { cancellationFailure = nil }
                if flushTask?.id == id { flushTask = nil }
                let proof: CloudPolicyNativeCancellation?
                if cancellationFailure == nil, deadlineFailure == nil, result.error is CancellationError,
                   result.fetchProofOutcome?.preservesFailure != true,
                   let dependency = owner.dependencyCancellation, let issuer = owner.issuer,
                   let park = owner.park, let cause = cancellationEdge.originalCause,
                   let receipt = owner.nativeReceipt {
                    proof = dependency.dependentFlush(issuer: issuer, dependency: park, cause: cause, receipt: receipt)
                } else { proof = nil }
                return .init(outputs: result.outputs + (expired.map { [$0] } ?? []),
                    error: cancellationFailure ?? deadlineFailure ?? result.error, deferred: result.deferred,
                    acknowledgedOutputCount: result.acknowledgedOutputCount + (expired == nil || CloudPolicyOutputAcknowledgement.input == nil ? 0 : 1),
                    fetchProofOutcome: cancellationFailure == nil && deadlineFailure == nil ? result.fetchProofOutcome : nil,
                    nativeCancellation: proof, priorError: result.priorError)
            case .cancelBackgroundFlush:
                // Removal remains required even after the dependent returned.
                if let exchange { try await revokeFlushPark(joining: exchange) }
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
                defer { exchange = nil; failedFlushRevocations.removeValue(forKey: id) }
                let cancellationIntent = CloudPolicyCancellationContext.current ?? CloudPolicyCancellationContext()
                let task = Task {
                    await CloudPolicyCancellationContext.$current.withValue(cancellationIntent) {
                    #if DEBUG
                    return await CloudMediaCancellationContext.$selectedAction.withValue(command.action) {
                        await self.performExchange(command, id: id, acknowledgeOutputs: acknowledgeOutputs)
                    }
                    #else
                    return await self.performExchange(command, id: id, acknowledgeOutputs: acknowledgeOutputs)
                    #endif
                    }
                }
                nativeExchangeTask = task
                nativeExchangeCancellation = cancellationIntent
                defer { nativeExchangeTask = nil; nativeExchangeCancellation = nil }
                return await withTaskCancellationHandler { await task.value } onCancel: {
                    _ = cancellationIntent.capture()
                    Task { await self.cancelOriginalExchangeTask(id: id, task: task) }
                }
            case .cancelExchange(let id):
                try await revokeFlushPark(joining: id)
                let execution = try await policy.prepareExecution(command)
                if let execution, exchange == id {
                    let intent = nativeExchangeCancellation
                    if let intent { try await adapter.recordAccountExchangeCancellation(command, execution: execution, intent: intent) }
                    guard exchange == id, nativeExchangeCancellation === intent else {
                        return .init(outputs: outputs, error: nil, deferred: false, acknowledgedOutputCount: delivery.count)
                    }
                    _ = intent?.capture()
                    nativeExchangeTask?.cancel(); await adapter.cancelPolicyExchange()
                }
            }
            return .init(outputs: outputs, error: nil, deferred: false, acknowledgedOutputCount: delivery.count)
        } catch let caught {
            // Qualification belongs only to this exact fault. Required refresh
            // failure replaces it without inheriting any earlier qualification.
            let attempt = caught as? CloudPolicyAttemptError
            let dependent = caught as? CloudDependentFetchRefusalError
            let proof = caught as? CloudFetchProofError
            let cancellation = caught as? CloudNativeCancellationError
            let error: any Error
            if let attempt { error = attempt.error } else if let dependent { error = dependent.error } else if let proof { error = proof.error } else if let cancellation { error = cancellation.error } else { error = caught }
            let refusal = attempt?.refusal
            #if DEBUG
            switch command.action {
            case .requestSend, .updatePending, .resumeMedia:
                DebugCloudDiagnostics.sendFailureCapture?.record(site, error: error)
            default: break
            }
            DebugCloudDiagnostics.record(.policyExecute, action: command.action, error: error)
            #endif
            if requiredRefreshStarted {
                return .init(outputs: outputs, error: error, deferred: false,
                    acknowledgedOutputCount: delivery.count)
            }
            // A failed native event transaction may have stopped tracking. Its
            // real projection supplies StopEngine and any other required work.
            if CloudPolicyOutputAcknowledgement.input != nil {
                switch command.action {
                case .updatePending, .applyReady, .recordExchange, .recordQuietPoint, .probeTracking, .statusChanged:
                    // Return the failed local computation through its original
                    // ACK before the later refresh takes another input turn.
                    return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count,
                        afterOutputs: deferredRefreshAfterFailure(caught), originalGuardRefusal: refusal)
                default: break
                }
            }
            await delivery.acknowledge(outputs)
            do { try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) } }
            catch {
                return .init(outputs: outputs, error: error, deferred: false,
                    acknowledgedOutputCount: delivery.count, priorError: caught)
            }
            return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count, originalGuardRefusal: refusal, dependentFetchRefusal: dependent?.qualification, fetchProofOutcome: proof?.outcome, nativeCancellation: cancellation?.proof ?? proof?.outcome.nativeCancellation)
        }
    }
    private func performCleanup(_ command: CloudPolicyCommand, corePrepared: @Sendable () async -> Void) async -> CloudPolicyNativeResult {
        var requiredRefreshStarted = false
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
            requiredRefreshStarted = true
            let projection = try await nextPolicyInput { [policy] in try await policy.input(.refresh(.cloud)) }
            return .init(outputs: [projection], error: nil, deferred: false, cleanupInventoryError: inventoryFailure, acknowledgedOutputCount: CloudPolicyOutputAcknowledgement.input == nil ? 0 : 1)
        } catch {
            CloudCleanupDiagnostics.record(error)
            if requiredRefreshStarted { return .init(outputs: [], error: error, deferred: false) }
            if CloudPolicyOutputAcknowledgement.input != nil {
                return .init(outputs: [], error: error, deferred: false,
                    afterOutputs: deferredRefreshAfterFailure(error))
            }
            // A failed required computation may have stopped tracking. Return
            // its actual projection so lifecycle follow-ups can still drain.
            let originalError = error
            let refreshed = await refreshAfterFailure(originalError)
            return .init(outputs: refreshed.outputs, error: refreshed.error ?? originalError, deferred: false,
                acknowledgedOutputCount: refreshed.acknowledgedOutputCount, priorError: refreshed.priorError)
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
        if let failedFence = failedFlushRevocations[exchangeID] { try await failedFence.revoke() }
        if let dependent = flushTask, dependent.owner.exchangeID == exchangeID {
            do { try await dependent.owner.park?.revoke() }
            catch {
                if failedFlushRevocations[exchangeID] == nil, let fence = dependent.owner.park {
                    failedFlushRevocations[exchangeID] = fence
                }
                throw error
            }
        }
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
        if original.owner.cancellationEdge.cause == nil {
            let observation = original.owner.intent.capture()
            original.owner.cancellationEdge.request(observation)
        }
        guard original.owner.cancellationEdge.cause != nil else { return nil }
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
            do {
                if let exchangeID = original.owner.exchangeID { try await self.revokeFlushPark(joining: exchangeID) }
                try await original.owner.park?.revoke()
            } catch {
                if let predecessor = original.owner.exchangeID, let fence = original.owner.park {
                    if self.failedFlushRevocations[predecessor] == nil {
                        self.failedFlushRevocations[predecessor] = fence
                    }
                }
                original.owner.dependencyResult.finish(.failure(error))
                return error
            }
            let predecessorObservation = original.owner.cancellationEdge.cause
            _ = original.owner.exchangeIntent?.capture(predecessor: predecessorObservation)
            original.owner.exchange?.cancel()
            if let exchangeID = original.owner.exchangeID, self.exchange == exchangeID {
                await self.adapter.cancelPolicyExchange()
            }
            var failure: (any Error)?
            do { original.owner.nativeReceipt = try await self.adapter.cancelPolicyFlush(execution: original.owner.execution) }
            catch { failure = error }
            _ = await original.task.value
            return failure
        }
    }
    private func performFlush(owner: FlushOwner,
                              acknowledgeOutputs: (@Sendable ([CloudPolicyOutput]) async -> Void)?) async -> CloudPolicyNativeResult {
        var delivery = CloudPolicyOutputDelivery(deliver: acknowledgeOutputs)
        var outputs: [CloudPolicyOutput] = []
        var requiredRefreshStarted = false
        let execution = owner.execution
        var dependencyOutcome: CloudPolicyFetchProofOutcome?
        var predecessorCancellation: CloudPolicyNativeCancellation?
        defer { owner.cancellationEdge.finish() }
        do {
            if let exchange = owner.exchange {
                // prepareExecution/validateControl have consumed their real Core
                // results. No flush-native work or output prefix exists yet.
                await delivery.acknowledge(outputs)
                try await owner.park?.enter()
                let edge = owner.cancellationEdge
                let context = owner.intent
                let dependencyResult = owner.dependencyResult
                Task {
                    let completed = await exchange.value
                    edge.dependencyCompleted()
                    dependencyResult.finish(.success(completed))
                }
                let result = try await withTaskCancellationHandler {
                    try await dependencyResult.wait()
                } onCancel: {
                    let observation = context.capture()
                    edge.originalCancelled(observation)
                }
                dependencyOutcome = result.fetchProofOutcome
                try await owner.park?.revoke()
                predecessorCancellation = result.nativeCancellation
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
            requiredRefreshStarted = true
            try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
            await delivery.acknowledge(outputs)
            return .init(outputs: outputs, error: nil, deferred: false, acknowledgedOutputCount: delivery.count)
        } catch {
            let failure = error
            if requiredRefreshStarted {
                return .init(outputs: outputs, error: failure, deferred: false, acknowledgedOutputCount: delivery.count)
            }
            do { try await owner.park?.revoke() }
            catch { return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count,
                priorError: failure) }
            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
            await delivery.acknowledge(outputs)
            do { try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) } }
            catch {
                await delivery.acknowledge(outputs)
                return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count,
                    priorError: failure)
            }
            await delivery.acknowledge(outputs)
            owner.dependencyCancellation = predecessorCancellation
            return .init(outputs: outputs, error: failure, deferred: false, acknowledgedOutputCount: delivery.count,
                fetchProofOutcome: dependencyOutcome)
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
        var requiredRefreshStarted = false
        var selected = (issued, permit)
        #if DEBUG
        var failureSite: DebugCloudDiagnostics.SendFailureSite = .mediaCancellationCheck
        #endif
        do {
            repeat {
                let (issued, permit) = selected
                await delivery.acknowledge(outputs)
                #if DEBUG
                failureSite = .mediaCancellationCheck
                #endif
                try Task.checkCancellation()
                #if DEBUG
                failureSite = .mediaBegin
                #endif
                guard try await permit.beginMedia() else {
                    // A prepared original may outlive its configure drain
                    // without ever registering a worker there. Release this
                    // unstarted executor instead of retaining one per old SDK.
                    heldMediaResume.removeValue(forKey: instance)
                    if mediaExecutors[instance] === worker { mediaExecutors.removeValue(forKey: instance) }
                    await worker.close()
                    break
                }
                let onlyEnvelopes: Bool
                if case .readBlobs = issued.action { onlyEnvelopes = true } else { onlyEnvelopes = false }
                await worker.beginRound()
                #if DEBUG
                failureSite = .mediaReportBegin
                #endif
                try await delivery.appendInput(to: &outputs) { try await permit.report(zones: [.media], kind: .read, started: true) }
                await delivery.acknowledge(outputs)
                var readFailure: (any Error)?
                var sendStopped = false
                do {
                    round: while true {
                        #if DEBUG
                        failureSite = .mediaCancellationCheck
                        #endif
                        try Task.checkCancellation()
                        #if DEBUG
                        failureSite = .mediaValidation
                        #endif
                        try await permit.validate()
                        #if DEBUG
                        failureSite = .mediaNextWork
                        #endif
                        switch try await worker.runNext(metadataOnly: onlyEnvelopes) {
                        case .idle: break round
                        case .waitingForCollectionDrain:
                            #if DEBUG
                            failureSite = .mediaDrainWait
                            #endif
                            guard !onlyEnvelopes else { throw CloudMediaFailure.metadataScope }
                            try await adapter.waitForMediaFetchDrain(core: coreMedia, execution: permit)
                        case .engineSendRequired:
                            #if DEBUG
                            failureSite = .mediaSendWorkflow
                            #endif
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
                                #if DEBUG
                                DebugCloudDiagnostics.sendFailureCapture?.record(failureSite, error: error)
                                #endif
                                context.observe(error)
                                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                                await delivery.acknowledge(outputs)
                                #if DEBUG
                                failureSite = .mediaContinueAfterSend
                                #endif
                                if try await permit.continueMediaAfterSend(error) == false { sendStopped = true; throw error }
                            }
                            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                        case .read(let reply):
                            #if DEBUG
                            failureSite = .mediaReadReply
                            #endif
                            if let error = reply.failure() { throw error }
                        }
                    }
                } catch {
                    #if DEBUG
                    DebugCloudDiagnostics.sendFailureCapture?.record(failureSite, error: error)
                    #endif
                    context.observe(error); readFailure = error
                }
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                #if DEBUG
                await mediaEndReadObserver?(instance)
                #endif
                await delivery.acknowledge(outputs)
                let endedFailure = sendStopped ? nil : readFailure
                #if DEBUG
                failureSite = .mediaReportEnd
                #endif
                do { try await delivery.appendInput(to: &outputs) { try await permit.report(zones: [.media], kind: .read, started: false, error: endedFailure) } }
                catch {
                    #if DEBUG
                    DebugCloudDiagnostics.sendFailureCapture?.record(failureSite, error: error)
                    #endif
                    if context.observeEndReadReportFailure(error, primary: readFailure) {
                        #if DEBUG
                        DebugCloudDiagnostics.record(.policyExecute, action: issued.action, error: error)
                        #endif
                    }
                    if readFailure == nil { readFailure = error }
                }
                if let readFailure { throw readFailure }
                await delivery.acknowledge(outputs)
                requiredRefreshStarted = true
                #if DEBUG
                failureSite = .mediaRequiredRefresh
                #endif
                try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) }
                requiredRefreshStarted = false
                guard let queuedResume = heldMediaResume.removeValue(forKey: instance) else { break }
                selected = queuedResume
            } while true
            return .init(outputs: outputs, error: nil, deferred: false, acknowledgedOutputCount: delivery.count)
        } catch {
            var mediaCancellation = context.observe(error)
            #if DEBUG
            DebugCloudDiagnostics.sendFailureCapture?.record(failureSite, error: error)
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
            if requiredRefreshStarted {
                return .init(outputs: outputs, error: error, deferred: false, acknowledgedOutputCount: delivery.count)
            }
            let originalError = error
            do { try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) } }
            catch {
                return .init(outputs: outputs, error: error, deferred: false,
                    acknowledgedOutputCount: delivery.count, priorError: originalError)
            }
            return .init(outputs: outputs, error: error, deferred: false, mediaCancellation: mediaCancellation, acknowledgedOutputCount: delivery.count)
        }
    }

    private func waitForReadiness(_ command: CloudPolicyCommand, reservation: CloudExchangeReservation) async throws -> CloudPolicyEarlyEnd? {
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
                reservation.store(try await adapter.reservePolicyExchange())
                var events = changes.makeAsyncIterator()
                while true {
                    try Task.checkCancellation()
                    do {
                        if await adapter.policySDKReady(), let execution = try await policy.prepareExecution(command),
                           try await adapter.prepareSyncNowEpoch(execution: execution) {
                            await adapter.retainPreparationOutput(try await policy.input(.refresh(.cloud)))
                        }
                    } catch {
                        // An earlier typed failure outranks a later guard change.
                        if let preparation = error as? CloudEpochPreparationError,
                           case .failed = preparation.outcome { throw preparation }
                        // Issuance and final validation can refuse before a read
                        // has an outcome. Qualify that refusal with this request's
                        // original guard, never a replacement command or SDK.
                        if let readiness = try? await policy.exchangeReadiness(command, nativeReady: false),
                           case .end(let outcome) = readiness {
                            throw CloudEpochPreparationError(outcome: CloudPolicyEarlyEnd(outcome), original: error)
                        }
                        throw error
                    }
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
            let first = try await group.next()!
            group.cancelAll()
            // The timer cannot discard an earlier native failure whose
            // original reply was still being joined at expiry.
            while let result = await group.nextResult() {
                if case .failure(let error) = result, let preparation = error as? CloudEpochPreparationError {
                    switch preparation.outcome {
                    case .failed: throw preparation
                    case .waiting where preparation.original != nil: throw preparation
                    default: break
                    }
                }
            }
            return first
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
        let reservation = CloudExchangeReservation()
        do {
            var captured = false
            var failure: (any Error)?
            var priorError: (any Error)?
            do {
                // Reserve the same adapter owner before preparation. The read
                // runs inside the existing readiness deadline and joins on cancel.
                let readiness = try await waitForReadiness(command, reservation: reservation)
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                await delivery.acknowledge(outputs)
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
                    #if DEBUG
                    site = .exchangePreparation
                    #endif
                    guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                    let preparedExchange = try await adapter.preparePolicyExchange(command: command, execution: execution)
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
                let outcome: CloudPolicyEarlyEnd
                if let preparation = failure as? CloudEpochPreparationError { outcome = preparation.outcome }
                else { outcome = failure is CancellationError ? .cancelled : .failed(.other) }
                try await delivery.appendInput(to: &outputs) { [policy] in try await policy.endBeforeCapture(id, outcome: outcome) }
            }
            if let id = reservation.take() { await adapter.releasePolicyExchange(id) }
            await delivery.acknowledge(outputs)
            if let held = heldConfigure {
                heldConfigure = nil
                do {
                    let adapter = self.adapter
                    let policy = self.policy
                    #if DEBUG
                    site = .heldConfiguration
                    #endif
                    try await revokeFlushPark(joining: id)
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
                    priorError = failure
                    failure = error
                }
            }
            await delivery.acknowledge(outputs)
            let proof = failure as? CloudFetchProofError
            return .init(outputs: outputs, error: proof?.error ?? failure, deferred: false, acknowledgedOutputCount: delivery.count,
                fetchProofOutcome: proof?.outcome, nativeCancellation: proof?.outcome.nativeCancellation, priorError: priorError)
        } catch {
            #if DEBUG
            diagnostic?.record(site, error: error)
            DebugCloudDiagnostics.record(.policyExchange, action: command.action, error: error)
            #endif
            if let id = reservation.take() { await adapter.releasePolicyExchange(id) }
            await delivery.acknowledge(outputs)
            let originalError = error
            do { try await delivery.appendInput(to: &outputs) { [policy] in try await policy.input(.refresh(.cloud)) } }
            catch {
                await delivery.acknowledge(outputs)
                return .init(outputs: outputs, error: error, deferred: false,
                    acknowledgedOutputCount: delivery.count, priorError: originalError)
            }
            await delivery.acknowledge(outputs)
            // Qualify only the failure actually returned here. A different
            // required completion failure supersedes the operation's outcome.
            let proof = error as? CloudFetchProofError
            return .init(outputs: outputs, error: proof?.error ?? error, deferred: false, acknowledgedOutputCount: delivery.count,
                fetchProofOutcome: proof?.outcome, nativeCancellation: proof?.outcome.nativeCancellation)
        }
    }
}
