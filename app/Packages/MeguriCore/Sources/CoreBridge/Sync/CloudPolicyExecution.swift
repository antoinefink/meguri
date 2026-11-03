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
        return try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginAssetReadPass) {
            CoreCloudAssetReadPass(try raw.beginAssetReadPass())
        }
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
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionRegisterSdk) { [raw] in
            let r = try raw.registerSdk()
            return CloudRegistration(instanceID: r.instanceId, serialization: r.serialization,
                                     collectionZone: r.collectionZone, needsRootFetch: r.needsRootFetch)
        }
    }
    func retireOriginal() async throws {
        let raw = self.raw
        try await CoreCloudPolicyService.call(.cloudPolicyExecutionRetireOriginal) { [raw] in try raw.retireOriginal() }
    }
    func report(zones: [CloudPolicyZone], kind: CloudPolicyOperationKind, started: Bool, error: (any Error)? = nil, saved: UInt64 = 0) async throws -> CloudPolicyOutput {
        let raw = self.raw
        let facts = error as? CloudKitSDKOperationFailure
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionOperationActivity) {
            CloudPolicyOutput(try raw.operationActivity(facts: .init(zones: zones.map(\.raw), kind: kind.raw,
                started: started, failed: error != nil, code: facts?.code,
                retryAfterMs: facts?.retryAfterMilliseconds, saved: saved, appCancelled: error is CancellationError)))
        }
    }
    /// Invocation activity is reported in the validated task, never at task
    /// creation. This records an attempted SDK call, not network acceptance.
    func reportInvocation(zones: [CloudPolicyZone], kind: CloudPolicyOperationKind) throws -> CloudPolicyOutput {
        let raw = self.raw
        return try cloudFinalCoreCall(.cloudPolicyExecutionOperationActivity) {
            CloudPolicyOutput(try raw.operationActivity(facts: .init(zones: zones.map(\.raw), kind: kind.raw,
                started: true, failed: false, code: nil, retryAfterMs: nil, saved: 0, appCancelled: false)))
        }
    }
    func continueMediaAfterSend(_ error: any Error) async throws -> Bool {
        let raw = self.raw
        let code = (error as? CloudKitSDKOperationFailure)?.code
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionContinueMediaAfterSend) { try raw.continueMediaAfterSend(code: code) }
    }
    func beginMedia() async throws -> Bool {
        let raw = self.raw
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionBeginMedia) { try raw.beginMedia() }
    }
    func beginFetch(zones: [String]) async throws -> UInt64 {
        let raw = self.raw
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionBeginFetch) { [raw] in try raw.beginFetch(zones: zones) }
    }
    func validateControl() async throws {
        let raw = self.raw
        try await CoreCloudPolicyService.call(.cloudPolicyExecutionValidateControl) { try raw.validateControl() }
    }
    func prepareFlush() async throws -> CloudPolicyOutput {
        let raw = self.raw
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionPrepareFlush) { CloudPolicyOutput(try raw.prepareFlush()) }
    }
    func beginSend() async throws -> UInt64? {
        let raw = self.raw
        return try await CoreCloudPolicyService.call(.cloudPolicyExecutionBeginSend) { [raw] in try raw.beginSend() }
    }
}

/// The task publishes synchronous Core activity without entering the adapter
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

