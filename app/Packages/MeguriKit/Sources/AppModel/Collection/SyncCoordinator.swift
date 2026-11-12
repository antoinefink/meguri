import CoreBridge
import Foundation
import Network
import Observation
#if canImport(CloudKit)
import CloudKit
#endif

public struct CloudBuildConfiguration: Sendable {
    public let container: String
    public let environment: CloudPlatformEnvironment
    public let schemaConfigured: Bool
    public init(container: String, environment: CloudPlatformEnvironment, schemaConfigured: Bool) {
        self.container = container; self.environment = environment; self.schemaConfigured = schemaConfigured
    }
    public static var fromBundle: Self? {
        let bundle = Bundle.main
        guard let container = bundle.object(forInfoDictionaryKey: "MeguriCloudContainer") as? String,
              !container.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              let name = bundle.object(forInfoDictionaryKey: "MeguriCloudEnvironment") as? String else { return nil }
        let environment: CloudPlatformEnvironment
        switch name { case "development": environment = .development; case "production": environment = .production; default: return nil }
        return Self(container: container, environment: environment,
            schemaConfigured: bundle.object(forInfoDictionaryKey: "MeguriCloudSchemaConfigured") as? Bool == true)
    }
}

// This seam substitutes transport only in focused owner tests. Production
// constructs NativeCollectionCloudBackend and the retained Rust services below.
@MainActor
protocol SyncCoordinatorBackend: AnyObject, Sendable {
    var connection: CoreCloudConnectionService? { get }
    var own: CloudOwnExecutor? { get }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult
    // RunCleanup alone reports its qualified local-pass return before waiting
    // for native inventory. This callback grants no Core or SDK authority.
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void) async -> SyncCommandResult
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void,
                 acknowledgeOutputs: @escaping @MainActor @Sendable ([CloudPolicyOutput]) async -> Void,
                 performInput: @escaping @MainActor @Sendable (@escaping @Sendable () async throws -> CloudPolicyOutput) async throws -> CloudPolicyOutput) async -> SyncCommandResult
    func beginAccountRead(transition: Bool) async throws -> UInt64
    func readAccount(_ read: UInt64) async -> SyncCommandResult
    // Resolve only. The completion commits proof only when admitted == true;
    // cancellation still returns its original failure through result admission.
    func prepareAccountRead(_ read: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult
    func takeAccountReadRequest() async -> Bool
    func setProofAdmission(_ admission: @escaping @Sendable (CloudOriginalCommandProofYield) async throws -> Void,
                           acknowledgeOutputs: @escaping @Sendable ([CloudPolicyOutput]) async -> Void) async
    func freezeForSuspend() async throws
    func resumeAfterCoreResume() async
    func quiesce() async throws
    func close() async throws
}
extension SyncCoordinatorBackend {
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void) async -> SyncCommandResult {
        await execute(command)
    }
    func execute(_ command: CloudPolicyCommand, corePrepared: @escaping @MainActor @Sendable () async -> Void,
                 acknowledgeOutputs: @escaping @MainActor @Sendable ([CloudPolicyOutput]) async -> Void,
                 performInput: @escaping @MainActor @Sendable (@escaping @Sendable () async throws -> CloudPolicyOutput) async throws -> CloudPolicyOutput) async -> SyncCommandResult {
        await CloudPolicyOutputAcknowledgement.$current.withValue(acknowledgeOutputs) {
            await CloudPolicyOutputAcknowledgement.$input.withValue(performInput) {
                await execute(command, corePrepared: corePrepared)
            }
        }
    }
    // Transport-only test results already completed; retain their failures
    // even when admission closes. Native backends defer the actual proof.
    func prepareAccountRead(_ read: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult {
        let result = await readAccount(read)
        return { _ in result }
    }
    func takeAccountReadRequest() async -> Bool { false }
    func setProofAdmission(_ admission: @escaping @Sendable (CloudOriginalCommandProofYield) async throws -> Void,
                           acknowledgeOutputs: @escaping @Sendable ([CloudPolicyOutput]) async -> Void) async {}
    func freezeForSuspend() async throws {}
    func resumeAfterCoreResume() async {}
}
nonisolated struct SyncCommandResult: Sendable {
    #if DEBUG
    var causalCommandOrdinal: UInt64? = nil
    var causalReadOrdinal: UInt64? = nil
    var diagnosticFailure: DebugCloudDiagnostics.ProjectedFailure? = nil
    #endif
    let outputs: [CloudPolicyOutput]
    let error: (any Error)?
    let originalGuardRefusal: CloudPolicyOriginalGuardRefusal?
    let accountReadRefusal: SyncAccountReadRefusal?
    let deferred: Bool
    let accountVerified: Bool
    let mediaCancellation: Bool
    let acknowledgedOutputCount: Int
    // Account preparation returns before any subsequent Core preparation/input.
    let afterOutputs: (@MainActor @Sendable (Bool) async -> SyncCommandResult)?
    init(outputs: [CloudPolicyOutput], error: (any Error)?, deferred: Bool, accountVerified: Bool = false, mediaCancellation: Bool = false, acknowledgedOutputCount: Int = 0,
         afterOutputs: (@MainActor @Sendable (Bool) async -> SyncCommandResult)? = nil, originalGuardRefusal: CloudPolicyOriginalGuardRefusal? = nil,
         accountReadRefusal: SyncAccountReadRefusal? = nil) {
        self.outputs = outputs; self.error = error; self.deferred = deferred; self.accountVerified = accountVerified
        self.mediaCancellation = mediaCancellation; self.originalGuardRefusal = originalGuardRefusal
        self.accountReadRefusal = accountReadRefusal
        self.acknowledgedOutputCount = acknowledgedOutputCount; self.afterOutputs = afterOutputs
    }
}
enum SyncOwnerFailure: Error { case configurationUnavailable }

// Inherited by native child work, but marked only when the coordinator cancels
// its retained original command task for shutdown. Account reads use a separate
// explicit control, without task-local ownership.
@MainActor final class SyncCommandTaskContext {
    @TaskLocal nonisolated static var current: SyncCommandTaskContext?
    let generation: UInt64
    let revision: UInt64
    var shutdownCancellationRequested = false
    var shutdownCancelled: Bool { shutdownCancellationRequested && Task.isCancelled }
    init(generation: UInt64, revision: UInt64) { self.generation = generation; self.revision = revision }
}

// This control is retained by the original account task and its queued
// completion. Record the first cancellation before sending it to that task.
@MainActor private final class SyncAccountTaskControl {
    private enum Cancellation { case shutdown, other }
    private var firstCancellation: Cancellation?
    func recordCancellation(shutdown: Bool, alreadyCancelled: Bool) {
        guard firstCancellation == nil else { return }
        firstCancellation = shutdown && !alreadyCancelled ? .shutdown : .other
    }
    var shutdownOwned: Bool { firstCancellation == .shutdown }
}

/// Presentation lifetime only. The coordinator retains the original publication
/// task after durable acceptance, including when this presentation closes.
@Observable @MainActor
public final class CloudPublicationConfirmation {
    public enum Result: Equatable { case accepted, refused(CloudConnectionFailure?) }
    public private(set) var confirming = false
    public private(set) var result: Result?
    /// Safe copy for this attempt's post-acceptance failure; never inherited history.
    public private(set) var backgroundErrorKey: String?
    private var attempt: UUID?
    private var dismissed = false
    public init() {}
    public var accepted: Bool { result == .accepted }
    public var refused: Bool { if case .refused = result { true } else { false } }
    public var failure: CloudConnectionFailure? { if case .refused(let failure) = result { failure } else { nil } }
    public var submitted: Bool { confirming || accepted }
    public func clearRefusal() { if refused { result = nil } }
    public func confirm(using sync: SyncCoordinator) async {
        guard !dismissed, !confirming, !accepted else { return }
        let token = UUID(), generation = sync.generation
        attempt = token; confirming = true; result = nil; backgroundErrorKey = nil
        await sync.confirmPublication(onConfirmation: { outcome in
            guard self.attempt == token, sync.generation == generation else { return }
            self.result = outcome; self.confirming = false
        }, onBackgroundFailure: { failure in
            guard self.attempt == token, sync.generation == generation, self.accepted else { return }
            self.backgroundErrorKey = failure.map(SyncStatusPresentation.connectionErrorKey) ?? "sync.recovery.error.other"
        })
        // No completion mutation here: retained work can finish much later.
    }
    public func close() -> Bool {
        guard !confirming else { return false }
        invalidate()
        return true
    }
    public func invalidate() {
        dismissed = true; attempt = nil; confirming = false; backgroundErrorKey = nil
    }
}

@Observable @MainActor
public final class SyncCoordinator {
    public enum SetupState: Equatable { case configurationUnavailable, prepared, attached, failed }
    public private(set) var setupState: SetupState = .configurationUnavailable
    public private(set) var status: CloudPolicyStatus?
    public private(set) var settings: CloudPolicyDeviceSettings
    public private(set) var generation: UInt64 = 0
    #if DEBUG
    public var debugPolicyStatusValue: String? {
        guard status != nil else { return nil }
        return DebugCloudDiagnostics.policyStatusValue(generation: generation, hostRevision: hostRevision)
    }
    #endif
    public private(set) var operationFailed = false
    /// Retained deferred native admission is not a completed command. Only the
    /// Rust projection may describe sync completion to the UI.
    public private(set) var hasDeferredNativeWork = false
    public private(set) var syncNowUnavailable: CloudPolicyUnavailable?
    public private(set) var preview: CloudConnectionPreviewWork?
    public private(set) var connectionFailure: CloudConnectionFailure?
    public private(set) var recovery: CloudRecoveryStatus?
    /// True until the original retained preview task has returned, including cancellation/drain.
    /// Publication and recovery work deliberately do not count as preview activity.
    public var connectionPreviewActive: Bool { readingPreview }
    public var connectionPreviewCancelling: Bool { readingPreview && ownTask?.isCancelled == true }
    public var connectionOperationActive: Bool { (ownTask != nil && !readingPreview) || initialAdoptionEnabledBefore != nil }
    public var initialAdoptionActive: Bool { initialAdoptionEnabledBefore != nil }
    public var connectionActivationActive: Bool { activatingInitialAdoption }
    public var connectionService: CoreCloudConnectionService? { setupState == .attached && !replacing ? backend?.connection : nil }

    private enum Input {
        case policy(CloudPolicyEvent)
        case visibleMedia([String], generation: UInt64)
        case replayVisibleMedia
        case core(CoreEvent)
        case lifecycle(CollectionLifecycle)
        case replaced(UInt64)
        case persisted(revision: UInt64)
        case account(Bool)
        case ownAccountProof(UInt64, generation: UInt64, revision: UInt64)
        case result(SyncCommandResult, generation: UInt64, revision: UInt64, shutdownCancelled: Bool,
                    supersededAccountCancellation: Bool = false, acknowledged: CheckedContinuation<Void, Never>?)
        case completeAccount(UInt64, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                             control: SyncAccountTaskControl,
                             complete: @MainActor (Bool) async -> SyncCommandResult,
                             acknowledged: CheckedContinuation<Bool, Never>)
        case accountOutputsReturned(UInt64, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                                    result: SyncCommandResult, acknowledged: CheckedContinuation<Bool, Never>)
        case resumeBinding(CloudConnectionPreview, preparation: ResumePreparation, service: CoreCloudConnectionService, generation: UInt64, revision: UInt64,
                           acknowledged: CheckedContinuation<ResumeAdmission, any Error>)
        case nativeInput(@Sendable () async throws -> CloudPolicyOutput, generation: UInt64, revision: UInt64,
                         phaseOwner: UUID?, acknowledged: CheckedContinuation<CloudPolicyOutput, any Error>)
        case nativeInputReturned(CloudPolicyOutput, acknowledged: CheckedContinuation<CloudPolicyOutput, any Error>)
        case corePhaseFinished(UUID)
        case proofYield(CloudOriginalCommandProofYield, owner: ProofOwner?, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64)
        case proofAccountRead
        case quiescentDependency(CloudQuiescentDependencyWait, CloudQuiescentDependencyWait.Phase, owner: UUID,
                             backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                             acknowledged: CheckedContinuation<Void, any Error>)
        case corePrepared(acknowledged: CheckedContinuation<Void, Never>)
        case ownCore(UUID, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                     acknowledged: CheckedContinuation<Void, any Error>)
        case ownCoreReturned(acknowledged: CheckedContinuation<Void, Never>)
        case reconcile(any SyncCoordinatorBackend, acknowledged: CheckedContinuation<Void, Never>?)
        case barrier(CheckedContinuation<Void, Never>)
    }
    // Created with the consumed Resume preview before its task does any work.
    // The input lane revokes it before the actual Core Resume call begins.
    private final class ResumePreparation { let id = UUID() }
    private enum ResumeAdmission { case pending, cancelled, resumed(CloudOriginalCommandProofYield) }
    private enum ProofOwner: Equatable {
        case command(UUID), own(UUID)
        var commandID: UUID? { if case .command(let id) = self { id } else { nil } }
    }
    #if DEBUG
    enum CausalBoundary {
        case commandResultEnqueue, commandAcknowledgementJoined
        case proofResultReduced, proofAcknowledgementSending
        case accountCompletion(read: UInt64, followup: Bool, closing: Bool, suspended: Bool, replacing: Bool,
                               taskCancelled: Bool?, admitted: Bool, backendMatches: Bool, generationMatches: Bool,
                               revisionMatches: Bool, pendingMismatch: Bool)
        case accountAccepted(read: UInt64, verified: Bool, succeeded: Bool)
        case resultReduced(command: UInt64?, read: UInt64?, cancellation: Bool, shutdownOwned: Bool, before: UInt64, after: UInt64)
        case resumeEntered, resumeReturned, resumeRefused(authorityChanged: Bool), resumeCancelled
        case ownCoreStarted
        case dependencyRevocationJoined(UUID)
        case corePredecessorWaiting(CloudPolicyCommand), corePredecessorReleased(CloudPolicyCommand)
    }
    @ObservationIgnored var observeQuiescentDependency: (@MainActor (Bool) -> Void)?
    @ObservationIgnored var observeCausalBoundary: (@MainActor (CausalBoundary, CloudPolicyCommand?) -> Void)?
    #endif
    enum ResumePolicyDrainStage { case waiting, finished }
    // Scheduling observation only; never supplies a result or a capability.
    @ObservationIgnored var observeAccountPolicyDrain: (@MainActor (ResumePolicyDrainStage) -> Void)?
    @ObservationIgnored var observeResumePolicyDrain: (@MainActor (ResumePolicyDrainStage) -> Void)?
    enum PlatformSignalStage: Equatable { case started, drained }
    @ObservationIgnored var observePlatformSignalLifetime: (@MainActor (PlatformSignalStage) -> Void)?
    // Scheduling observation only; never substitutes Core confirmation authority.
    @ObservationIgnored var observePublicationConfirmation: (@MainActor () async -> Void)?
    private struct Issued { let command: CloudPolicyCommand; let generation: UInt64; let sequence: UInt64 }
    @MainActor private final class CorePhase {
        let command: CloudPolicyCommand?
        init(command: CloudPolicyCommand? = nil) { self.command = command }
        private var finished = false
        private var waiters: [CheckedContinuation<Void, Never>] = []
        func wait(waiting: () -> Void) async {
            if !finished {
                waiting()
                await withCheckedContinuation { waiters.append($0) }
            }
        }
        func finish() {
            finished = true
            let originals = waiters; waiters.removeAll()
            for original in originals { original.resume() }
        }
    }
    @ObservationIgnored private let bootstrap: @MainActor () async throws -> Void
    @ObservationIgnored private let makeBackend: @MainActor (@escaping @Sendable () async -> Void) async throws -> any SyncCoordinatorBackend
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let clock: AppClock
    @ObservationIgnored private let directory: URL?
    @ObservationIgnored private let observeSignals: Bool
    private var platformPrepared = false
    // Ready can arrive inside open, before FFI retains the Host. Only the
    // retained-open completion admits the first backend for this Host lifetime.
    private var retainedHostAdmitted = false
    private var platformTask: Task<Void, Never>?
    private var backend: (any SyncCoordinatorBackend)?
    private var inputQueue: [Input] = []
    private var inputTask: Task<Void, Never>?
    private var operationTasks: [UUID: Task<Void, Never>] = [:]
    private var quitOperationIDs: Set<UUID> = []
    private var operationContexts: [UUID: SyncCommandTaskContext] = [:]
    // Core commands retain their original task/result ACKs. Only their phase,
    // never a network task, holds the next policy input or local own step.
    private var corePhase: [UUID: CorePhase] = [:]
    private var lastCorePhase: CorePhase?
    private var pendingActionLists: [(issued: [Issued], backend: any SyncCoordinatorBackend, revision: UInt64)] = []
    private var backendRetired = false
    private var replacementTask: Task<Void, Never>?
    private var replacing = false
    private var suspended = false
    private var awaitingResumeProof = false
    private var pendingPreviewCancellation = false
    private var pendingExpiry = false
    private var pendingLocalRefresh = false
    private var desiredVisibleMedia: (generation: UInt64, names: [String])?
    private var deliveredVisibleMedia: (revision: UInt64, names: [String])?
    private var mediaEnabledRevision: UInt64?
    private var reachable: Bool?
    private var quitCompletion: (@MainActor () -> Void)?
    private var accountTask: Task<Void, Never>?
    private var accountTaskControl: SyncAccountTaskControl?
    private var pendingAccountRead: UInt64?
    private struct ProofWait {
        let proof: CloudOriginalCommandProofYield
        let owner: ProofOwner?
        var read: UInt64?
    }
    private var proofWaits: [UUID: ProofWait] = [:]
    private var quiescentDependencies: [UUID: UUID] = [:]
    private var ownOwner: UUID?
    private var ownResumePreparation: (owner: UUID, token: ResumePreparation)?
    private var heldAccountCompletion: Input?
    private var readyPreviewOwner: (service: CoreCloudConnectionService, generation: UInt64, revision: UInt64, id: UUID)?
    private var readingPreview = false
    // A retained acceptance, not preview/view ownership. Nil is distinct from
    // an earlier Enabled=false and survives queued work until Rust ends it.
    private var initialAdoptionEnabledBefore: Bool?
    @ObservationIgnored var observeInitialAdoptionPreview: (@MainActor (CloudConnectionPreview) async -> Void)?
    @ObservationIgnored var observeInitialAdoptionAcceptance: (@MainActor () -> Void)?
    private var activatingInitialAdoption = false
    private var initialActivationReturned = false
    private var initialAdoptionCancelled = false
    private let initialOwnAdmission = CloudOwnWorkAdmission()
    private var initialOwnDrain: Task<Void, Never>?
    private var initialOwnResumeRead: UInt64?
    private var initialOwnResume: Task<Void, Never>?
    private var awaitingInitialOwnProof = false
    // Discovery is presentation only. This attachment's one startup request
    // waits for successful proof on the ordered account-completion lane.
    private var retainedRecoveryStart: (backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64)?
    private var ownWorkRequested = false
    private var ownTask: Task<Void, Never>?
    private var flushTimer: Timer?
    private var networkTask: Task<Void, Never>?
    private var accountEventsTask: Task<Void, Never>?
    private var networkMonitor: NWPathMonitor?
    private var accountObserver: (any NSObjectProtocol)?
    private var closing = false
    private var drained = false
    private var closeTask: Task<Bool, Never>?
    private var attachmentTask: Task<Void, Never>?
    private var hostRevision: UInt64 = 0
    private(set) var failureRevision: UInt64 = 0
    private var commandSequence: UInt64 = 0
    private var timerSequence: UInt64 = 0
    private var statusSequence: UInt64 = 0
    private static let settingsKey = "meguri.cloud.deviceSettings"