/// A coordinator owns this executor alongside its ONE adapter. Outputs include
/// required failure follow-ups. Deferred configuration is not completed work.
public struct CloudPolicyNativeResult: Sendable {
    public let outputs: [CloudPolicyOutput]
    public let error: (any Error)?
    public let deferred: Bool
    /// The original cleanup failure is diagnostic evidence, outside app failure cycles.
    public let zoneDeletionError: (any Error)?
    public let cleanupInventoryError: (any Error)?
    init(outputs: [CloudPolicyOutput], error: (any Error)?, deferred: Bool, zoneDeletionError: (any Error)? = nil, cleanupInventoryError: (any Error)? = nil) {
        self.outputs = outputs; self.error = error; self.deferred = deferred; self.zoneDeletionError = zoneDeletionError; self.cleanupInventoryError = cleanupInventoryError
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
    private let flushDeadline: @Sendable (Int64) async throws -> Void
    private let readinessClock: any CloudPolicyReadinessClock
    private var exchange: UInt64?
    private var nativeExchangeTask: Task<CloudPolicyNativeResult, Never>?
    private var flushTask: (id: UUID, task: Task<CloudPolicyNativeResult, Never>, execution: CoreCloudPolicyExecution)?
    private var heldConfigure: (CloudPolicyCommand, CoreCloudPolicyExecution)?
    private let mediaService: CoreCloudMediaService?
    private let mediaTransport: (any CloudMediaTransport)?
    private var mediaExecutors: [String: CloudMediaExecutor] = [:]
    private var mediaRuns: [String: Task<CloudPolicyNativeResult, Never>] = [:]
    private var mediaRunIDs: [String: UUID] = [:]
    private var mediaDrains: [String: Task<Void, Never>] = [:]
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
                flushDeadline: @escaping @Sendable (Int64) async throws -> Void = { until in
                    try await Task.sleep(for: .seconds(max(0, Double(until) / 1_000 - Date().timeIntervalSince1970)))
                }) {
        self.policy = policy; self.adapter = adapter; self.exchangeGate = exchangeGate; self.readinessClock = readinessClock
        self.mediaService = mediaService; self.mediaTransport = mediaTransport
        self.cleanupWorker = ownTransport.map { CloudCleanupExecutor(transport: $0) }
        self.zoneDeletionWorker = ownTransport.map { CloudZoneDeletionExecutor(transport: $0) }
        self.flushDeadline = flushDeadline
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
        var outputs: [CloudPolicyOutput] = []
        if !mediaHookInstalled {
            mediaHookInstalled = true
            await adapter.setOriginalMediaDrain { [weak self] sdk in
                await self?.drainCleanups(sdk: sdk)
                await self?.drainZoneDeletions(sdk: sdk)
                await self?.drainOriginalMedia(sdk)
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
            case .updatePending, .applyReady, .recordExchange, .recordQuietPoint, .probeTracking, .statusChanged:
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
                outputs.append(try await policy.input(.refresh(.cloud)))
            case .requestFetch(let zones), .requestSend(let zones):
                if zones.contains(.media), mediaService == nil || mediaTransport == nil { throw BridgeError.cloudPolicy(.coreCapabilityUnavailable) }
                guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                do { try await adapter.executePolicyOperation(command, execution: execution) }
                catch {
                    outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                    throw error
                }
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                outputs.append(try await policy.input(.refresh(.cloud)))
            case .runCleanup:
                guard cleanupRun == nil else { throw CloudOwnTransportFailure.busy }
                let task = Task { await self.performCleanup(command) }
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
                guard let permit = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                let instance = try await permit.originalSDK()
                if case .pauseMedia = command.action {
                    if let instance { await drainOriginalMedia(instance) }
                    _ = try await permit.beginMedia()
                    try await adapter.waitForDeferredUpdates()
                    outputs.append(try await policy.input(.refresh(.cloud)))
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
                        worker = CloudMediaExecutor(core: mediaService, sdk: instance, transport: mediaTransport)
                        mediaExecutors[instance] = worker
                    }
                    let runID = UUID()
                    let running = Task { await self.performMediaRound(command, permit: permit, instance: instance, runID: runID, worker: worker, coreMedia: mediaService) }
                    mediaRuns[instance] = running; mediaRunIDs[instance] = runID
                    return await withTaskCancellationHandler { await running.value } onCancel: { running.cancel() }
                }
            case .backgroundFlush(let until), .quitFlush(let until):
                guard flushTask == nil else { return .init(outputs: [], error: nil, deferred: true) }
                guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                try await execution.validateControl()
                let id = UUID()
                let task = Task { await self.performFlush(execution: execution) }
                flushTask = (id, task, execution)
                let deadline = Task<CloudPolicyOutput?, Never> {
                    do {
                        try await self.flushDeadline(until)
                        _ = await self.cancelFlush(id: id)
                        return try await self.policy.input(.backgroundExpiring)
                    } catch { return nil }
                }
                let result = await withTaskCancellationHandler { await task.value }
                    onCancel: { Task { _ = await self.cancelFlush(id: id) } }
                deadline.cancel()
                let expired = await deadline.value
                if flushTask?.id == id { flushTask = nil }
                if let expired { return .init(outputs: result.outputs + [expired], error: result.error, deferred: result.deferred) }
                return result
            case .cancelBackgroundFlush:
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
                let task = Task { await self.performExchange(command, id: id) }
                nativeExchangeTask = task
                defer { nativeExchangeTask = nil }
                return await withTaskCancellationHandler { await task.value } onCancel: { task.cancel() }
            case .cancelExchange(let id):
                let execution = try await policy.prepareExecution(command)
                if execution != nil, exchange == id { nativeExchangeTask?.cancel(); await adapter.cancelPolicyExchange() }
            }
            return .init(outputs: outputs, error: nil, deferred: false)
        } catch {
            #if DEBUG
            DebugCloudDiagnostics.record(.policyExecute, action: command.action, error: error)
            #endif
            // A failed native event transaction may have stopped tracking. Its
            // real projection supplies StopEngine and any other required work.
            if let followup = try? await policy.input(.refresh(.cloud)) { outputs.append(followup) }
            return .init(outputs: outputs, error: error, deferred: false)
        }
    }
    private func performCleanup(_ command: CloudPolicyCommand) async -> CloudPolicyNativeResult {
        do {
            try Task.checkCancellation()
            guard let worker = cleanupWorker else { throw BridgeError.cloudPolicy(.coreCapabilityUnavailable) }
            guard let permit = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
            cleanupSDK = try await permit.originalSDK()
            try Task.checkCancellation()
            // A failure here is a required-computation failure. Only a pass
            // returned by this exact qualified port permits the observation.
            let pass = try await permit.beginCleanupPass()
            var inventoryFailure: (any Error)?
            do { try await worker.run(pass) }
            catch { inventoryFailure = error }
            let projection = try await policy.input(.refresh(.cloud))
            return .init(outputs: [projection], error: nil, deferred: false, cleanupInventoryError: inventoryFailure)
        } catch {
            CloudCleanupDiagnostics.record(error)
            // A failed required computation may have stopped tracking. Return
            // its actual projection so lifecycle follow-ups can still drain.
            let followup = try? await policy.input(.refresh(.cloud))
            return .init(outputs: followup.map { [$0] } ?? [], error: error, deferred: false)
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
            let followup = try await policy.input(.refresh(.cloud))
            return .init(outputs: [followup], error: nil, deferred: false, zoneDeletionError: failure)
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
    private func cancelFlush(id: UUID) async -> (any Error)? {
        guard let original = flushTask, original.id == id else { return nil }
        original.task.cancel()
        nativeExchangeTask?.cancel()
        await adapter.cancelPolicyExchange()
        // Cancel the exact original SDK; no current-instance recapture.
        var failure: (any Error)?
        do { try await adapter.cancelPolicyFlush(execution: original.execution) }
        catch { failure = error }
        _ = await original.task.value
        return failure
    }
    private func performFlush(execution: CoreCloudPolicyExecution) async -> CloudPolicyNativeResult {
        var outputs: [CloudPolicyOutput] = []
        do {
            if let exchange = nativeExchangeTask {
                let result = await exchange.value
                if let failure = result.error { throw failure }
            } else {
                try Task.checkCancellation()
                outputs.append(try await execution.prepareFlush())
                try Task.checkCancellation()
                try await adapter.flushPolicyCollection(execution: execution)
            }
            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
            outputs.append(try await policy.input(.refresh(.cloud)))
            return .init(outputs: outputs, error: nil, deferred: false)
        } catch {
            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
            if let state = try? await policy.input(.refresh(.cloud)) { outputs.append(state) }
            return .init(outputs: outputs, error: error, deferred: false)
        }
    }
    private func drainOriginalMedia(_ sdk: String) async {
        if let draining = mediaDrains[sdk] { await draining.value; return }
        heldMediaResume.removeValue(forKey: sdk)
        mediaRuns[sdk]?.cancel()
        guard let executor = mediaExecutors.removeValue(forKey: sdk) else { return }
        let task = Task { await executor.close() }
        mediaDrains[sdk] = task
        await task.value
        mediaDrains.removeValue(forKey: sdk)
    }
    private func performMediaRound(_ issued: CloudPolicyCommand, permit: CoreCloudPolicyExecution, instance: String, runID: UUID,
                          worker: CloudMediaExecutor, coreMedia: CoreCloudMediaService) async -> CloudPolicyNativeResult {
        defer {
            if mediaRunIDs[instance] == runID { mediaRuns.removeValue(forKey: instance); mediaRunIDs.removeValue(forKey: instance) }
        }
        var outputs: [CloudPolicyOutput] = []
        var selected = (issued, permit)
        do {
            repeat {
                let (issued, permit) = selected
                try Task.checkCancellation()
                guard try await permit.beginMedia() else { break }
                let onlyEnvelopes: Bool
                if case .readBlobs = issued.action { onlyEnvelopes = true } else { onlyEnvelopes = false }
                await worker.beginRound()
                outputs.append(try await permit.report(zones: [.media], kind: .read, started: true))
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
                            do { try await adapter.sendPolicyMedia(execution: permit) }
                            catch {
                                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                                if try await permit.continueMediaAfterSend(error) == false { sendStopped = true; throw error }
                            }
                            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                        case .read(let reply):
                            if let error = reply.failure() { throw error }
                        }
                    }
                } catch { readFailure = error }
                outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
                do { outputs.append(try await permit.report(zones: [.media], kind: .read, started: false, error: sendStopped ? nil : readFailure)) }
                catch { if readFailure == nil { readFailure = error } }
                if let readFailure { throw readFailure }
                outputs.append(try await policy.input(.refresh(.cloud)))
                guard let queuedResume = heldMediaResume.removeValue(forKey: instance) else { break }
                selected = queuedResume
            } while true
            return .init(outputs: outputs, error: nil, deferred: false)
        } catch {
            if let queuedResume = heldMediaResume.removeValue(forKey: instance), let mediaTransport {
                let replacement: CloudMediaExecutor
                if let retained = mediaExecutors[instance] { replacement = retained }
                else {
                    replacement = CloudMediaExecutor(core: coreMedia, sdk: instance, transport: mediaTransport)
                    mediaExecutors[instance] = replacement
                }
                let replacementID = UUID()
                let pendingRun = Task { await self.performMediaRound(queuedResume.0, permit: queuedResume.1, instance: instance, runID: replacementID, worker: replacement, coreMedia: coreMedia) }
                mediaRuns[instance] = pendingRun; mediaRunIDs[instance] = replacementID
                let completed = await pendingRun.value
                outputs.append(contentsOf: completed.outputs)
            }
            if let refreshed = try? await policy.input(.refresh(.cloud)) { outputs.append(refreshed) }
            return .init(outputs: outputs, error: error, deferred: false)
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
    private func performExchange(_ command: CloudPolicyCommand, id: UInt64) async -> CloudPolicyNativeResult {
        var outputs: [CloudPolicyOutput] = []
        var reservation: UUID?
        do {
            var captured = false
            var failure: (any Error)?
            do {
                let readiness = try await waitForReadiness(command)
                let early: CloudPolicyEarlyEnd?
                if let readiness { early = readiness }
                else { early = try await exchangeGate?.waitBeforeCapture(command) }
                if let outcome = early {
                    outputs.append(try await policy.endBeforeCapture(id, outcome: outcome))
                } else {
                    try Task.checkCancellation()
                    guard let execution = try await policy.prepareExecution(command) else { throw BridgeError.cloudPolicy(.staleCommand) }
                    let reserved = try await adapter.reservePolicyExchange()
                    reservation = reserved
                    let preparedExchange = try await adapter.preparePolicyExchange(execution: execution)
                    try Task.checkCancellation()
                    let capture = try await policy.captureExchange(command)
                    outputs.append(capture.output)
                    if let runtimeID = capture.runtimeExchange {
                        captured = true
                        try await adapter.runPolicyExchange(runtimeID: runtimeID, execution: execution, prepared: preparedExchange)
                    }
                }
            } catch {
                #if DEBUG
                DebugCloudDiagnostics.record(.policyExchange, action: command.action, error: error)
                #endif
                failure = error
            }
            outputs.append(contentsOf: await adapter.takePolicyOperationOutputs())
            if captured {
                outputs.append(try await policy.completeExchange(id))
            } else if let failure {
                outputs.append(try await policy.endBeforeCapture(id, outcome: failure is CancellationError ? .cancelled : .failed(.other)))
            }
            if let reservation { await adapter.releasePolicyExchange(reservation) }
            reservation = nil
            if let held = heldConfigure {
                heldConfigure = nil
                do {
                    let adapter = self.adapter
                    let policy = self.policy
                    let configure = Task { try await adapter.executePolicyLifecycle(held.0, execution: held.1, policy: policy) }
                    _ = try await configure.value
                    outputs.append(try await policy.input(.refresh(.cloud)))
                } catch {
                    #if DEBUG
                    DebugCloudDiagnostics.record(.policyExchange, action: held.0.action, error: error)
                    #endif
                    if failure == nil { failure = error }
                }
            }
            return .init(outputs: outputs, error: failure, deferred: false)
        } catch {
            #if DEBUG
            DebugCloudDiagnostics.record(.policyExchange, action: command.action, error: error)
            #endif
            if let reservation { await adapter.releasePolicyExchange(reservation) }
            if let followup = try? await policy.input(.refresh(.cloud)) { outputs.append(followup) }
            return .init(outputs: outputs, error: error, deferred: false)
        }
    }
}