    public convenience init(directory: URL, clock: AppClock = .system,
                            configuration: CloudBuildConfiguration? = .fromBundle, defaults: UserDefaults? = .standard) {
        self.init(bootstrap: {
            guard let configuration, configuration.schemaConfigured,
                  !configuration.container.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                throw SyncOwnerFailure.configurationUnavailable
            }
            // No potentially network-bound account lookup precedes local open.
            // CouldNotDetermine maps to Rust's ReadFailed, never signed-out.
            try await CoreCloudWorkflowService.configurePlatform(container: configuration.container,
                environment: configuration.environment, status: .couldNotDetermine, identity: nil)
        }, makeBackend: { persisted in
            guard let configuration else { throw SyncOwnerFailure.configurationUnavailable }
            #if canImport(CloudKit)
            return try await NativeCollectionCloudBackend.make(configuration: configuration,
                directory: directory, persisted: persisted)
            #else
            throw SyncOwnerFailure.configurationUnavailable
            #endif
        }, defaults: defaults, clock: clock, directory: directory, observeSignals: true)
    }

    init(bootstrap: @escaping @MainActor () async throws -> Void,
         makeBackend: @escaping @MainActor (@escaping @Sendable () async -> Void) async throws -> any SyncCoordinatorBackend,
         defaults: UserDefaults?, clock: AppClock = .system, directory: URL? = nil, observeSignals: Bool = false) {
        self.bootstrap = bootstrap; self.makeBackend = makeBackend; self.defaults = defaults
        self.clock = clock; self.directory = directory
        self.observeSignals = observeSignals
        let saved = defaults?.dictionary(forKey: Self.settingsKey)
        settings = .init(enabled: saved?["enabled"] as? Bool ?? false, paused: saved?["paused"] as? Bool ?? false,
            automatic: saved?["automatic"] as? Bool ?? true, media: saved?["media"] as? Bool ?? true)
    }

    /// Both open and prepareEntry call this before either can create a Host.
    /// Missing setup is local-only availability, not a local opening failure.
    func preparePlatform() async {
        if let platformTask { await platformTask.value; return }
        if drained { closing = false; drained = false }
        guard !platformPrepared else { return }
        let task = Task {
            defer { platformTask = nil }
            do { try await bootstrap(); platformPrepared = true; setupState = .prepared }
            catch SyncOwnerFailure.configurationUnavailable { setupState = .configurationUnavailable }
            catch {
                #if DEBUG
                DebugCloudDiagnostics.record(.platformBootstrap, error: error)
                #endif
                setupState = .failed; operationFailed = true
            }
        }
        platformTask = task
        await task.value
    }

    /// Only ReviewCollection.completeOpenedHost calls this, after FFI retention.
    func completeOpenedHost(generation: UInt64) async {
        while let original = replacementTask { await original.value }
        guard !closing, !Task.isCancelled else { return }
        if let original = attachmentTask {
            if generation > self.generation { enqueue(.replaced(generation)) }
            await original.value
            return
        }
        guard platformPrepared else {
            self.generation = max(self.generation, generation)
            return
        }
        guard generation >= self.generation else { return }
        retainedHostAdmitted = true
        if backend != nil || replacing { enqueue(.replaced(generation)); return }
        hostRevision &+= 1
        let revision = hostRevision
        self.generation = generation
        if desiredVisibleMedia?.generation != generation { desiredVisibleMedia = nil }
        let task = Task { [self] in
            defer { attachmentTask = nil }
            do {
                let value = try await self.makeBackend { [weak self] in
                    await self?.nativeEventPersisted(revision: revision)
                }
                // Even an obsolete factory result owns native resources. Keep
                // it retained on close failure; the next lane must drain it.
                backend = value
                backendRetired = false
                await installProofAdmission(value, generation: generation, revision: revision)
                guard !closing, !Task.isCancelled, !replacing,
                      revision == hostRevision, generation == self.generation else {
                    try await value.close()
                    backend = nil
                    return
                }
                drained = false
                setupState = .attached
                let retained = await readRetainedRecovery(value)
                guard !closing, !suspended, !replacing, !Task.isCancelled,
                      backend === value, !backendRetired, !drained,
                      revision == hostRevision, generation == self.generation else { return }
                attachRetainedRecovery(retained, backend: value, generation: generation, revision: revision)
                enqueue(.policy(.settings(settings)))
                enqueue(.replayVisibleMedia)
                enqueue(.policy(.refresh(.cloud)))
                enqueue(.account(false))
                if observeSignals { observePlatformSignals() }
            } catch {
                #if DEBUG
                DebugCloudDiagnostics.record(.backendAttachment, error: error)
                #endif
                operationFailed = true; setupState = .failed
            }
        }
        attachmentTask = task
        await task.value
    }

    private func readRetainedRecovery(_ original: any SyncCoordinatorBackend) async -> Result<CloudRecoveryStatus?, any Error> {
        do { return .success(try await original.connection?.recoveryStatus()) }
        catch { return .failure(error) }
    }
    // Called only after the original attachment has been revalidated following
    // discovery. Keep Core's actual status/handle; infer no operation or permission.
    private func attachRetainedRecovery(_ result: Result<CloudRecoveryStatus?, any Error>,
                                       backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64) {
        retainedRecoveryStart = nil
        switch result {
        case .success(let status):
            #if DEBUG
            backend.connection?.debugResetAdoptionProgress()
            #endif
            recovery = status
            if status != nil { retainedRecoveryStart = (backend, generation, revision) }
        case .failure:
            #if DEBUG
            if case .failure(let error) = result {
                DebugCloudDiagnostics.record(.retainedRecoveryFailure, error: error)
            }
            #endif
            recovery = nil
            operationFailed = true
        }
    }
    private func startRetainedRecoveryAfterProof(_ original: any SyncCoordinatorBackend, generation expected: UInt64, revision: UInt64) {
        guard let pending = retainedRecoveryStart, pending.backend === original,
              pending.generation == expected, pending.revision == revision,
              backend === original, expected == generation, revision == hostRevision,
              !closing, !suspended, !replacing, !drained, !backendRetired, setupState == .attached,
              settings.enabled, !settings.paused, accountTask?.isCancelled == false,
              !awaitingResumeProof, !awaitingInitialOwnProof, initialOwnDrain == nil,
              !initialActivationReturned, initialAdoptionEnabledBefore == nil, ownTask == nil,
              original.connection != nil else { return }
        resumeConnection()
        if ownTask != nil { retainedRecoveryStart = nil }
    }

    private func nativeEventPersisted(revision: UInt64) {
        guard revision == hostRevision, !closing, !replacing, !drained else { return }
        // SDK callbacks only enqueue. send/fetch never runs in callback delivery.
        enqueue(.persisted(revision: revision))
        if recovery != nil, retainedRecoveryStart == nil {
            if ownTask == nil { resumeConnection() }
            else { ownWorkRequested = true }
        }
    }
    func receiveLifecycle(_ lifecycle: CollectionLifecycle) { if !closing { enqueue(.lifecycle(lifecycle)) } }
    func collectionReplaced(_ generation: UInt64) { if !closing { enqueue(.replaced(generation)) } }
    func receive(_ event: CloudPolicyEvent) { if !closing { enqueue(.policy(event)) } }
    func coreEvent(_ event: CoreEvent) {
        guard !closing else { return }
        switch event.kind {
        case .sessionStarted, .sessionEnded, .changes, .available, .mediaArrived:
            // The Driver reads the actual session registry; focus is never a
            // SessionEnded input and cannot release the newest held answer.
            enqueue(.core(event))
        default: break
        }
    }

    public func setDeviceSettings(paused: Bool, automatic: Bool, media: Bool) {
        let wasPaused = settings.paused
        if paused { initialOwnAdmission.setOpen(false) }
        saveSettings(.init(enabled: settings.enabled, paused: paused, automatic: automatic, media: media))
        if wasPaused, !paused, settings.enabled, let pending = retainedRecoveryStart,
           pending.backend === backend, pending.generation == generation, pending.revision == hostRevision,
           !closing, !suspended, !replacing, !drained, !backendRetired, setupState == .attached,
           initialAdoptionEnabledBefore == nil {
            // Revisit this attachment through a new ordered read. The proof
            // that returned while paused grants no authority to this attempt.
            enqueue(.account(false))
        }
        guard initialAdoptionEnabledBefore != nil, wasPaused != paused else { return }
        if paused { drainInitialOwnWork() }
        else {
            awaitingInitialOwnProof = true
            initialOwnResume = Task {
                defer { initialOwnResume = nil }
                await initialOwnDrain?.value
                guard !closing, !suspended, !replacing, !settings.paused else { return }
                enqueue(.account(false))
                await prepareConnectionAuthority()
            }
        }
    }
    private func drainInitialOwnWork() {
        initialOwnAdmission.setOpen(false)
        initialOwnResumeRead = nil
        guard initialOwnDrain == nil, let own = backend?.own else { return }
        let original = ownTask
        original?.cancel()
        initialOwnDrain = Task {
            await own.interruptAndDrain()
            await original?.value
            initialOwnDrain = nil
        }
    }
    private func saveSettings(_ settings: CloudPolicyDeviceSettings) {
        self.settings = settings
        defaults?.set(["enabled": settings.enabled, "paused": settings.paused,
            "automatic": settings.automatic, "media": settings.media], forKey: Self.settingsKey)
        receive(.settings(settings))
    }
    public func syncNow(source: CloudPolicySource = .command) {
        reportAvailableSpace(); receive(.syncNowRequested(source))
    }
    public func cancelSyncNow(exchange: UInt64) { receive(.syncNowCancelled(exchange: exchange)) }
    public func visibleMedia(_ names: [String]) { visibleMedia(names, generation: generation) }
    func visibleMedia(_ names: [String], generation: UInt64) {
        if !closing { enqueue(.visibleMedia(names, generation: generation)) }
    }
    /// Discovery is read-only. Only Rust's explicit no-file directive transfers
    /// the ready authority to a separate, app-owned adoption operation.
    public func beginConnectionPreview() {
        guard !closing, !suspended, !replacing, !settings.paused, setupState == .attached, initialAdoptionEnabledBefore == nil, ownTask == nil, preview == nil, let backend, let own = backend.own else { return }
        let expected = generation, revision = hostRevision
        connectionFailure = nil
        readingPreview = true
        startOwnTask(backend: backend, generation: expected, revision: revision) { [self] in
            defer { readingPreview = false; finishOwnWork() }
            do {
                let work = try await own.preview()
                guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused,
                      expected == generation, revision == hostRevision else {
                    if case .ready(let handle, _) = work { try await backend.connection?.cancelPreview(handle) }
                    return
                }
                if case .ready(let handle, let summary) = work, summary.initialAdoptionRequired {
                    guard !settings.paused, let connection = backend.connection else {
                        try await backend.connection?.cancelPreview(handle)
                        return
                    }
                    await observeInitialAdoptionPreview?(handle)
                    guard !closing, !suspended, !replacing, !settings.paused, !Task.isCancelled,
                          expected == generation, revision == hostRevision else {
                        try await connection.cancelPreview(handle)
                        return
                    }
                    readingPreview = false
                    initialAdoptionCancelled = false
                    initialActivationReturned = false
                    initialAdoptionEnabledBefore = settings.enabled
                    initialOwnAdmission.setOpen(true)
                    // No await between the persistent setting and actual adopt.
                    saveEnabled(true)
                    observeInitialAdoptionAcceptance?()
                    do {
                        _ = try await connection.confirmAdoption(handle)
                        if expected == generation, revision == hostRevision, !closing { connectionFailure = nil }
                    }
                    catch {
                        // The call did not accept an operation. Restore even
                        // when authority/account changed during the call.
                        restoreInitialAdoptionEnabled()
                        throw error
                    }
                    recovery = try await connection.recoveryStatus()
                    await runOwnWork(backend, generation: expected, revision: revision)
                } else if case .ready = work, let connection = backend.connection {
                    readyPreviewOwner = (connection, expected, revision, UUID())
                    preview = work
                    receive(.refresh(.cloud))
                } else { preview = work }
            } catch {
                #if DEBUG
                DebugCloudDiagnostics.record(.connectionPreview, error: error)
                #endif
                if expected == generation, revision == hostRevision, !closing, !Task.isCancelled { recordConnectionFailure(error) }
            }
        }
    }
    public func cancelConnectionPreview() async {
        if suspended { pendingPreviewCancellation = true; return }
        if readingPreview {
            let originalTask = ownTask, original = backend?.own
            originalTask?.cancel()
            await original?.cancelPreview()
            await originalTask?.value
            return
        }
        guard case .ready(let handle, _) = preview, let owner = readyPreviewOwner else { return }
        do {
            try await owner.service.cancelPreview(handle)
            if readyPreviewOwner?.id == owner.id { preview = nil; readyPreviewOwner = nil }
        } catch {
            if readyPreviewOwner?.id == owner.id {
                preview = nil; readyPreviewOwner = nil
                recordConnectionFailure(error)
            }
        }
    }
    public func confirmPublication() async { await confirmConnection(.publication) }
    public func confirmPublication(onConfirmation: @escaping @MainActor (CloudPublicationConfirmation.Result) -> Void,
                                   onBackgroundFailure: (@MainActor (CloudConnectionFailure?) -> Void)? = nil) async {
        await confirmConnection(.publication, publicationConfirmation: onConfirmation, publicationBackgroundFailure: onBackgroundFailure)
    }
    public func confirmAdoption() async { await confirmConnection(.adoption) }
    public func confirmResume() async { await confirmConnection(.resume) }
    private func recordConnectionFailure(_ error: any Error) {
        #if DEBUG
        DebugCloudDiagnostics.record(.connectionFailure, error: error)
        #endif
        operationFailed = true
        if case BridgeError.cloudConnection(let failure) = error { connectionFailure = failure }
    }
    private func confirmConnection(_ intent: CloudConnectionConfirmation,
                                   publicationConfirmation: (@MainActor (CloudPublicationConfirmation.Result) -> Void)? = nil,
                                   publicationBackgroundFailure: (@MainActor (CloudConnectionFailure?) -> Void)? = nil) async {
        guard !closing, !suspended, !replacing, !settings.paused, !readingPreview,
              initialAdoptionEnabledBefore == nil, ownTask == nil, case .ready(let handle, let summary) = preview,
              SyncStatusPresentation.permits(intent, decision: summary.decision),
              let owner = readyPreviewOwner, owner.generation == generation, owner.revision == hostRevision,
              let backend, let connection = backend.connection, connection === owner.service else {
            publicationConfirmation?(.refused(.notOffered))
            return
        }
        let expected = generation, revision = hostRevision
        preview = nil; readyPreviewOwner = nil
        let resumePreparation: ResumePreparation? = intent == .resume ? ResumePreparation() : nil
        startOwnTask(backend: backend, generation: expected, revision: revision, resumePreparation: resumePreparation) { [self] in
            var publicationAccepted = false
            var publicationResultDelivered = false
            defer {
                if !publicationResultDelivered, expected == generation, revision == hostRevision,
                   self.backend === backend, backend.connection === connection, !closing {
                    publicationConfirmation?(.refused(.authorityChanged))
                }
                finishOwnWork()
            }
            do {
                guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused,
                      expected == generation, revision == hostRevision,
                      self.backend === backend, backend.connection === connection else {
                    try await connection.cancelPreview(handle)
                    return
                }
                var resumeProof: CloudOriginalCommandProofYield?
                switch intent {
                case .adoption: _ = try await connection.confirmAdoption(handle)
                case .publication:
                    await observePublicationConfirmation?()
                    guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused,
                          expected == generation, revision == hostRevision,
                          self.backend === backend, backend.connection === connection else {
                        try await connection.cancelPreview(handle)
                        return
                    }
                    _ = try await connection.confirmPublication(handle)
                    if expected == generation, revision == hostRevision,
                       self.backend === backend, backend.connection === connection, !closing {
                        publicationAccepted = true
                        publicationResultDelivered = true
                        // Durable acceptance ends confirmation, not this task.
                        publicationConfirmation?(.accepted)
                    }
                case .resume:
                    guard let resumePreparation,
                          let proof = try await resumeBindingAfterPolicyDrain(handle, preparation: resumePreparation, service: connection,
                        generation: expected, revision: revision) else {
                        try await connection.cancelPreview(handle)
                        return
                    }
                    resumeProof = proof
                }
                guard expected == generation, revision == hostRevision,
                      self.backend === backend, backend.connection === connection else { return }
                if intent != .resume && (closing || suspended || replacing) { return }
                connectionFailure = nil
                // Durable confirmation succeeded. Enable only now; subsequent
                // status comes from policy. Adoption/publication keep their
                // recoverable operation if later work fails.
                saveSettings(.init(enabled: true, paused: settings.paused,
                    automatic: settings.automatic, media: settings.media))
                guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused else { return }
                if intent == .resume {
                    guard let resumeProof, let admission = CloudOriginalCommandProofYield.admission else { throw CancellationError() }
                    try await admission(resumeProof)
                } else { await waitForInputs() }
                guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused,
                      expected == generation, revision == hostRevision,
                      self.backend === backend, backend.connection === connection else { return }
                if intent == .resume {
                    // Resume invalidates the old identity proof. The ordered
                    // native read above must return before policy refreshes.
                    receive(.refresh(.cloud))
                    await waitForInputs()
                    guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused,
                          expected == generation, revision == hostRevision,
                          self.backend === backend, backend.connection === connection else { return }
                } else {
                    await runOwnWork(backend, generation: expected, revision: revision, onFailure: { failure in
                        guard publicationAccepted, expected == self.generation, revision == self.hostRevision,
                              self.backend === backend, backend.connection === connection, !self.closing, !self.replacing else { return }
                        publicationBackgroundFailure?(failure)
                    })
                }
            } catch {
                if expected == generation, revision == hostRevision, !closing {
                    recordConnectionFailure(error)
                    let failure: CloudConnectionFailure?
                    if case BridgeError.cloudConnection(let value) = error { failure = value }
                    else { failure = nil }
                    publicationResultDelivered = true
                    publicationConfirmation?(.refused(failure))
                }
            }
        }
        await ownTask?.value
    }
    private func resumeBindingAfterPolicyDrain(_ handle: CloudConnectionPreview, preparation: ResumePreparation, service: CoreCloudConnectionService,
                                              generation expected: UInt64, revision: UInt64) async throws -> CloudOriginalCommandProofYield? {
        while true {
            let admission: ResumeAdmission = try await withCheckedThrowingContinuation {
                enqueue(.resumeBinding(handle, preparation: preparation, service: service, generation: expected, revision: revision, acknowledged: $0))
            }
            switch admission {
            case .resumed(let proof): return proof
            case .cancelled: return nil
            case .pending:
                observeResumePolicyDrain?(.waiting)
                await prepareConnectionAuthority()
                // Do not join ownTask: it owns this confirmation. Results and
                // their follow-up commands still run on the ordinary input lane.
                for original in Array(operationTasks.values) { await original.value }
            }
        }
    }
    /// User action only. Automatic connection/policy wakes cannot release a wait.
    public func resumeWaitingAdoption() {
        guard !closing, !suspended, !replacing, !settings.paused, !awaitingInitialOwnProof,
              initialOwnDrain == nil, !initialActivationReturned, ownTask == nil,
              recovery?.adoptionTransportWait == true, let operation = recovery?.operation,
              let backend, let connection = backend.connection, let own = backend.own else { return }
        let expected = generation, revision = hostRevision
        startOwnTask(backend: backend, generation: expected, revision: revision) { [self] in
            defer { finishOwnWork() }
            do {
                let resumed = try await own.resumeAdoption(operation)
                guard expected == generation, revision == hostRevision, self.backend === backend,
                      !closing, !suspended, !replacing,
                      (!initialAdoptionActive || !initialAdoptionCancelled) else { return }
                let current = try await connection.recoveryStatus()
                guard expected == generation, revision == hostRevision, self.backend === backend else { return }
                recovery = current
                if resumed { await runOwnWork(backend, generation: expected, revision: revision) }
            } catch {
                if expected == generation, revision == hostRevision, !closing { operationFailed = true }
            }
        }
    }
    public func resumeConnection() {
        guard !closing, !suspended, !replacing, !settings.paused, !awaitingInitialOwnProof,
              initialOwnDrain == nil, !initialActivationReturned, ownTask == nil, let backend, let connection = backend.connection else { return }
        let expected = generation, revision = hostRevision
        startOwnTask(backend: backend, generation: expected, revision: revision) { [self] in
            defer { finishOwnWork() }
            do {
                if initialAdoptionEnabledBefore == nil { try await connection.trigger() }
                await runOwnWork(backend, generation: expected, revision: revision)
            }
            catch {
                if expected == generation, !closing {
                    #if DEBUG
                    DebugCloudDiagnostics.record(.ownWorkTrigger, error: error)
                    #endif
                    operationFailed = true
                }
            }
        }
    }
    private func startOwnTask(backend original: any SyncCoordinatorBackend, generation expected: UInt64, revision: UInt64,
                              resumePreparation: ResumePreparation? = nil, operation: @escaping @MainActor () async -> Void) {
        let id = UUID()
        ownOwner = id
        ownResumePreparation = resumePreparation.map { (id, $0) }
        ownTask = Task { [self] in
            let admission: @Sendable (CloudOriginalCommandProofYield) async throws -> Void = { [self] proof in
                try await admitProof(proof, owner: .own(id), backend: original, generation: expected, revision: revision)
            }
            await CloudOriginalCommandProofYield.$admission.withValue(admission) { await operation() }
        }
    }
    private func finishOwnWork() {
        if let id = ownOwner {
            for wait in proofWaits.values.filter({ $0.owner == .own(id) }) {
                proofWaits.removeValue(forKey: wait.proof.id); wait.proof.finish(CancellationError())
            }
        }
        ownResumePreparation = nil; ownOwner = nil; ownTask = nil
        retryAccountCompletion()
        if ownWorkRequested, !closing, !replacing, !suspended, !settings.paused, !awaitingInitialOwnProof, initialOwnDrain == nil { ownWorkRequested = false; resumeConnection() }
    }
    public func cancelConnectionOperation() async {
        guard !closing, !suspended, !replacing, !activatingInitialAdoption,
              let backend, let own = backend.own, let operation = recovery?.operation else { return }
        initialAdoptionCancelled = true
        do {
            try await own.cancel(operation)
            await ownTask?.value
            recovery = try await backend.connection?.recoveryStatus()
            await reconcileInitialAdoption(backend)
            receive(.refresh(.cloud))
        } catch {
            #if DEBUG
            DebugCloudDiagnostics.record(.connectionOperationCancel, error: error)
            #endif
            operationFailed = true
        }
    }
    private func saveEnabled(_ enabled: Bool) {
        saveSettings(.init(enabled: enabled, paused: settings.paused,
            automatic: settings.automatic, media: settings.media))
    }
    private func restoreInitialAdoptionEnabled() {
        guard let earlier = initialAdoptionEnabledBefore else { return }
        initialAdoptionEnabledBefore = nil
        saveEnabled(earlier)
    }
    private func reconcileInitialAdoption(_ original: any SyncCoordinatorBackend) async {
        guard initialAdoptionEnabledBefore != nil else { return }
        await withCheckedContinuation { enqueue(.reconcile(original, acknowledged: $0)) }
        await waitForInputs()
    }
    private func reconcileInitialAdoptionOnInput(_ original: any SyncCoordinatorBackend) async {
        guard initialAdoptionEnabledBefore != nil, !activatingInitialAdoption, let connection = original.connection else { return }
        let expected = generation, revision = hostRevision
        do {
            // No count, missing status, or transient failed step proves an end.
            let current = try await connection.recoveryStatus()
            guard expected == generation, revision == hostRevision else { return }
            recovery = current
            guard current == nil else { return }
            let output = try await original.input(.refresh(.cloud))
            guard expected == generation, revision == hostRevision else { return }
            consume(output, generation: expected)
            let policy = try await CoreCloudWorkflowService().policy()
            let usable = try await policy.status().controls.bindingUsable
            guard expected == generation, revision == hostRevision else { return }
            if usable == true { initialAdoptionEnabledBefore = nil; initialActivationReturned = false }
            else if usable == false { restoreInitialAdoptionEnabled() }
        } catch { /* Unknown authority preserves the accepted operation's setting. */ }
    }
    private func willActivateInitialAdoption(generation expected: UInt64, revision: UInt64) throws {
        guard !closing, !suspended, !replacing, !settings.paused, !initialAdoptionCancelled,
              ownTask?.isCancelled == false, expected == generation, revision == hostRevision else { throw CancellationError() }
        activatingInitialAdoption = true
    }
    private func didActivateInitialAdoption(generation expected: UInt64) async {
        defer { activatingInitialAdoption = false }
        if case .ready(let actual) = try? await CoreCollectionService().lifecycle(), actual > expected {
            initialActivationReturned = true
        }
    }
    private func runOwnWork(_ backend: any SyncCoordinatorBackend, generation expected: UInt64, revision: UInt64,
                            onFailure: (@MainActor (CloudConnectionFailure?) -> Void)? = nil) async {
        guard let own = backend.own else { return }
        do {
            while !closing, !Task.isCancelled, revision == hostRevision {
                await waitForInputs()
                guard !closing, !Task.isCancelled, !replacing, !suspended,
                      expected == generation, revision == hostRevision, self.backend === backend else { return }
                let performCore: @MainActor @Sendable (@escaping @Sendable () async throws -> Void) async throws -> Void = { [self] operation in
                    try await self.performOwnCore(operation, backend: backend, generation: expected, revision: revision)
                }
                let progress: CloudConnectionProgress
                if initialAdoptionEnabledBefore != nil {
                    guard !initialAdoptionCancelled, !settings.paused, !suspended, !awaitingInitialOwnProof else { return }
                    progress = try await own.runNext(admission: initialOwnAdmission,
                        performCore: performCore,
                        beforeActivation: { [self] in try await self.willActivateInitialAdoption(generation: expected, revision: revision) },
                        afterActivation: { [self] in await self.didActivateInitialAdoption(generation: expected) })
                } else { progress = try await own.runNext(performCore: performCore) }
                // Activation can deliver CollectionReplaced before this call
                // returns. Finish this original task without touching successor
                // state; replacementTask joins it, closes, then attaches once.
                guard !initialActivationReturned, !replacing, expected == generation, revision == hostRevision else { return }
                let current = try await backend.connection?.recoveryStatus()
                if expected == generation { recovery = current }
                await reconcileInitialAdoption(backend)
                receive(.refresh(.cloud))
                // A send is requested only by the Rust policy's emitted work.
                // Continue only after real progress; no idle/retry polling.
                switch progress { case .progressed: continue; case .idle, .sendEntities: return }
            }
        } catch {
            #if DEBUG
            DebugCloudDiagnostics.record(.ownWork, error: error)
            #endif
            if expected == generation, !closing, !initialAdoptionCancelled, !(error is CancellationError) {
                operationFailed = true
                let failure: CloudConnectionFailure?
                if case BridgeError.cloudConnection(let value) = error { failure = value }
                else { failure = nil }
                onFailure?(failure)
            }
            if expected == generation, revision == hostRevision, !replacing, self.backend === backend {
                if let current = try? await backend.connection?.recoveryStatus(),
                   expected == generation, revision == hostRevision, self.backend === backend { recovery = current }
                await reconcileInitialAdoption(backend)
            }
        }
    }
    private func performOwnCore(_ operation: @escaping @Sendable () async throws -> Void,
                                backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64) async throws {
        let id = UUID()
        try await withCheckedThrowingContinuation {
            enqueue(.ownCore(id, backend: backend, generation: generation, revision: revision, acknowledged: $0))
        }
        // Run on the original executor task, retaining its job cancellation and
        // exact outcome. Only local Core work borrows the input lane's phase.
        let outcome: Result<Void, any Error>
        do { try await operation(); outcome = .success(()) }
        catch { outcome = .failure(error) }
        await withCheckedContinuation { enqueue(.ownCoreReturned(acknowledged: $0)) }
        enqueue(.corePhaseFinished(id))
        try outcome.get()
    }
    func foregroundAfterLocalResume() async {
        guard !closing, !replacing else { return }
        let expected = generation, revision = hostRevision
        let needsProof = suspended || awaitingResumeProof
        await backend?.resumeAfterCoreResume()
        guard !closing, !replacing, expected == generation, revision == hostRevision else { return }
        // Core rebinds media even when Swift's background flush or freeze failed.
        // Reapply settings before delivering the retained screen references.
        deliveredVisibleMedia = nil
        mediaEnabledRevision = nil
        awaitingResumeProof = needsProof
        suspended = false
        if pendingPreviewCancellation { pendingPreviewCancellation = false; await cancelConnectionPreview() }
        guard !closing, !replacing, expected == generation, revision == hostRevision else { return }
        // Starting the app-owned read is ordered, but waiting for Apple is not
        // part of local resume. Policy refresh must not retire the held SDK
        // merely because the resumed binding is awaiting this fresh proof.
        if !needsProof { foregroundAfterAccountProof() }
        enqueue(.account(false))
        await waitForInputs()
    }
    private func foregroundAfterAccountProof() {
        awaitingResumeProof = false
        if !settings.paused { initialOwnAdmission.setOpen(true) }
        receive(.settings(settings))
        enqueue(.replayVisibleMedia)
        if let reachable { receive(.network(reachable: reachable)) }
        if pendingExpiry { pendingExpiry = false; receive(.backgroundExpiring) }
        if pendingLocalRefresh { pendingLocalRefresh = false; receive(.refresh(.local)) }
        reportAvailableSpace(); receive(.foreground)
        if initialAdoptionEnabledBefore != nil, !initialAdoptionCancelled { resumeConnection() }
    }
    @discardableResult func backgroundAfterLocalFlush(deadlineMilliseconds: Int64?) async -> Bool {
        await replacementTask?.value
        guard !replacing else { return false }
        if initialAdoptionEnabledBefore != nil { initialOwnAdmission.setOpen(false) }
        if let deadlineMilliseconds { receive(.background(deadlineMilliseconds: deadlineMilliseconds)) }
        else { receive(.backgroundExpiring) }
        await waitForInputs()
        if initialAdoptionEnabledBefore != nil {
            drainInitialOwnWork()
            await initialOwnDrain?.value
        }
        await drainOperations()
        suspended = true
        cancelAccountTask(shutdown: false)
        revokeProofWaits()
        await waitForInputs()
        do {
            try await backend?.freezeForSuspend()
            return true
        } catch {
            suspended = false; await backend?.resumeAfterCoreResume()
            #if DEBUG
            DebugCloudDiagnostics.record(.suspendFreeze, error: error)
            #endif
            operationFailed = true; return false
        }
    }
    public func backgroundExpired() {
        if suspended { pendingExpiry = true }
        else {
            for task in operationTasks.values { task.cancel() }
            ownTask?.cancel()
            receive(.backgroundExpiring)
        }
    }

    // The application's existing quit owner supplies its completion. No native
    // library terminates the application or invents a quit owner.
    func quitAfterLocalFlush(completion: @escaping @MainActor () -> Void) async {
        guard backend != nil else { completion(); return }
        quitCompletion = completion
        receive(.quitRequested)
        await waitForInputs()
        // QuitFlush moves to a child task. Join its original result, leaving
        // unrelated work for cancelAndDrain to cancel before joining it.
        while !quitOperationIDs.isEmpty {
            let originals = quitOperationIDs.compactMap { operationTasks[$0] }
            for task in originals { await task.value }
            await waitForInputs()
        }
    }

    static func changeCause(_ source: OpSource) -> CloudPolicyChangeCause {
        switch source {
        case .cloudIncoming, .cloudRekey: .cloud
        default: .local
        }
    }

    private func installProofAdmission(_ original: any SyncCoordinatorBackend, generation expected: UInt64, revision: UInt64) async {
        await original.setProofAdmission({ [weak self, weak original] proof in
            guard let self, let original else { throw CancellationError() }
            try await self.admitProof(proof, owner: nil, backend: original, generation: expected, revision: revision)
        }, acknowledgeOutputs: { [weak self] outputs in
            guard let self else { return }
            await self.acknowledgeProofOutputs(outputs, generation: expected, revision: revision)
        })
    }
    private func acknowledgeProofOutputs(_ outputs: [CloudPolicyOutput], generation expected: UInt64, revision: UInt64) async {
        await withCheckedContinuation { acknowledged in
            enqueue(.result(.init(outputs: outputs, error: nil, deferred: false), generation: expected, revision: revision,
                shutdownCancelled: false, acknowledged: acknowledged))
        }
        await waitForInputs()
    }
    private func admitProof(_ proof: CloudOriginalCommandProofYield, owner: ProofOwner?, backend original: any SyncCoordinatorBackend,
                            generation expected: UInt64, revision: UInt64) async throws {
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            enqueue(.proofYield(proof, owner: owner, backend: original, generation: expected, revision: revision))
            try await proof.wait()
        } onCancel: {
            proof.finish(CancellationError())
            Task { @MainActor [weak self] in self?.retryAccountCompletion() }
        }
    }
    // Retain every original task. Only its typed terminal/ACK yield or exact
    // quiescent dependency park may admit account proof. A live own task fences
    // admission until its own yield; another task's yield cannot cover it.
    private var commandsAdmitAccountProof: Bool {
        operationTasks.keys.allSatisfy { id in
            quiescentDependencies.values.contains(id) || proofWaits.values.contains { $0.owner == .command(id) && $0.proof.isPending }
        } && (ownTask == nil || ownOwner.map { id in
            ownResumePreparation?.owner == id || proofWaits.values.contains { $0.owner == .own(id) && $0.proof.isPending }
        } == true)
    }
    private func containsProofOwner(_ owner: ProofOwner?) -> Bool {
        switch owner {
        case .command(let id): operationTasks[id] != nil
        case .own(let id): ownOwner == id && ownTask != nil
        case nil: true
        }
    }
    private func retryAccountCompletion() {
        if let held = heldAccountCompletion {
            heldAccountCompletion = nil
            enqueue(held)
        }
        requestProofReadIfReady()
    }
    private func requestProofReadIfReady() {
        guard !closing, !suspended, !replacing, accountTask == nil, pendingAccountRead == nil,
              commandsAdmitAccountProof,
              proofWaits.values.contains(where: { $0.proof.isPending && $0.read == nil }),
              !inputQueue.contains(where: { if case .proofAccountRead = $0 { true } else { false } }) else { return }
        enqueue(.proofAccountRead)
    }
    private func revokeProofWaits() {
        for wait in proofWaits.values { wait.proof.finish(CancellationError()) }
        proofWaits.removeAll()
        retryAccountCompletion()
    }

    private func enqueue(_ input: Input) {
        inputQueue.append(input)
        #if DEBUG
        // These admissions can wait behind a Core phase before their reducer
        // runs. Observe that existing wait without changing queue eligibility.
        if !corePhase.isEmpty {
            switch input {
            case .resumeBinding: observeResumePolicyDrain?(.waiting)
            case .completeAccount: observeAccountPolicyDrain?(.waiting)
            default: break
            }
        }
        #endif
        guard inputTask == nil else { return }
        inputTask = Task {
            defer { inputTask = nil }
            await CloudOriginalCommandProofYield.$admission.withValue(nil) {
            await CloudQuiescentDependencyWait.$transition.withValue(nil) {
            await SyncCommandTaskContext.$current.withValue(nil) {
                while !inputQueue.isEmpty {
                    let index: Int
                    if corePhase.isEmpty { index = 0 }
                    else {
                        // Leave the reducer free to ACK actual returned results.
                        // Their tasks release the phase only after joining ACK.
                        guard let ready = inputQueue.firstIndex(where: {
                            switch $0 {
                            case .result, .corePrepared, .ownCoreReturned, .corePhaseFinished: true
                            case .proofYield(_, let owner, _, _, _): owner?.commandID.map { corePhase[$0] != nil } == true
                            case .quiescentDependency: true
                            case .nativeInput(_, _, _, let owner, _): owner.map { corePhase[$0] != nil } == true
                            default: false
                            }
                        }) else { return }
                        index = ready
                    }
                    await reduce(inputQueue.remove(at: index))
                }
            }
            }
            }
        }
    }
    private func reduce(_ input: Input) async {
        switch input {
        case .proofYield(let proof, let owner, let original, let expected, let revision):
            guard proof.isPending, !closing, !suspended, !replacing, !backendRetired,
                  expected == generation, revision == hostRevision, backend === original,
                  containsProofOwner(owner) else {
                proof.finish(CancellationError()); return
            }
            proofWaits[proof.id] = ProofWait(proof: proof, owner: owner)
            if let id = owner?.commandID { await reduce(.corePhaseFinished(id)) }
            retryAccountCompletion()
        case .quiescentDependency(let wait, let phase, let owner, let original, let expected, let revision, let acknowledged):
            switch phase {
            case .revoke:
                quiescentDependencies.removeValue(forKey: wait.id)
                #if DEBUG
                observeQuiescentDependency?(false)
                #endif
                acknowledged.resume()
            case .park:
                guard !closing, !suspended, !replacing, !backendRetired,
                      expected == generation, revision == hostRevision, backend === original,
                      operationTasks[owner] != nil else { acknowledged.resume(throwing: CancellationError()); return }
                quiescentDependencies[wait.id] = owner
                #if DEBUG
                observeQuiescentDependency?(true)
                #endif
                retryAccountCompletion()
                acknowledged.resume()
            }
        case .proofAccountRead:
            guard accountTask == nil, pendingAccountRead == nil, commandsAdmitAccountProof,
                  proofWaits.values.contains(where: { $0.proof.isPending && $0.read == nil }) else { return }
            await reduce(.account(false))
        case .nativeInput(let work, let expected, let revision, let owner, let acknowledged):
            // A failed Core command has already returned its actual result and
            // joined ACK. Hand its phase to this required follow-up input turn
            // without letting unrelated queued inputs pass between them.
            if let owner { await reduce(.corePhaseFinished(owner)) }
            do {
                let output = try await work()
                await reduce(.result(.init(outputs: [output], error: nil, deferred: false),
                    generation: expected, revision: revision, shutdownCancelled: false, acknowledged: nil))
                enqueue(.nativeInputReturned(output, acknowledged: acknowledged))
            } catch { acknowledged.resume(throwing: error) }
        case .nativeInputReturned(let output, let acknowledged):
            acknowledged.resume(returning: output)
        case .corePrepared(let acknowledged), .ownCoreReturned(let acknowledged): acknowledged.resume()
        case .corePhaseFinished(let id):
            guard let phase = corePhase.removeValue(forKey: id) else { return }
            phase.finish()
            if corePhase.isEmpty {
                lastCorePhase = nil
                let lists = pendingActionLists
                pendingActionLists.removeAll()
                for list in lists { startActions(list.issued, backend: list.backend, revision: list.revision) }
            }
            retryAccountCompletion()
        case .ownCore(let id, let original, let expected, let revision, let acknowledged):
            guard !closing, !suspended, !replacing, ownTask?.isCancelled == false,
                  expected == generation, revision == hostRevision, backend === original, !backendRetired else {
                acknowledged.resume(throwing: CancellationError())
                return
            }
            // Local publication Bind/WriteDecision and adoption mutations own
            // this input turn. Their native network waits stay outside it.
            let phase = CorePhase()
            corePhase[id] = phase; lastCorePhase = phase
            #if DEBUG
            observeCausalBoundary?(.ownCoreStarted, nil)
            #endif
            acknowledged.resume()
        case .reconcile(let original, let acknowledged):
            if backend === original, !backendRetired { await reconcileInitialAdoptionOnInput(original) }
            acknowledged?.resume()
        case .barrier(let continuation): continuation.resume()
        case .visibleMedia(let names, let expected):
            guard !closing, expected >= generation else { return }
            desiredVisibleMedia = (expected, Set(names).sorted())
            await replayVisibleMedia()
        case .replayVisibleMedia: await replayVisibleMedia()
        case .completeAccount(let read, let original, let expected, let revision, let control, let complete, let acknowledged):
            guard !closing, !suspended, !replacing, accountTask?.isCancelled == false,
                  expected == generation, revision == hostRevision, backend === original else {
                #if DEBUG
                observeAccountCompletion(read, original: original, expected: expected, revision: revision, admitted: false)
                #endif
                #if DEBUG
                var result = await complete(false)
                result.causalReadOrdinal = read
                #else
                let result = await complete(false)
                #endif
                // Only the original completion's generated refusal can use
                // this original task's recorded shutdown ownership. Results
                // from commands, follow-ups and legacy readers never enter it.
                let shutdownOwned = control === accountTaskControl && control.shutdownOwned
                    && expected == generation && revision == hostRevision && backend === original
                    && pendingAccountRead.map { $0 != read } != true
                    && result.accountReadRefusal?.read == read
                    && result.accountReadRefusal?.isGeneratedCancellation == true
                await reduce(.result(result, generation: expected, revision: revision, shutdownCancelled: shutdownOwned, acknowledged: nil))
                acknowledged.resume(returning: true)
                return
            }
            guard commandsAdmitAccountProof, !inputQueue.contains(where: { if case .result = $0 { true } else { false } }) else {
                // Keep this exact resolved app read and its completion. A proof
                // yield wakes admission without joining the yielding task.
                heldAccountCompletion = input
                observeAccountPolicyDrain?(.waiting)
                return
            }
            // Lookup has returned, but no proof has been committed yet. Only
            // this lane may rotate authority, after all original command ACKs.
            observeAccountPolicyDrain?(.finished)
            #if DEBUG
            observeAccountCompletion(read, original: original, expected: expected, revision: revision, admitted: true)
            #endif
            #if DEBUG
            var result = await complete(true)
            result.causalReadOrdinal = read
            #else
            let result = await complete(true)
            #endif
            let supersededAccountCancellation = result.error is CancellationError
                && pendingAccountRead.map { $0 != read } == true
                && expected == generation && revision == hostRevision && backend === original
            #if DEBUG
            if let error = result.error, error is CancellationError {
                DebugCloudDiagnostics.record(
                    supersededAccountCancellation ? .accountResultSuperseded : .accountResultCurrent, error: error)
            }
            #endif
            await reduce(.result(result, generation: expected, revision: revision, shutdownCancelled: false,
                supersededAccountCancellation: supersededAccountCancellation, acknowledged: nil))
            #if DEBUG
            observeCausalBoundary?(.proofResultReduced, nil)
            #endif
            enqueue(.accountOutputsReturned(read, backend: original, generation: expected, revision: revision,
                result: result, acknowledged: acknowledged))
        case .accountOutputsReturned(let read, let original, let expected, let revision, let result, let acknowledged):
            if let next = result.afterOutputs {
                let admitted = !closing && !suspended && !replacing && accountTask?.isCancelled == false
                    && expected == generation && revision == hostRevision && backend === original
                    && pendingAccountRead.map { $0 != read } != true
                #if DEBUG
                observeAccountCompletion(read, original: original, expected: expected, revision: revision, admitted: admitted, followup: true)
                #endif
                #if DEBUG
                var followup = await next(admitted)
                followup.causalReadOrdinal = read
                #else
                let followup = await next(admitted)
                #endif
                let supersededAccountCancellation = followup.error is CancellationError
                    && pendingAccountRead.map { $0 != read } == true
                    && expected == generation && revision == hostRevision && backend === original
                await reduce(.result(followup, generation: expected, revision: revision, shutdownCancelled: false,
                    supersededAccountCancellation: supersededAccountCancellation, acknowledged: nil))
                enqueue(.accountOutputsReturned(read, backend: original, generation: expected, revision: revision,
                    result: followup, acknowledged: acknowledged))
                return
            }
            #if DEBUG
            observeCausalBoundary?(.accountAccepted(read: read, verified: result.accountVerified, succeeded: result.error == nil), nil)
            #endif
            if result.accountVerified { await reduce(.ownAccountProof(read, generation: expected, revision: revision)) }
            if result.error == nil, result.accountVerified {
                startRetainedRecoveryAfterProof(original, generation: expected, revision: revision)
            }
            let waiting = proofWaits.values.filter {
                $0.read == read || ($0.read == nil && (result.error != nil || !result.accountVerified))
            }
            for wait in waiting {
                proofWaits.removeValue(forKey: wait.proof.id)
                wait.proof.finish(result.error ?? (result.accountVerified ? nil : BridgeError.unavailable(reason: "Cloud account proof unavailable")))
            }
            #if DEBUG
            observeCausalBoundary?(.proofAcknowledgementSending, nil)
            #endif
            acknowledged.resume(returning: true)
        case .resumeBinding(let handle, let preparation, let service, let expected, let revision, let acknowledged):
            guard !closing, !suspended, !replacing, !settings.paused, ownTask?.isCancelled == false,
                  expected == generation, revision == hostRevision, backend?.connection === service,
                  ownResumePreparation?.owner == ownOwner, ownResumePreparation?.token === preparation else {
                #if DEBUG
                observeCausalBoundary?(.resumeCancelled, nil)
                #endif
                acknowledged.resume(returning: .cancelled)
                return
            }
            // Returned outputs and their follow-up commands must be fully
            // acknowledged before Resume can replace their native owner.
            let queuedResult = inputQueue.contains { if case .result = $0 { return true }; return false }
            guard operationTasks.isEmpty, accountTask == nil, pendingAccountRead == nil, !queuedResult else {
                acknowledged.resume(returning: .pending)
                return
            }
            // Resume rotates Core's command owner and backend guard. Admit it
            // only after every old command result was acknowledged, and keep
            // policy/account inputs ordered behind the actual native call.
            observeResumePolicyDrain?(.finished)
            ownResumePreparation = nil // atomic revocation before Core/SDK work
            do {
                #if DEBUG
                observeCausalBoundary?(.resumeEntered, nil)
                #endif
                let proof = try await service.resumeBindingForAccountProof(handle)
                #if DEBUG
                observeCausalBoundary?(.resumeReturned, nil)
                #endif
                acknowledged.resume(returning: .resumed(proof))
            } catch {
                #if DEBUG
                observeCausalBoundary?(.resumeRefused(authorityChanged: (error as? BridgeError) == .cloudConnection(.authorityChanged)), nil)
                #endif
                acknowledged.resume(throwing: error)
            }
        case .result(let result, let expected, let revision, let shutdownCancelled, let supersededAccountCancellation, let acknowledged):
            #if DEBUG
            let failureBefore = failureRevision
            defer {
                observeCausalBoundary?(.resultReduced(command: result.causalCommandOrdinal, read: result.causalReadOrdinal,
                    cancellation: result.error is CancellationError, shutdownOwned: shutdownCancelled,
                    before: failureBefore, after: failureRevision), nil)
            }
            #endif
            if expected == generation, revision == hostRevision, !replacing, !suspended {
                for output in result.outputs.dropFirst(result.acknowledgedOutputCount) {
                    consume(output, generation: expected)
                }
                if result.deferred { hasDeferredNativeWork = true }
                if let error = result.error, !(closing && shutdownCancelled && error is CancellationError) {
                    #if DEBUG
                    if let failure = result.diagnosticFailure {
                        DebugCloudDiagnostics.record(failure)
                    } else {
                        DebugCloudDiagnostics.record(.commandResult, error: error)
                    }
                    #endif
                    if result.originalGuardRefusal == nil, !supersededAccountCancellation, !(result.mediaCancellation && error is CancellationError) {
                        operationFailed = true; failureRevision &+= 1
                    }
                }
                if !closing, result.afterOutputs == nil, result.accountVerified, awaitingResumeProof { foregroundAfterAccountProof() }
                if !closing, initialAdoptionEnabledBefore != nil, ownTask == nil, let backend {
                    enqueue(.reconcile(backend, acknowledged: nil))
                }
            }
            acknowledged?.resume()
            retryAccountCompletion()
        case .replaced(let next):
            guard next > generation || (next == generation && setupState == .failed) else { return }
            if desiredVisibleMedia?.generation != next { desiredVisibleMedia = nil }
            deliveredVisibleMedia = nil
            generation = next; status = nil; preview = nil; recovery = nil
            retainedRecoveryStart = nil
            hasDeferredNativeWork = false
            hostRevision &+= 1
            // Local collection generations still advance without an iCloud
            // installation. Only successful platform preparation admits an owner.
            guard platformPrepared, retainedHostAdmitted else { return }
            replacing = true
            initialOwnAdmission.setOpen(false)
            awaitingResumeProof = false
            flushTimer?.invalidate(); flushTimer = nil
            if replacementTask == nil {
                let originalAttachment = attachmentTask
                replacementTask = Task { await replaceBackend(after: originalAttachment) }
            }
        case .lifecycle(let lifecycle):
            if case .ready(let next) = lifecycle, next < generation { return }
            if case .ready(let next) = lifecycle, next != generation {
                await reduce(.replaced(next))
            } else { await policy(.refresh(.cloud)) }
        case .policy(.visibleMedia(let names)):
            await reduce(.visibleMedia(names, generation: generation))
        case .policy(let event): await policy(event)
        case .core(let event):
            guard event.generation == generation else { return }
            switch event.kind {
            case .changes(let changes): await policy(.refresh(Self.changeCause(changes.source)))
            case .sessionStarted, .sessionEnded, .available, .mediaArrived: await policy(.refresh(.cloud))
            default: break
            }
        case .persisted(let revision):
            guard revision == hostRevision, !closing, !replacing, !suspended, let backend else { return }
            let requested = await backend.takeAccountReadRequest()
            guard revision == hostRevision, !closing, !replacing, !suspended else { return }
            if requested { enqueue(.account(true)) }
            await policy(.refresh(.cloud))
        case .ownAccountProof(let read, let expected, let revision):
            guard read == initialOwnResumeRead, expected == generation, revision == hostRevision,
                  !closing, !suspended, !replacing, !settings.paused, initialOwnDrain == nil else { return }
            initialOwnResumeRead = nil
            awaitingInitialOwnProof = false
            initialOwnAdmission.setOpen(true)
            resumeConnection()
        case .account(let transition):
            guard !closing, !replacing, !suspended, let backend else { return }
            do {
                // Establish Rust's read generation on the ORDERED input lane,
                // before awaiting any network resolution. A later signal makes
                // an earlier in-flight reply stale immediately.
                pendingAccountRead = try await backend.beginAccountRead(transition: transition)
                // These are real Core-issued generations on the existing lane.
                // A later app observation supersedes a proof waiter too.
                for key in Array(proofWaits.keys) where proofWaits[key]?.proof.isPending == true {
                    proofWaits[key]?.read = pendingAccountRead
                }
                if awaitingInitialOwnProof { initialOwnResumeRead = pendingAccountRead }
                startAccountRead()
            } catch {
                #if DEBUG
                DebugCloudDiagnostics.record(.beginAccountRead, error: error)
                #endif
                operationFailed = true
                for wait in proofWaits.values { wait.proof.finish(error) }
                proofWaits.removeAll()
            }
        }
    }
    private func replayVisibleMedia() async {
        guard let desired = desiredVisibleMedia, desired.generation == generation,
              mediaEnabledRevision == hostRevision,
              !closing, !suspended, !replacing, !backendRetired, backend != nil else { return }
        guard deliveredVisibleMedia?.revision != hostRevision || deliveredVisibleMedia?.names != desired.names else { return }
        await policy(.visibleMedia(names: desired.names))
    }
    private func policy(_ event: CloudPolicyEvent) async {
        guard !closing else { return }
        if case .network(let value) = event { reachable = value }
        if awaitingResumeProof, !suspended {
            switch event {
            case .refresh(let cause):
                if cause == .local { pendingLocalRefresh = true }
                return
            case .network, .foreground, .volumeSpaceAvailable: return
            default: break // Explicit controls still reach Core immediately.
            }
        }
        if suspended {
            if case .refresh(.local) = event { pendingLocalRefresh = true }
            if case .backgroundExpiring = event { pendingExpiry = true }
            return
        }
        guard !replacing, let backend else { return }
        let expected = generation, revision = hostRevision
        do {
            let output = try await backend.input(event)
            guard expected == generation, revision == hostRevision, !replacing else { return }
            if case .settings(let value) = event {
                mediaEnabledRevision = value.media ? revision : nil
                if !value.media { deliveredVisibleMedia = nil }
            }
            if case .visibleMedia(let names) = event { deliveredVisibleMedia = (revision, names) }
            consume(output, generation: expected)
            if case .settings(let value) = event, value.media { await replayVisibleMedia() }
        } catch {
            guard expected == generation, revision == hostRevision, !replacing else { return }
            if case BridgeError.cloudPolicy(.generationChanged) = error {
                // Activation can advance the retained Core Host before its
                // replacement event reaches this ordered lane. Only Core's
                // ready successor proves this original input was superseded.
                let lifecycle: CollectionLifecycle?
                do { lifecycle = try await CoreCollectionService().lifecycle() }
                catch { lifecycle = nil } // Failed proof retains the input failure below.
                guard expected == generation, revision == hostRevision, !replacing else { return }
                if let lifecycle, case .ready(let next) = lifecycle, next > expected {
                    enqueue(.replaced(next))
                    return
                }
            }
            #if DEBUG
            DebugCloudDiagnostics.record(.policyInput, error: error)
            #endif
            operationFailed = true; failureRevision &+= 1
        }
    }
    private func consume(_ output: CloudPolicyOutput, generation expected: UInt64) {
        #if DEBUG
        if expected == generation {
            switch output {
            case .coreActionFailed(let error, _): DebugCloudDiagnostics.record(.coreActionFailed, error: error)
            case .applyDeferred(let error): DebugCloudDiagnostics.record(.applyDeferred, error: error)
            default: break
            }
        }
        #endif
        let issued: [CloudPolicyCommand]
        switch output {
        case .commands(let values): issued = values
        case .coreActionFailed(_, let values): issued = values; if expected == generation { operationFailed = true; failureRevision &+= 1 }
        case .applyDeferred: if expected == generation { operationFailed = true; failureRevision &+= 1 }; return
        case .syncNowUnavailable(let reason): if expected == generation { syncNowUnavailable = reason }; return
        }
        // Ordinary and failure outputs can both carry required finalization.
        // The close filter below prevents either from starting a fresh cycle.
        guard let backend, !replacing, !backendRetired else { return }
        let revision = hostRevision
        var core: [Issued] = [], ordered: [Issued] = []
        for command in issued {
            if closing, !Self.requiredDuringClose(command.action) { continue }
            commandSequence &+= 1
            let value = Issued(command: command, generation: expected, sequence: commandSequence)
            switch command.action {
            case .stopEngine, .pauseMedia, .cancelExchange, .cancelBackgroundFlush, .cancelFlushTimer:
                startActions([value], backend: backend, revision: revision)
            case .statusChanged, .applyReady, .recordExchange, .recordQuietPoint, .probeTracking, .runCleanup:
                core.append(value)
            default: ordered.append(value)
            }
        }
        for value in core { startActions([value], backend: backend, revision: revision, corePhaseOwner: true) }
        if !ordered.isEmpty {
            if corePhase.isEmpty { startActions(ordered, backend: backend, revision: revision) }
            else { pendingActionLists.append((ordered, backend, revision)) }
        }
    }
    private func transitionQuiescentDependency(_ wait: CloudQuiescentDependencyWait, phase: CloudQuiescentDependencyWait.Phase,
                                               owner: UUID, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64) async throws {
        try await withCheckedThrowingContinuation { acknowledged in
            enqueue(.quiescentDependency(wait, phase, owner: owner, backend: backend,
                generation: generation, revision: revision, acknowledged: acknowledged))
        }
    }
    private func startActions(_ issued: [Issued], backend: any SyncCoordinatorBackend, revision: UInt64,
                              separateOperation: Bool = false, corePhaseOwner: Bool = false) {
        let id = UUID()
        let predecessor = corePhaseOwner ? lastCorePhase : nil
        if corePhaseOwner {
            let phase = CorePhase(command: issued.first?.command)
            corePhase[id] = phase; lastCorePhase = phase
        }
        let context = SyncCommandTaskContext(generation: generation, revision: revision)
        operationContexts[id] = context
        if issued.contains(where: {
            switch $0.command.action {
            case .quitNow, .quitFlush: true
            default: false
            }
        }) { quitOperationIDs.insert(id) }
        operationTasks[id] = Task { [self] in
            defer {
                quitOperationIDs.remove(id)
                quiescentDependencies = quiescentDependencies.filter { $0.value != id }
                operationTasks.removeValue(forKey: id); operationContexts.removeValue(forKey: id)
                for wait in proofWaits.values.filter({ $0.owner == .command(id) }) {
                    proofWaits.removeValue(forKey: wait.proof.id)
                    wait.proof.finish(CancellationError())
                }
                retryAccountCompletion()
                if corePhaseOwner { enqueue(.corePhaseFinished(id)) }
            }
            // An unfinished predecessor still owns its real Core/result ACK
            // prefix. Its ACK/corePrepared/proofYield releases this phase on the
            // ordered lane; waiting B supplies no admission exemption.
            await predecessor?.wait {
                #if DEBUG
                if let original = predecessor?.command {
                    observeCausalBoundary?(.corePredecessorWaiting(original), issued.first?.command)
                }
                #endif
            }
            #if DEBUG
            if let original = predecessor?.command {
                observeCausalBoundary?(.corePredecessorReleased(original), issued.first?.command)
            }
            #endif
            let proofAdmission: @Sendable (CloudOriginalCommandProofYield) async throws -> Void = { [self] proof in
                try await admitProof(proof, owner: .command(id), backend: backend, generation: context.generation, revision: revision)
            }
            await CloudOriginalCommandProofYield.$admission.withValue(proofAdmission) {
            await SyncCommandTaskContext.$current.withValue(context) {
                for value in issued {
                    let command = value.command
                    if replacing || revision != hostRevision || backendRetired { return }
                    if closing, !Self.requiredDuringClose(command.action) { continue }
                    if !separateOperation {
                        switch command.action {
                        case .runExchange, .backgroundFlush, .quitFlush:
                            startActions([value], backend: backend, revision: revision, separateOperation: true)
                            continue
                        default: break
                        }
                    }
                    var result: SyncCommandResult
                    let park: @Sendable (CloudQuiescentDependencyWait, CloudQuiescentDependencyWait.Phase) async throws -> Void = { [self] wait, phase in
                        try await transitionQuiescentDependency(wait, phase: phase, owner: id, backend: backend,
                            generation: value.generation, revision: revision)
                        #if DEBUG
                        if case .revoke = phase {
                            await MainActor.run { self.observeCausalBoundary?(.dependencyRevocationJoined(wait.id), command) }
                        }
                        #endif
                    }
                    result = await CloudQuiescentDependencyWait.$transition.withValue(park) {
                    await backend.execute(command, corePrepared: { [self] in
                        // The qualified local cleanup pass returned; inventory
                        // remains owned by the original command task.
                        await withCheckedContinuation { enqueue(.corePrepared(acknowledged: $0)) }
                        if corePhaseOwner { enqueue(.corePhaseFinished(id)) }
                    }, acknowledgeOutputs: { [self] outputs in
                        await withCheckedContinuation { acknowledged in
                            enqueue(.result(.init(outputs: outputs, error: nil, deferred: false),
                                generation: value.generation, revision: revision,
                                shutdownCancelled: context.shutdownCancelled, acknowledged: acknowledged))
                        }
                        // Release this local stage before joining follow-up
                        // Core phases. This wait runs on the command task,
                        // never on the reducer which must ACK those results.
                        if corePhaseOwner { enqueue(.corePhaseFinished(id)) }
                        await waitForInputs()
                    }, performInput: { [self] work in
                        try await withCheckedThrowingContinuation { acknowledged in
                            enqueue(.nativeInput(work, generation: value.generation, revision: revision,
                                phaseOwner: corePhaseOwner ? id : nil, acknowledged: acknowledged))
                        }
                    })
                    }
                    // The native executor validates control capabilities as well
                    // as network work. UI effects occur only after that admission.
                    if result.error == nil, revision == hostRevision {
                        switch command.action {
                        case .armFlushTimer(let at):
                            guard value.sequence > timerSequence else { break }
                            timerSequence = value.sequence
                            flushTimer?.invalidate()
                            if !closing {
                                self.flushTimer = Timer(fire: Date(timeIntervalSince1970: Double(at) / 1_000), interval: 0, repeats: false) { [weak self] _ in
                                    Task { @MainActor in self?.receive(.flushTimerFired) }
                                }
                                if let flushTimer { RunLoop.main.add(flushTimer, forMode: .common) }
                            }
                        case .cancelFlushTimer:
                            if value.sequence > timerSequence {
                                timerSequence = value.sequence; flushTimer?.invalidate(); flushTimer = nil
                            }
                        case .statusChanged(let snapshot):
                            if !closing, value.sequence > statusSequence {
                                statusSequence = value.sequence; status = snapshot
                                #if DEBUG
                                DebugCloudDiagnostics.recordStatus(snapshot.error)
                                DebugCloudDiagnostics.recordPolicyStatus(snapshot, generation: value.generation, hostRevision: revision)
                                #endif
                            }
                        case .quitNow: quitCompletion?(); quitCompletion = nil
                        case .quitFlush: quitCompletion?(); quitCompletion = nil
                        default: break
                        }
                    } else if revision == hostRevision, case .quitFlush = command.action {
                        // A failed send still returns control to the app quit owner
                        // after the exact native operation has drained.
                        quitCompletion?(); quitCompletion = nil
                    }
                    while true {
                        await withCheckedContinuation { acknowledged in
                            #if DEBUG
                            observeCausalBoundary?(.commandResultEnqueue, command)
                            result.causalCommandOrdinal = value.sequence
                            #endif
                            enqueue(.result(result, generation: value.generation, revision: revision,
                                shutdownCancelled: context.shutdownCancelled, acknowledged: acknowledged))
                        }
                        #if DEBUG
                        observeCausalBoundary?(.commandAcknowledgementJoined, command)
                        #endif
                        guard let followup = result.afterOutputs else { break }
                        result = await followup(true)
                    }
                }
            }
            }
        }
    }
    private func replaceBackend(after originalAttachment: Task<Void, Never>?) async {
        defer { replacementTask = nil }
        await originalAttachment?.value
        guard platformPrepared else { return }
        for task in operationTasks.values { task.cancel() }
        cancelAccountTask(shutdown: false)
        revokeProofWaits()
        await initialOwnDrain?.value
        // Activation is a local Rust call, with no held SDK callback. Waiting
        // for that call cannot wait for this replacement task. All transport
        // work and unrelated replacements use the ordinary cancel/drain path.
        if activatingInitialAdoption { await ownTask?.value }
        else { ownTask?.cancel() }
        do {
            try await backend?.quiesce()
            await accountTask?.value
            await ownTask?.value
            await drainOperations()
            try await backend?.close()
            backendRetired = true
            backend = nil
            while !closing {
                // generation/revision retain the newest event's successor intent
                // while this task is joining or awaiting the original factory.
                let expected = generation, revision = hostRevision
                let successor: any SyncCoordinatorBackend
                do {
                    successor = try await makeBackend { [weak self] in await self?.nativeEventPersisted(revision: revision) }
                } catch {
                    if !closing, expected != generation || revision != hostRevision { continue }
                    throw error
                }
                // Retain even a stale candidate until its actual close succeeds.
                // No later owner is constructed before that candidate drains.
                backend = successor
                backendRetired = false
                await installProofAdmission(successor, generation: expected, revision: revision)
                guard !closing, expected == generation, revision == hostRevision else {
                    try await successor.close()
                    backend = nil
                    continue
                }
                let discoverySuspended = suspended
                let retained = await readRetainedRecovery(successor)
                guard !closing, suspended == discoverySuspended, replacing,
                      backend === successor, !backendRetired,
                      expected == generation, revision == hostRevision else {
                    try await successor.close()
                    backend = nil
                    continue
                }
                setupState = .attached
                replacing = false
                attachRetainedRecovery(retained, backend: successor, generation: expected, revision: revision)
                enqueue(.policy(.settings(settings)))
                enqueue(.replayVisibleMedia)
                enqueue(.policy(.refresh(.cloud)))
                if let reachable { enqueue(.policy(.network(reachable: reachable))) }
                enqueue(.account(false))
                if observeSignals { observePlatformSignals() }
                await reconcileInitialAdoption(successor)
                return
            }
        } catch {
            #if DEBUG
            DebugCloudDiagnostics.record(.backendReplacement, error: error)
            #endif
            replacing = true; operationFailed = true; setupState = .failed
        }
    }
    private static func requiredDuringClose(_ action: CloudPolicyAction) -> Bool {
        switch action {
        case .stopEngine, .cancelExchange, .recordExchange, .recordQuietPoint, .statusChanged,
             .updatePending, .applyReady, .probeTracking: true
        default: false
        }
    }
    #if DEBUG
    private func observeAccountCompletion(_ read: UInt64, original: any SyncCoordinatorBackend, expected: UInt64,
                                          revision: UInt64, admitted: Bool, followup: Bool = false) {
        observeCausalBoundary?(.accountCompletion(read: read, followup: followup, closing: closing, suspended: suspended,
            replacing: replacing, taskCancelled: accountTask?.isCancelled, admitted: admitted, backendMatches: backend === original,
            generationMatches: expected == generation, revisionMatches: revision == hostRevision,
            pendingMismatch: pendingAccountRead.map { $0 != read } == true), nil)
    }
    #endif
    private func startAccountRead() {
        guard accountTask == nil, !closing, !replacing, !suspended, let backend else { return }
        let control = SyncAccountTaskControl()
        accountTaskControl = control
        accountTask = Task {
            defer {
                accountTask = nil; accountTaskControl = nil
                if pendingAccountRead != nil, !closing { startAccountRead() }
                retryAccountCompletion()
            }
            while let read = pendingAccountRead, !closing, !Task.isCancelled {
                pendingAccountRead = nil
                let expected = generation, revision = hostRevision
                let complete = await backend.prepareAccountRead(read)
                _ = await withCheckedContinuation {
                    enqueue(.completeAccount(read, backend: backend, generation: expected, revision: revision,
                        control: control, complete: complete, acknowledged: $0))
                }
            }
        }
    }
    private func cancelAccountTask(shutdown: Bool) {
        guard let accountTask else { return }
        accountTaskControl?.recordCancellation(shutdown: shutdown, alreadyCancelled: accountTask.isCancelled)
        accountTask.cancel()
    }
    private func reportAvailableSpace() {
        guard let directory, let values = try? directory.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey]),
              let capacity = values.volumeAvailableCapacityForImportantUsage, capacity >= 512 * 1_024 * 1_024 else { return }
        receive(.volumeSpaceAvailable)
    }
    func waitForReplacement() async { await waitForInputs(); await replacementTask?.value; await waitForInputs() }
    func waitForConnectionPreview() async { await ownTask?.value }
    func waitForInitialOwnDrain() async { await initialOwnDrain?.value }
    func waitForInitialOwnResume() async { await initialOwnResume?.value; await ownTask?.value }
    func accountChanged() async { enqueue(.account(true)); await waitForInputs() }
    func prepareConnectionAuthority() async {
        await waitForInputs()
        await accountTask?.value
        await waitForInputs()
    }
    func waitForInputs() async {
        await withCheckedContinuation { enqueue(.barrier($0)) }
    }
    // Internal so real-command fixtures can join execution and result ACKs.
    func drainOperations() async {
        repeat {
            let originals = Array(operationTasks.values)
            for task in originals { await task.value }
            await ownTask?.value
            await waitForInputs()
        } while !operationTasks.isEmpty || ownTask != nil
    }
    private func cancelCommandsForShutdown() {
        for (id, task) in operationTasks {
            // Preserve the first cancellation origin (for example expiry).
            if !task.isCancelled, let context = operationContexts[id],
               context.generation == generation, context.revision == hostRevision {
                context.shutdownCancellationRequested = true
            }
            task.cancel()
        }
    }

    /// Stops native admissions and joins original SDK/own/media callbacks while
    /// Core is OPEN. A failed drain leaves the backend retained for close retry.
    func cancelAndDrain() async -> Bool {
        if let closeTask { return await closeTask.value }
        if drained { return true }
        closing = true
        revokeProofWaits()
        retainedRecoveryStart = nil
        initialOwnAdmission.setOpen(false)
        let task = Task { () -> Bool in
            await attachmentTask?.value
            await replacementTask?.value
            await initialOwnDrain?.value
            await initialOwnResume?.value
            networkTask?.cancel(); accountEventsTask?.cancel(); networkMonitor?.cancel()
            if let accountObserver { NotificationCenter.default.removeObserver(accountObserver); self.accountObserver = nil }
            flushTimer?.invalidate(); flushTimer = nil
            // A core-phase barrier joins original results. Cancel their jobs
            // before joining it, including queued work that has no effect yet.
            cancelCommandsForShutdown()
            cancelAccountTask(shutdown: true); ownTask?.cancel()
            retryAccountCompletion()
            await waitForInputs()
            cancelCommandsForShutdown()
            cancelAccountTask(shutdown: true); ownTask?.cancel()
            retryAccountCompletion()
            do {
                if initialAdoptionEnabledBefore != nil, !activatingInitialAdoption,
                   let own = backend?.own, let operation = try await backend?.connection?.recoveryStatus()?.operation {
                    initialAdoptionCancelled = true
                    try await own.cancel(operation)
                }
                // Cancel/join native transports without retiring the original
                // SDK capability needed by returned control effects.
                try await backend?.quiesce()
                await accountTask?.value
                await networkTask?.value
                await accountEventsTask?.value
                if networkMonitor != nil { observePlatformSignalLifetime?(.drained) }
                await waitForInputs()
                await drainOperations()
                if let backend { await reconcileInitialAdoption(backend) }
                // Reconciliation can issue required finalization too. Keep the
                // original backend until all results have been acknowledged.
                await drainOperations()
                try await backend?.close()
                backendRetired = true
                backend = nil; suspended = false; awaitingResumeProof = false; networkMonitor = nil; status = nil; preview = nil; recovery = nil
                desiredVisibleMedia = nil; deliveredVisibleMedia = nil
                hasDeferredNativeWork = false
                hostRevision &+= 1; drained = true; platformPrepared = false; retainedHostAdmitted = false
                return true
            } catch {
                #if DEBUG
                DebugCloudDiagnostics.record(.closeDrain, error: error)
                #endif
                operationFailed = true; return false
            }
        }
        closeTask = task
        let result = await task.value
        closeTask = nil
        return result
    }

    private func observePlatformSignals() {
        guard networkMonitor == nil else { return }
        let monitor = NWPathMonitor()
        let (paths, sink) = AsyncStream<Bool>.makeStream(bufferingPolicy: .unbounded)
        monitor.pathUpdateHandler = { sink.yield($0.status == .satisfied) }
        networkMonitor = monitor
        monitor.start(queue: DispatchQueue(label: "Meguri.cloud.reachability"))
        networkTask = Task { [weak self] in
            var wasReachable = false
            for await reachable in paths {
                guard let self, !Task.isCancelled else { return }
                receive(.network(reachable: reachable))
                if reachable && !wasReachable { enqueue(.account(false)) }
                wasReachable = reachable
            }
        }
        let (accounts, changes) = AsyncStream<Void>.makeStream(bufferingPolicy: .unbounded)
        #if canImport(CloudKit)
        accountObserver = NotificationCenter.default.addObserver(forName: .CKAccountChanged, object: nil, queue: nil) { _ in changes.yield(()) }
        #endif
        accountEventsTask = Task { [weak self] in
            for await _ in accounts {
                guard let self, !Task.isCancelled else { return }
                await accountChanged()
            }
        }
        observePlatformSignalLifetime?(.started)
    }
}
