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
    var providerService: CoreProviderService? { get }
    func stopSyncing() async throws
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
    var providerService: CoreProviderService? { nil }
    func stopSyncing() async throws { throw BridgeError.cloudConnection(.notOffered) }
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
    let dependentFetchRefusal: CloudPolicyDependentFetchRefusal?
    let fetchProofOutcome: CloudPolicyFetchProofOutcome?
    let nativeCancellation: CloudPolicyNativeCancellation?
    let accountReadRefusal: SyncAccountReadRefusal?
    let deferred: Bool
    let accountVerified: Bool
    let mediaCancellation: Bool
    let acknowledgedOutputCount: Int
    // Account preparation returns before any subsequent Core preparation/input.
    let afterOutputs: (@MainActor @Sendable (Bool) async -> SyncCommandResult)?
    init(outputs: [CloudPolicyOutput], error: (any Error)?, deferred: Bool, accountVerified: Bool = false, mediaCancellation: Bool = false, acknowledgedOutputCount: Int = 0,
         afterOutputs: (@MainActor @Sendable (Bool) async -> SyncCommandResult)? = nil, originalGuardRefusal: CloudPolicyOriginalGuardRefusal? = nil,
         accountReadRefusal: SyncAccountReadRefusal? = nil, dependentFetchRefusal: CloudPolicyDependentFetchRefusal? = nil, fetchProofOutcome: CloudPolicyFetchProofOutcome? = nil, nativeCancellation: CloudPolicyNativeCancellation? = nil) {
        self.outputs = outputs; self.error = error; self.deferred = deferred; self.accountVerified = accountVerified
        self.mediaCancellation = mediaCancellation; self.originalGuardRefusal = originalGuardRefusal
        self.accountReadRefusal = accountReadRefusal; self.dependentFetchRefusal = dependentFetchRefusal
        self.fetchProofOutcome = fetchProofOutcome; self.nativeCancellation = nativeCancellation
        self.acknowledgedOutputCount = acknowledgedOutputCount; self.afterOutputs = afterOutputs
    }
}
enum SyncOwnerFailure: Error { case configurationUnavailable }
enum MeguriSignInLocalFailure: Error { case busy, attachment, credentialStore }
/// T19.3's `sync.setup.done.*` lines for Meguri Sync.
public enum MeguriSetupDone: Sendable, Equatable { case uploaded, downloaded(backup: Bool) }

// Observation and a single replay budget belong to the original wake.
// Recording either release signal creates no task and grants no admission.
@MainActor final class MeguriWakeAvailability {
    private let generation: UInt64
    private(set) var revision: UInt64 = 0
    private var replayed = false

    init(generation: UInt64) { self.generation = generation }

    func observe(kind: CoreEvent.Kind, sequence: UInt64, generation: UInt64) {
        guard generation == self.generation else { return }
        switch kind {
        case .available, .sessionEnded: break
        default: return
        }
        revision = max(revision, sequence)
    }

    func consume(collectionOutcome: MeguriRoundPhaseOutcome, since snapshot: UInt64, allowed: Bool) -> Bool {
        guard allowed, !Task.isCancelled, !replayed, revision > snapshot,
              case .refused(.busy) = collectionOutcome else { return false }
        replayed = true
        return true
    }
}

// Retained by the exact original command task. Intent is recorded before
// cancellation, and never upgraded by a later close or replacement.
@MainActor final class SyncCommandTaskContext {
    @TaskLocal nonisolated static var current: SyncCommandTaskContext?
    enum Cancellation { case expiry, replacement, shutdown, other }
    let generation: UInt64
    let revision: UInt64
    private let backend: any SyncCoordinatorBackend
    private let originalCommands: [CloudPolicyCommand]
    var activeCommandIndex: Int?
    private var activeCommand: CloudPolicyCommand? {
        guard let index = activeCommandIndex, originalCommands.indices.contains(index) else { return nil }
        return originalCommands[index]
    }
    private var firstCancellation: Cancellation?
    let cancellationIntent: CloudPolicyCancellationContext
    private var cancelledCommands: [CloudPolicyCommand] = []
    private(set) var shutdownCancellationRequested = false
    var shutdownCancelled: Bool { shutdownCancellationRequested && Task.isCancelled }
    init(generation: UInt64, revision: UInt64, backend: any SyncCoordinatorBackend,
         originalCommands: [CloudPolicyCommand], cancellationIntent: CloudPolicyCancellationContext = CloudPolicyCancellationContext()) {
        self.generation = generation; self.revision = revision; self.backend = backend
        self.originalCommands = originalCommands
        self.cancellationIntent = cancellationIntent
    }
    func recordCancellation(_ intent: Cancellation, alreadyCancelled: Bool, event: UUID = UUID()) {
        guard firstCancellation == nil else { return }
        firstCancellation = alreadyCancelled ? .other : intent
        let request: CloudPolicyCancellationContext.Trigger
        switch firstCancellation {
        case .expiry: request = .expiry
        case .replacement: request = .replacement
        case .shutdown: request = .shutdown
        default: request = .other
        }
        cancellationIntent.record(request, sequence: event)
        if let index = activeCommandIndex {
            // An invalid position cannot identify a remaining original queue.
            if originalCommands.indices.contains(index) { cancelledCommands = Array(originalCommands[index...]) }
        } else { cancelledCommands = originalCommands }
        shutdownCancellationRequested = firstCancellation == .shutdown
    }
    func ownsNativeCancellation(_ result: SyncCommandResult, command: CloudPolicyCommand,
                                backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64) -> Bool {
        guard self.backend === backend, self.generation == generation, self.revision == revision,
              result.error is CancellationError, result.fetchProofOutcome?.preservesFailure != true,
              let proof = result.nativeCancellation, let activeCommand,
              proof.matches(activeCommand), proof.matches(command) else { return false }
        if firstCancellation == .expiry || firstCancellation == .replacement {
            return cancelledCommands.contains(where: { proof.matches($0) }) &&
                (proof.matchesIntent(cancellationIntent) || proof.ownsDeadline(cancellationIntent))
        }
        // The executor's own P11 deadline has no outer cancellation request.
        // Its immutable native cause still belongs to this exact task/command.
        return firstCancellation == nil && proof.ownsDeadline(cancellationIntent) &&
            originalCommands.contains(where: { proof.matches($0) })
    }
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
    /// P25.1: `media` is true in every value this coordinator holds or sends.
    public private(set) var settings: CloudPolicyDeviceSettings {
        didSet {
            if !settings.media {
                settings = .init(enabled: settings.enabled, paused: settings.paused, automatic: settings.automatic, media: true)
            }
        }
    }
    public private(set) var generation: UInt64 = 0 {
        didSet { if generation != oldValue { meguriLastExchange = nil } }
    }
    #if DEBUG
    /// Content-free observation of owner-valid, complete collection/media rounds.
    public private(set) var debugMeguriRoundSuccessSequence: UInt64 = 0
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
    /// The retained op could not be read at this attachment: it is not proved absent.
    public private(set) var recoveryReadFailed = false
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
        case ownAccountProof(UInt64, generation: UInt64, revision: UInt64, verified: Bool)
        case result(SyncCommandResult, generation: UInt64, revision: UInt64, shutdownCancelled: Bool,
                    supersededAccountCancellation: Bool = false, nativeCancellationOwned: Bool = false, acknowledged: CheckedContinuation<Void, Never>?)
        case completeAccount(UInt64, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                             control: SyncAccountTaskControl,
                             complete: @MainActor (Bool) async -> SyncCommandResult,
                             acknowledged: CheckedContinuation<Bool, Never>)
        case accountOutputsReturned(UInt64, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                                    result: SyncCommandResult, acknowledged: CheckedContinuation<Bool, Never>)
        case discoveryPrepared(preparation: ResumePreparation, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                               acknowledged: CheckedContinuation<Bool, any Error>)
        case resumeBinding(CloudConnectionPreview, preparation: ResumePreparation, service: CoreCloudConnectionService, generation: UInt64, revision: UInt64,
                           acknowledged: CheckedContinuation<ResumeAdmission, any Error>)
        case localOnly(stopOnly: Bool, preview: CloudConnectionPreview?, previewOwner: UUID?, preparation: ResumePreparation,
                       backend: any SyncCoordinatorBackend, service: CoreCloudConnectionService, generation: UInt64, revision: UInt64,
                       acknowledged: CheckedContinuation<LocalOnlyAdmission, any Error>)
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
    // A tracked own task has not entered native work yet: explicit discovery
    // joins its first account read, or a guard-changing action joins policy work.
    // The input lane revokes preparation before either enters Core/native work.
    private final class ResumePreparation { let id = UUID() }
    private enum ResumeAdmission { case pending, cancelled, resumed(CloudOriginalCommandProofYield) }
    private enum LocalOnlyAdmission { case pending, cancelled, accepted }
    private enum ProofOwner: Equatable {
        case command(UUID), own(UUID)
        var commandID: UUID? { if case .command(let id) = self { id } else { nil } }
    }
    enum CausalBoundary {
        case commandResultEnqueue, commandAcknowledgementJoined
        case proofResultReduced, proofAcknowledgementSending
        case accountCompletion(read: UInt64, followup: Bool, closing: Bool, suspended: Bool, replacing: Bool,
                               taskCancelled: Bool?, admitted: Bool, backendMatches: Bool, generationMatches: Bool,
                               revisionMatches: Bool, pendingMismatch: Bool)
        case accountAccepted(read: UInt64, verified: Bool, succeeded: Bool)
        case resultReduced(command: UInt64?, read: UInt64?, cancellation: Bool, shutdownOwned: Bool, before: UInt64, after: UInt64)
        case resumeEntered, resumeReturned, resumeRefused(authorityChanged: Bool), resumeCancelled
        #if DEBUG
        case localOnlyPolicyWaiting
        #endif
        case ownCoreStarted
        case ownDrainWaiting(UUID), ownDrainJoined(UUID)
        case dependencyRevocationJoined(UUID)
        case corePredecessorWaiting(CloudPolicyCommand), corePredecessorReleased(CloudPolicyCommand)
    }
    #if DEBUG
    @ObservationIgnored var observeQuiescentDependency: (@MainActor (Bool) -> Void)?
    #endif
    @ObservationIgnored var observeCausalBoundary: (@MainActor (CausalBoundary, CloudPolicyCommand?) -> Void)?
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
    @ObservationIgnored private let recordDiagnostic: FieldDiagnosticRecorder
    @ObservationIgnored private var diagnosticStatus = FieldDiagnosticCloudStatus()
    @ObservationIgnored private let directory: URL?
    @ObservationIgnored private let observeSignals: Bool
    private var platformPrepared = false
    // Successful pre-open setup belongs to the retained Host, even after a
    // provider-only drain retires its backend. Ordinary close/replacement
    // revokes this evidence; only a committed successor may reuse it.
    private var providerPlatform: (generation: UInt64, owner: CoreProviderService)?
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
    private var cancellingConnectionOperation = false
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
    // Retain the last actual task's identity after finish clears active work.
    // A resumed action must also detect newer work that has already finished.
    private var latestOwnOwner: UUID?
    private var ownResumePreparation: (owner: UUID, token: ResumePreparation)?
    private var heldAccountCompletion: Input?
    private var readyPreviewOwner: (service: CoreCloudConnectionService, generation: UInt64, revision: UInt64, id: UUID)?
    private var readingPreview = false
    // Actual tracked account-read output for the current pre-work discovery.
    // Native proof capabilities remain exclusively Core-issued.
    private var discoveryAccountRead: (read: UInt64?, result: SyncCommandResult?)?
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
    private var entryActivationHeld = false
    private var drained = false
    private var closeTask: Task<Bool, Never>?
    private var attachmentTask: Task<Void, Never>?
    private var attachmentFailure: (any Error)?
    private var hostRevision: UInt64 = 0 {
        didSet { if hostRevision != oldValue { meguriLastExchange = nil } }
    }
    private(set) var failureRevision: UInt64 = 0
    private var commandSequence: UInt64 = 0
    private var timerSequence: UInt64 = 0
    private var statusSequence: UInt64 = 0
    private static let settingsKey = "meguri.cloud.deviceSettings"
    // sync/policy P25. The device's own Sync Automatically value, kept apart
    // from the selected service's committed settings so it survives a change.
    private static let automaticKey = "meguri.sync.automatic"
    private static let round2Key = "meguri.sync.round2Migrated"
    /// Rendering fixtures only: no binding stands behind this value.
    private var renderConnected: Bool?
    private var renderChoice = false
    private var renderRetryMedia = false
    /// P25.3: on with no stored value. Written only when the person changes
    /// the switch or P25.2's migration writes off.
    public var storedAutomatic: Bool { (defaults?.object(forKey: Self.automaticKey) as? Bool) ?? memoryAutomatic }
    private var memoryAutomatic = true
    fileprivate func applyRenderFixture(_ state: RenderState) {
        let recent = Int64(clock.now().timeIntervalSince1970 * 1_000) - 240_000
        setupState = .attached
        switch state {
        case .off:
            providerState = .renderFixture(selection: .localOnly, phase: .localOnly)
            status = .renderFixture(state: .localOnly, syncNow: .unavailable(.notConnected))
        case .meguriConnected, .meguriNeedsYou:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .setupRequired; meguriEndpoint = "https://sync.example.test/"; meguriUsername = "study-account"
            meguriRoutine = state == .meguriConnected ? .idle : .fullSyncRequired
            meguriLastExchange = recent
            // A collection choice is not a ready binding (P25.3); its one action shows (N15.2).
            renderConnected = state == .meguriConnected
            renderChoice = state == .meguriNeedsYou
        case .meguriSignIn:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .signedOut; meguriEndpoint = "https://sync.example.test/"
            renderConnected = false
        case .iCloudSelected:
            providerState = .renderFixture(selection: .iCloud, phase: .iCloud)
            status = .renderFixture(state: .localOnly, syncNow: .unavailable(.notConnected), controls: .init(enable: .connect))
            renderConnected = false
        case .iCloudConnected:
            providerState = .renderFixture(selection: .iCloud, phase: .iCloud)
            settings = .init(enabled: true, paused: false, automatic: true, media: true)
            status = .renderFixture(state: .upToDate(asOfMilliseconds: recent),
                controls: .init(bindingUsable: true, bindingActive: true, automaticEligible: true))
        case .iCloudNeedsYou:
            providerState = .renderFixture(selection: .iCloud, phase: .iCloud)
            settings = .init(enabled: true, paused: false, automatic: true, media: true)
            status = .renderFixture(state: .needsAttention, stateWithoutAttention: .changesPending,
                attention: [.quotaFull: 1, .oversize: 2], pending: 12,
                controls: .init(bindingUsable: true, bindingActive: true, automaticEligible: true))
        case .iCloudIssues:
            providerState = .renderFixture(selection: .iCloud, phase: .iCloud)
            settings = .init(enabled: true, paused: false, automatic: true, media: true)
            status = .renderFixture(state: .needsAttention, stateWithoutAttention: .upToDate(asOfMilliseconds: recent),
                attention: [.oversize: 2, .refused: 1, .conflicts: 3],
                controls: .init(bindingUsable: true, bindingActive: true, automaticEligible: true))
        case .iCloudSyncing:
            providerState = .renderFixture(selection: .iCloud, phase: .iCloud)
            settings = .init(enabled: true, paused: false, automatic: true, media: true)
            status = .renderFixture(state: .syncingCollection,
                controls: .init(bindingUsable: true, bindingActive: true, automaticEligible: true))
        case .iCloudSetupNeeded:
            providerState = .renderFixture(selection: .iCloud, phase: .iCloud)
            settings = .init(enabled: true, paused: false, automatic: true, media: true)
            status = .renderFixture(state: .iCloudUnavailable(.setupNeeded), accountStatus: .configuration,
                syncNow: .unavailable(.iCloudUnavailable))
        case .meguriSyncing, .meguriRoundFailed, .meguriSignInRefused, .meguriMediaBlocked, .meguriAcceptanceUnknown:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .setupRequired; meguriEndpoint = "https://sync.example.test/"; meguriUsername = "study-account"
            meguriRoutine = switch state {
            case .meguriSyncing: .exchanging
            case .meguriRoundFailed: .network
            case .meguriMediaBlocked: .mediaBlocked
            case .meguriAcceptanceUnknown: .acceptanceUnknown
            default: .authentication
            }
            meguriLastExchange = recent
            renderConnected = true
        case .meguriSetupMedia:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .setupRequired; meguriEndpoint = "https://sync.example.test/"; meguriUsername = "study-account"
            meguriStage = .media; meguriBusy = true
            meguriMediaProgress = .renderFixture(downloadedFiles: 18_945, awaiting: 25_055)
            renderConnected = false
        case .meguriSetupMediaPaused, .meguriSetupMediaStopped:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .setupRequired; meguriEndpoint = "https://sync.example.test/"; meguriUsername = "study-account"
            meguriStage = state == .meguriSetupMediaPaused ? .mediaPaused : .mediaStopped
            meguriMediaProgress = .renderFixture(downloadedFiles: 39_569, awaiting: 4_431)
            renderConnected = false; renderRetryMedia = true
        case .meguriPreparing:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .setupRequired; meguriEndpoint = "https://sync.example.test/"; meguriUsername = "study-account"
            meguriStage = .preparing; meguriBusy = true
            renderConnected = false
        case .iCloudNoAccount:
            providerState = .renderFixture(selection: .iCloud, phase: .iCloud)
            settings = .init(enabled: true, paused: false, automatic: true, media: true)
            status = .renderFixture(state: .iCloudUnavailable(.noAccount), accountStatus: .noAccount,
                syncNow: .unavailable(.iCloudUnavailable))
        case .meguriBlocked:
            providerState = .renderFixture(selection: .meguri, phase: .blocked)
            meguriPhase = .setupRequired; meguriEndpoint = "https://sync.example.test/"; meguriUsername = "study-account"
            renderConnected = false
        }
    }
    /// P25.3 "connected": the selected service has a ready binding.
    public var serviceConnected: Bool {
        if let renderConnected { return renderConnected }
        switch providerState?.selection {
        case .meguri: return meguriAutomaticSettingVisible
        case .iCloud: return status?.controls.bindingUsable == true
        case .localOnly, nil: return false
        }
    }
    /// What the Sync Automatically switch shows: the committed value once
    /// connected, the stored value while it is disabled before that.
    public var automaticSwitchValue: Bool { serviceConnected ? settings.automatic && storedAutomatic : storedAutomatic }
    public var automaticSwitchAvailable: Bool {
        if renderConnected == true { return true }
        guard serviceConnected, !closing, !replacing, !providerActionInProgress, !deviceSettingsSettling else { return false }
        return providerState?.selection != .meguri || meguriAutomaticSettingAvailable || settings.paused
    }
    public func setAutomatic(_ on: Bool) {
        guard automaticSwitchAvailable else { return }
        rememberAutomatic(on)
        // Turning it on is a clearing act of the update hold (P25.2).
        setDeviceSettings(paused: on ? false : settings.paused, automatic: on)
    }
    private func rememberAutomatic(_ on: Bool) {
        memoryAutomatic = on; memoryAutomaticStored = true
        defaults?.set(on, forKey: Self.automaticKey)
    }
    private var memoryAutomaticStored = false
    private var automaticIsStored: Bool {
        defaults.map { $0.object(forKey: Self.automaticKey) != nil } ?? memoryAutomaticStored
    }
    /// An earlier build's stored values still wait for P25.2's migration.
    private var round2MigrationPending: Bool {
        guard let defaults else { return false }
        return !defaults.bool(forKey: Self.round2Key)
    }
    /// P25.2: a stored pause is only the update hold; no control sets it.
    public var updateHold: Bool { settings.paused || desiredProviderSettings?.paused == true }
    /// Every clearing act writes `paused: false` before its request goes on.
    /// Auto stays as stored: a pause that the migration has not turned into
    /// Sync Automatically off yet (its write failed, or this act came first)
    /// stores off here, before the write, so it can never read on afterwards.
    @discardableResult public func clearUpdateHold() async -> Bool {
        if let pending = providerSettingsTask { await pending.value }
        guard settings.paused else { return true }
        if round2MigrationPending { rememberAutomatic(false) }
        setDeviceSettings(paused: false, automatic: settings.automatic && storedAutomatic)
        if let write = providerSettingsTask { await write.value }
        return !settings.paused
    }
    /// P25.1, P25.2: runs at each attachment until it has succeeded once, before
    /// that attachment's first DeviceSettings input. One write turns a stored
    /// pause into Sync Automatically off; the pause stays, as the update hold,
    /// only with a `publish` or `adopt` op. The device's own value is stored
    /// before the service's settings are written, so a failed or interrupted
    /// write never leaves Sync Automatically reading on.
    private func migrateRound2Settings(_ retained: Result<CloudRecoveryStatus?, any Error>) async {
        if let pending = providerSettingsTask { await pending.value }
        guard let defaults, round2MigrationPending else { return }
        let operation = retainedOperationForMigration?() ?? retained.map { $0 != nil }
        var next = settings
        // P25.3: an off the person chose under the earlier build belongs to the
        // device, so it survives a change of service.
        if defaults.object(forKey: Self.automaticKey) == nil,
           next.paused || (next.enabled && !next.automatic && providerState?.selection != .localOnly) {
            rememberAutomatic(false)
        }
        if next.paused {
            next = .init(enabled: next.enabled, paused: Self.keepsUpdateHold(operation: operation, selection: providerState?.selection),
                automatic: false, media: true)
        }
        if next != settings || !storedSettingsAreCurrent(next) {
            guard await commitMigratedSettings(next) else { return }
        }
        defaults.set(true, forKey: Self.round2Key)
    }
    /// P25.2: the stored pause stays, as the update hold, only with a `publish`
    /// op at any step or an `adopt` op. An op that cannot be read is not proved
    /// absent, so nothing opens; Meguri Sync has no such op.
    static func keepsUpdateHold(operation: Result<Bool, any Error>, selection: SyncProviderSelection?) -> Bool {
        switch operation {
        case .success(let present): present
        case .failure: selection != .meguri
        }
    }
    /// Tests only: whether a retained op exists, where no connection service can report one.
    @ObservationIgnored var retainedOperationForMigration: (@MainActor () -> Result<Bool, any Error>)?
    private func storedSettingsAreCurrent(_ value: CloudPolicyDeviceSettings) -> Bool {
        guard let stored = defaults?.dictionary(forKey: Self.settingsKey) as? [String: Bool] else { return true }
        return stored == ["enabled": value.enabled, "paused": value.paused, "automatic": value.automatic, "media": true]
    }
    /// The migration's one write, in order with every other settings write.
    private func commitMigratedSettings(_ next: CloudPolicyDeviceSettings) async -> Bool {
        let previous = providerSettingsTask
        let provider = provider, attachment = hostRevision
        var committed = false
        let write = Task {
            await previous?.value
            var value = next
            if let provider {
                guard let state = try? await provider.saveSettings(next), let saved = state.settings,
                      self.provider === provider, attachment == hostRevision else { return }
                providerState = state
                value = saved
            }
            settings = value
            defaults?.set(["enabled": settings.enabled, "paused": settings.paused,
                "automatic": settings.automatic, "media": true], forKey: Self.settingsKey)
            committed = true
        }
        providerSettingsTask = write
        await write.value
        return committed
    }
    private var providerDirectory: URL?
    private var provider: CoreProviderService?
    public private(set) var providerState: SyncProviderState? {
        didSet {
            updateProviderBlockedCause()
            if MeguriSyncStatusPresentation.stopsWorkOnBlock(was: oldValue?.phase, now: providerState?.phase,
                stage: meguriStage, roundRunning: meguriWake != nil) { stopMeguriWorkForBlockedProvider() }
            // A setup that a block stopped with no choice offered (its readiness
            // read failed) reads Core again once the phase recovers, so that
            // Try Again or the sheet returns. A failed read never meant "gone".
            if oldValue?.phase == .blocked, providerState?.phase != .blocked {
                meguriRecoveryReloadPending = true
                Task { self.reloadMeguriAfterRecoveryIfNeeded(); self.resumeMeguriMediaIfNeeded(.recovered) }
            }
        }
    }
    /// Kept until a reload can be issued: a setup action still running when the
    /// phase recovers rechecks it when it ends (`issueMeguri`).
    @ObservationIgnored private var meguriRecoveryReloadPending = false
    private func reloadMeguriAfterRecoveryIfNeeded() {
        guard meguriRecoveryReloadPending else { return }
        guard let state = providerState, state.phase != .blocked, state.phase != .handoffRecovery,
              state.selection == .meguri, meguriStage == .failed, !meguriChoiceAllowed else {
            // Recovered without a stuck setup, or blocked again: nothing to reload.
            if providerState?.phase == .blocked || !meguriBusy { meguriRecoveryReloadPending = false }
            return
        }
        guard !meguriBusy else { return }
        meguriRecoveryReloadPending = false
        loadMeguriConnection()
    }
    public var providerNeedsAttention: Bool { SyncStatusPresentation.providerNeedsAttention(providerState) }
    /// Copy Debug Info's names for why the phase is blocked; nil while it is not.
    public private(set) var providerBlockedCause: (source: ProviderBlockedSource, reason: ProviderFailureReason?)?
    @ObservationIgnored private var reportedBlockedCause: (source: ProviderBlockedSource, reason: ProviderFailureReason?)?
    /// A failed provider call that this coordinator reports as blocked.
    private func markProviderBlocked(_ source: ProviderBlockedSource, _ error: any Error) {
        reportedBlockedCause = (source, ProviderFailureReason(error))
        providerState = .blocked
        reportedBlockedCause = nil
    }
    private func updateProviderBlockedCause() {
        guard let state = providerState, state.phase == .blocked else { providerBlockedCause = nil; return }
        // Core's own snapshot: no selection means its metadata could not be read.
        providerBlockedCause = reportedBlockedCause ?? (state.selection == nil ? .metadataUnreadable : .admissionClosed, nil)
    }
    /// Content-free facts for Copy Debug Info.
    public var debugFacts: SyncDebugFacts {
        let service = switch providerState?.selection {
        case .meguri: "meguri"
        case .iCloud: "iCloud"
        case .localOnly: "off"
        case nil: "unread"
        }
        let status = mainStatus
        let connection: MeguriProviderConnectionPhase = meguriAuthenticating ? .authenticating : meguriPhase
        return SyncDebugFacts(service: service, providerPhase: providerState.map { "\($0.phase)" } ?? "unread",
            blockedSource: providerBlockedCause?.source, blockedReason: providerBlockedCause?.reason,
            meguriConnection: providerState?.selection == .meguri ? "\(connection)" : nil,
            meguriSetupStage: meguriStage.map { "\($0)" }, meguriRoutine: meguriRoutine.map { "\($0)" },
            statusTitleKey: status.titleKey, statusDetailKey: status.detailKey)
    }
    /// MP6.5: blocked metadata stops network work, including the work already
    /// running. Nothing is discarded: a stopped media step or round keeps its
    /// pending work, as a failed or cancelled round does (MP5), and a new start
    /// is refused while blocked.
    private func stopMeguriWorkForBlockedProvider() {
        let stage = meguriStage, facade = meguriActiveFacade, wake = meguriWake
        meguriWakePending = false; meguriWakeManual = false; meguriReadyWakePending = false
        wake?.task.cancel()
        // The media step may run on a coordinator adopted after activation, which
        // has no action of its own: the facade and the media stage identify it.
        // Stop only while that step is still the one running.
        if stage == .media, let facade {
            Task { guard self.meguriActiveFacade === facade, self.meguriStage == .media else { return }; await facade.stop(.system) }
        }
    }
    /// The Account section's one progress value while Meguri work runs (MP6.3).
    public var meguriProgressValue: MeguriProgressValue? {
        let media = meguriMediaProgress
        return MeguriSyncStatusPresentation.progress(busy: meguriBusy, stopped: providerNeedsAttention, stage: meguriStage,
            completed: meguriProgress?.completed, total: meguriProgress?.total,
            mediaDone: media.map { $0.uploadedFiles.addingReportingOverflow($0.downloadedFiles) }
                .flatMap { $0.overflow ? nil : $0.partialValue },
            mediaAwaiting: media?.awaiting)
    }
    /// The setup media step's files left to download: the running step's count,
    /// else the one the core last recorded for the operation; nil when none is
    /// recorded. Never a count Meguri works out itself.
    public var meguriSetupMediaLeft: UInt64? {
        switch meguriStage {
        case .media?, .mediaPaused?, .mediaStopped?: break
        default: return nil
        }
        let left = meguriMediaProgress?.awaiting ?? meguriReceipt?.media.awaiting ?? 0
        return left > 0 ? left : nil
    }
    /// The setup's media files left, while its media step runs and nothing stopped it.
    public var meguriMediaFilesLeft: UInt64? {
        guard meguriStage == .media, !providerNeedsAttention else { return nil }
        return meguriMediaProgress?.awaiting
    }
    /// N23.1: the status panel's one bar, from the same status as the Sync section.
    public var panelProgress: SyncPanel.Progress? {
        let status = self.status
        let shown = status?.state == .needsAttention ? status?.stateWithoutAttention : status?.state
        return SyncPanel.progress(mainStatus, meguri: providerState?.selection == .meguri || providerNeedsAttention,
            setupValue: meguriProgressValue,
            meguriMoving: meguriAuthenticating || (meguriStage == nil && meguriRoutine == .exchanging), cloudState: shown)
    }
    /// N23.1: the files line under the bar, while media transfers and a count is known.
    public var panelFilesLeft: UInt64? {
        guard panelProgress != nil else { return nil }
        if providerState?.selection == .meguri { return meguriMediaFilesLeft }
        let shown = status?.state == .needsAttention ? status?.stateWithoutAttention : status?.state
        guard shown == .syncingMedia, let media = status?.media,
              let upload = media.uploadFiles, let download = media.downloadFiles else { return nil }
        let sum = upload.addingReportingOverflow(download)
        return sum.overflow ? nil : sum.partialValue
    }
    /// N23.3: a service is selected, so Sync Now has a place in the panel.
    public var panelServiceSelected: Bool {
        switch providerState?.selection {
        case .meguri, .iCloud: true
        case .localOnly, nil: false
        }
    }
    /// N23.2: draw the `icloud` symbols, which refer to Apple's iCloud only.
    public var symbolsAreICloud: Bool { providerState?.selection == .iCloud && !providerNeedsAttention }
    /// Sync waits on the person, so review shows its status button (N16.2).
    public var needsUser: Bool {
        if providerNeedsAttention { return true }
        return switch providerState?.selection {
        case .meguri: meguriStatus.needsUser
        case .iCloud: SyncStatusPresentation.needsUser(mainStatus)
        case .localOnly, nil: false
        }
    }
    /// The facts this projection carries for T19.4's rows. An isolation
    /// reason or an op kind it does not carry stays at the row that claims least.
    public var cloudKindFacts: CloudKindFacts {
        var facts = CloudKindFacts()
        facts.resumeBindingOffered = status?.controls.resumeBindingOffered == true
        if let recovery {
            if recovery.adoptionRecoveredCancelOnly { facts.recoveryOp = .adoptUnproven }
            else if recovery.adoptionAfterReview { facts.recoveryOp = .adoptAfterReview }
            else if recovery.switchConflict { facts.recoveryOp = .publishConflict }
            else {
                switch recovery.step {
                case .staging, .staged, .ready: facts.recoveryOp = recovery.adoptionTransportWait ? .adoptWaiting : nil
                default: facts.recoveryOp = .publishWaiting
                }
            }
            facts.resumeOpOffered = SyncStatusPresentation.showsRecoveryResume(recovery)
            switch recovery.step {
            case .bound, .zones, .sent: facts.publishQuota = true
            default: break
            }
        }
        if status?.attention[.isolated] != nil, status?.accountStatus == .available, status?.accountMatchesBinding == false {
            facts.isolation = .accountChanged
        }
        return facts
    }
    /// T19.4's and T19.5's main status for the selected service.
    public var mainStatus: SyncMainStatus {
        if providerState?.selection == .meguri || providerNeedsAttention {
            return SyncKinds.meguri(meguriKindFacts)
        }
        return SyncKinds.cloud(state: status?.state, stateWithoutAttention: status?.stateWithoutAttention,
            attention: status?.attention ?? [:], facts: cloudKindFacts, setup: setupState,
            selectedNotSetUp: providerState?.selection == .iCloud && status?.controls.enable == .connect)
    }
    public var meguriKindFacts: MeguriKindFacts {
        .init(handoffRecovery: providerState?.phase == .handoffRecovery, providerBlocked: providerState?.phase == .blocked,
            connection: meguriAuthenticating ? .authenticating : meguriPhase, stage: meguriStage, routine: meguriRoutine,
            failureKey: meguriFailureKey, finishOffered: meguriCanFinish, automatic: settings.automatic && storedAutomatic,
            mediaFilesLeft: meguriSetupMediaLeft)
    }
    public var providerHandoffInProgress: Bool { providerHandoff != nil || providerState?.phase == .handoffRecovery }
    private var providerDiscovery: ProviderDiscoveryOwnership?
    private var providerHandoff: ProviderHandoffOwnership?
    private var providerHandoffDestination: SyncProviderSelection?
    private var providerSettingsTask: Task<Void, Never>?
    private var providerSettingsRevision: UInt64 = 0
    private var desiredProviderSettings: CloudPolicyDeviceSettings?
    private var providerActionInProgress = false
    // Issued only by explicit recovery on this attachment. Ordinary close or
    // replacement revokes it, including while a current-owner drain is held.
    private var providerRecoveryDrain: UUID?
    var providerChangeAllowed: @MainActor () -> Bool = { true }
    var drainManualProviderOwner: (@MainActor () async -> Void)?
    var attachProviderSuccessor: (@MainActor (SyncCoordinator) async throws -> Void)?
    var retryProviderAttachment: (@MainActor (SyncCoordinator) async throws -> Void)?
    private var providerAttachmentPending = false
    private var retryingProviderAttachment = false
    private let meguriCredentialStore: (any MeguriProviderCredentialStore)?
    private var meguriConnectionOwner: MeguriProviderConnectionOwner? {
        didSet { if meguriConnectionOwner !== oldValue { meguriLastExchange = nil } }
    }
    public private(set) var meguriPhase: MeguriProviderConnectionPhase = .unconfigured
    public private(set) var meguriEndpoint = ""
    /// The signed-in account's name (N15.2 row 2); nil while signed out.
    public private(set) var meguriUsername: String?
    public private(set) var meguriStage: MeguriSetupStage?
    public private(set) var meguriPreview: MeguriBootstrapPreview?
    /// MP6.3 row 4: the decision the keep-collection sheet shows. Its bound preview stays private.
    public private(set) var meguriSetupDecision: MeguriSetupDecision?
    private var meguriSetupPreview: MeguriBootstrapPreview?
    /// T19.3: shown in the sheet after changed facts reloaded it.
    public private(set) var meguriSetupNoticeKey: String?
    /// T19.3: what setup did when it asked nothing, until Meguri is next launched.
    public private(set) var meguriSetupDone: MeguriSetupDone?
    private var meguriSetupDonePending: MeguriSetupDone?
    private var meguriSetupKeepsService = false
    private var meguriFinishAfterSettle = false
    public private(set) var meguriReceipt: MeguriBootstrapReceipt?
    public private(set) var meguriAccounting: MeguriBootstrapMediaAccounting?
    public private(set) var meguriProgress: CoreProgress?
    public private(set) var meguriMediaProgress: MeguriBootstrapMediaProgress?
    public private(set) var meguriFailureKey: String?
    public private(set) var meguriSignInErrorKey: String?
    public private(set) var meguriRoutine: MeguriRoutineState?
    /// Device-local observation for this attachment, not cross-device convergence.
    public private(set) var meguriLastExchange: Int64?
    public private(set) var meguriRoundReport: MeguriRoundReport?
    public private(set) var meguriLastCommand: MeguriBootstrapCommandResult?
    public private(set) var meguriCollectionCommand: MeguriBootstrapCommandResult?
    public private(set) var meguriRecoveryPreparation: MeguriBootstrapCommandResult?
    public private(set) var meguriCollectionError: (any Error)?
    public private(set) var meguriLastError: (any Error)?
    public private(set) var meguriProgressFailure: BridgeError?
    public private(set) var meguriBusy = false
    public private(set) var meguriCancelling = false
    var meguriCollectionWorkHeld: Bool {
        meguriPreview != nil || meguriSetupPreview != nil || meguriStage == .preparing || meguriStage == .collection
    }
    public var serviceChangeAvailable: Bool {
        providerChangeAllowed() && !providerActionInProgress && !meguriBusy && !suspended && !replacing
    }
    public var meguriCanChoose: Bool {
        if renderChoice { return true }
        return meguriChoiceAllowed && (meguriStage == .required || meguriStage == .failed) && !meguriBusy && !settings.paused
    }
    public var meguriCanFinish: Bool {
        !meguriBusy && !settings.paused && meguriOperation.map { MeguriSyncStatusPresentation.canRecover($0.receipt) } == true
    }
    public var meguriCanRetryMedia: Bool {
        if renderRetryMedia { return true }
        return !meguriBusy && !settings.paused && meguriOperation != nil
            && (meguriStage == .mediaBlocked || meguriStage == .mediaPaused || meguriStage == .mediaStopped)
    }
    public var deviceSettingsSettling: Bool { desiredProviderSettings != nil }
    public var meguriAutomaticSettingVisible: Bool {
        meguriReady && meguriRound != nil && providerState?.selection == .meguri
    }
    public var meguriAutomaticSettingAvailable: Bool {
        meguriAutomaticSettingVisible && provider != nil && meguriConnectionOwner != nil
            && meguriCanRun && meguriWake == nil && !meguriCancelling && !deviceSettingsSettling && meguriStage == nil
            && meguriPhase == .setupRequired && meguriStatus.route == .round
    }
    public var meguriStatus: MeguriSyncStatusPresentation {
        .init(stopped: providerNeedsAttention, connection: meguriAuthenticating ? .authenticating : meguriPhase,
            stage: meguriStage, routine: meguriRoutine, settling: deviceSettingsSettling)
    }
    var prepareEditorReplacement: (@MainActor (String, ReviewCollection.EditorReplacementDecision, @MainActor () -> Bool) async -> ReviewCollection.EditorReplacementAdmission?)?
    var editorReplacementIsCurrent: (@MainActor (UUID) -> Bool)?
    var finishEditorReplacement: (@MainActor (UUID) -> Void)?
    var finishCloudEditorReplacement: (@MainActor (UUID) async -> Void)?
    private var ownEditorReplacement: (owner: UUID, reservation: UUID, decision: ReviewCollection.EditorReplacementDecision)?
    private var meguriEditorDecision: (owner: UUID, decision: ReviewCollection.EditorReplacementDecision)?
    private func cancelOwnEditorDecision() {
        if let held = ownEditorReplacement, held.owner == ownOwner { held.decision.cancel() }
    }
    private func cancelMeguriEditorDecision(_ owner: UUID?) {
        if let held = meguriEditorDecision, held.owner == owner { held.decision.cancel() }
    }
    var beginMeguriActivation: (@MainActor (UUID) throws -> Void)?
    var finishMeguriActivation: (@MainActor (Bool) async -> SyncCoordinator?)?
    private var meguriHasLocalCollection = false
    private var meguriChoiceAllowed = false
    private var meguriAddressDirty = false
    private var meguriAddressEditRevision: UInt64 = 0
    private var meguriLoadedConnection: (owner: MeguriProviderConnectionOwner, connection: MeguriProviderConnection)?
    private var meguriAuthenticating = false
    private var meguriBootstrap: MeguriProviderBootstrapService?
    private var meguriOperation: MeguriBootstrapOperation?
    private var meguriActiveFacade: MeguriProviderBootstrapService?
    private var meguriAction: (id: UUID, task: Task<Void, Never>)?
    private var meguriRound: CoreMeguriRoundService? {
        didSet { if meguriRound !== oldValue { meguriLastExchange = nil } }
    }
    private var meguriReady = false {
        didSet { if !meguriReady { meguriLastExchange = nil } }
    }
    private var meguriWakePending = false
    private var meguriWakeManual = false
    private var meguriReachabilityRecoveryRevision: UInt64 = 0
    private var meguriReadyWakePending = false
    /// MP6.5 (resume fix): when setup's unfinished media step may continue by
    /// itself, and a resume asked for while another setup action ran.
    @ObservationIgnored private var meguriMediaResume = MeguriMediaResume()
    @ObservationIgnored private var meguriMediaResumePending: MeguriMediaResume.Trigger?
    @ObservationIgnored private var meguriMediaRetry: Task<Void, Never>?
    /// Tests only: replaces the wait before an automatic attempt.
    @ObservationIgnored var meguriMediaRetryDelay: ((Duration?) -> Duration?)?
    private var meguriWake: (id: UUID, task: Task<Void, Never>, availability: MeguriWakeAvailability)?
    private var meguriAddressDrain: Task<Void, Never>?
    private var meguriPauseDrain: Task<Void, Never>?
    private var meguriCancellation: Task<Void, Never>?
    public private(set) var meguriCollectionFinalization: MeguriRoundPhaseOutcome?
    public private(set) var meguriMediaFinalization: MeguriRoundPhaseOutcome?


    func setMeguriLocalCollection(_ exists: Bool) { meguriHasLocalCollection = exists }
    private func meguriCurrent(_ owner: MeguriProviderConnectionOwner, _ expected: UInt64, _ revision: UInt64) -> Bool {
        meguriConnectionOwner === owner && generation == expected && hostRevision == revision
            && !closing && !replacing && !suspended && !providerHandoffInProgress
    }
    private func issueMeguri(_ body: @escaping @MainActor (UUID) async -> Void) {
        guard !meguriBusy, !meguriCancelling, !closing, !replacing, !suspended, !providerActionInProgress,
              providerState?.selection == .meguri, !providerNeedsAttention else { return }
        let id = UUID()
        meguriBusy = true
        let task = Task {
            if !closing, !replacing, !suspended, !providerHandoffInProgress { await body(id) }
            if meguriAction?.id == id {
                meguriAction = nil; meguriActiveFacade = nil; meguriBusy = false; meguriProgress = nil
                reloadMeguriAfterRecoveryIfNeeded()
                if meguriReadyWakePending {
                    meguriReadyWakePending = false
                    requestMeguriWake(automatic: true)
                }
                if let trigger = meguriMediaResumePending {
                    meguriMediaResumePending = nil
                    resumeMeguriMediaIfNeeded(trigger)
                }
            }
        }
        meguriAction = (id, task)
    }
    func waitForMeguriAction() async { await meguriAction?.task.value }
    var meguriSignInAddressRevision: UInt64 { meguriAddressEditRevision }
    var meguriSignInAttachmentRevision: UInt64 { hostRevision }
    var meguriSignInAvailable: Bool {
        meguriLocalActionAvailable && !settings.paused
    }
    var meguriLocalActionAvailable: Bool {
        !meguriBusy && !meguriCancelling && !closing && !replacing && !suspended && !drained
            && !providerActionInProgress && !providerHandoffInProgress
            && providerState?.selection == .meguri && !providerNeedsAttention
    }
    var meguriSignInAttached: Bool { provider != nil && setupState == .attached && !backendRetired }
    func failMeguriSignInPreparation(_ failure: MeguriSignInLocalFailure) {
        meguriLastError = failure
        meguriSignInErrorKey = Self.meguriAuthenticationKey(failure)
    }
    func clearMeguriSignInPreparationError() { meguriSignInErrorKey = nil }
    public func meguriAddressEdited() {
        meguriAddressEditRevision &+= 1
        meguriAddressDirty = true; meguriReady = false; meguriRoutine = nil; meguriSignInErrorKey = nil
        meguriWakePending = false
        let original = meguriConnectionOwner, previous = meguriAddressDrain
        cancelMeguriEditorDecision(meguriAction?.id)
        let wake = meguriWake
        wake?.task.cancel()
        meguriAddressDrain = Task {
            await previous?.value
            await original?.cancelAndDrain()
            await wake?.task.value
        }
    }
    public func loadMeguriConnection() {
        loadMeguriConnection(startupForeground: nil)
    }
    private func loadMeguriConnection(startupForeground: (@MainActor () -> Bool)?) {
        let expected = generation, revision = hostRevision
        issueMeguri { _ in
            guard self.generation == expected, self.hostRevision == revision else { return }
            do { try await self.readMeguriConnection(startupForeground: startupForeground) }
            catch { self.meguriLastError = error }
        }
    }
    // Offline attachment/readiness only. A valid cold foreground attachment
    // can ask issueMeguri's completion to schedule its automatic opportunity.
    private func readMeguriConnection(startupForeground: (@MainActor () -> Bool)? = nil) async throws {
        // A local reload must not turn a deliberate draft edit into an
        // unchanged-address authentication attempt.
        guard !meguriAddressDirty else { return }
        let editRevision = meguriAddressEditRevision
        let expected = generation, revision = hostRevision
        let owner = try await meguriConnection()
        meguriReady = false; meguriRound = nil; meguriBootstrap = nil; meguriChoiceAllowed = false
        do {
            try await owner.load()
            guard meguriCurrent(owner, expected, revision), editRevision == meguriAddressEditRevision else { return }
            meguriPhase = await owner.phase
            meguriUsername = await owner.username
            let configured = try await provider?.configuredMeguri()
            guard meguriCurrent(owner, expected, revision), editRevision == meguriAddressEditRevision else { return }
            guard let configured else {
                meguriLoadedConnection = nil; meguriEndpoint = ""
                return
            }
            let facade = try await owner.bootstrapService()
            guard meguriCurrent(owner, expected, revision), editRevision == meguriAddressEditRevision else { return }
            guard facade.connection == configured.connection else { throw MeguriProviderAuthenticationFailure.connectionChanged }
            meguriLoadedConnection = (owner, facade.connection)
            meguriEndpoint = facade.connection.endpoint
            guard meguriPhase == .setupRequired else { return }
            meguriBootstrap = facade
            try await readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
            if meguriCurrent(owner, expected, revision), editRevision == meguriAddressEditRevision,
               meguriBootstrap === facade, meguriReady, startupForeground?() == true {
                // No suspension from this check through issueMeguri clearing
                // busy and requesting the wake; its normal gates still apply.
                meguriReadyWakePending = true
            }
            if meguriCurrent(owner, expected, revision), editRevision == meguriAddressEditRevision,
               meguriBootstrap === facade,
               let trigger = Self.meguriLaunchResume(stage: meguriStage, foreground: startupForeground?()) {
                // MP6.5 (resume fix): setup's unfinished media step continues
                // once this load's action ends, as Try Again would run it.
                meguriMediaResumePending = trigger
            }
        } catch {
            let phase = await owner.phase
            if meguriCurrent(owner, expected, revision) { meguriPhase = phase; meguriLastError = error }
            throw error
        }
    }
    private func readMeguriReadiness(_ facade: MeguriProviderBootstrapService, owner: MeguriProviderConnectionOwner,
                                     expected: UInt64, revision: UInt64) async throws {
        meguriChoiceAllowed = false
        let readiness = try await facade.readiness()
        guard meguriCurrent(owner, expected, revision), meguriBootstrap === facade else { return }
        switch readiness {
        case .ready:
            let round = try await owner.roundService()
            guard meguriCurrent(owner, expected, revision), meguriBootstrap === facade else { return }
            meguriReady = true; meguriRound = round; meguriStage = nil
            if meguriRoutine == nil { meguriRoutine = .unreported }
        case .unbound(let retained):
            meguriReady = false; meguriRound = nil; meguriRoutine = nil
            // Intent is historical display evidence. Only the checked lookup
            // names an active preparation, which may be newer than that intent.
            let pending = try await facade.pendingPreparation()
            guard meguriCurrent(owner, expected, revision), meguriBootstrap === facade else { return }
            meguriOperation = pending
            if let pending { showMeguriOperation(pending) }
            else if let retained, Self.meguriRetainsEffects(retained.receipt) {
                // Collection/media completion and unresolved acceptance or
                // activation still belong to the real retained intent handle.
                showMeguriOperation(retained)
            } else {
                if let retained { meguriReceipt = retained.receipt }
                meguriStage = retained == nil ? .required : .failed
                meguriChoiceAllowed = true
            }
        case .notSelected, .stale:
            meguriReady = false; meguriStage = .failed; meguriFailureKey = nil
        }
    }
    /// A retained operation whose collection transferred and whose media step
    /// has not finished setup, as a launch reads it: it continues by itself
    /// (MP6.5, resume fix), or waits for Try Again once its automatic attempts
    /// are used up. Only listed files make it media blocked, once a step has
    /// read them (MP5). Nil for any other operation.
    static func meguriRetainedMediaStage(_ receipt: MeguriBootstrapReceipt, stopped: Bool) -> MeguriSetupStage? {
        guard MeguriMediaResume.continues(receipt) else { return nil }
        return stopped ? .mediaStopped : .mediaPaused
    }
    /// Whether the launch's own load resumes setup's media step: in the
    /// foreground, as for the ready binding's first round.
    static func meguriLaunchResume(stage: MeguriSetupStage?, foreground: Bool?) -> MeguriMediaResume.Trigger? {
        stage == .mediaPaused && foreground == true ? .launch : nil
    }
    private static func meguriRetainsEffects(_ receipt: MeguriBootstrapReceipt) -> Bool {
        receipt.phase == .collectionComplete || receipt.phase == .mediaIncomplete || receipt.phase == .complete
            || !MeguriSyncStatusPresentation.canDiscard(receipt)
    }
    public func signInMeguri(endpoint: String, username: String, password: String) {
        guard !settings.paused else { return }
        let editRevision = meguriAddressEditRevision
        let expected = generation, revision = hostRevision
        issueMeguri { _ in
            guard self.generation == expected, self.hostRevision == revision,
                  editRevision == self.meguriAddressEditRevision else { return }
            self.meguriSignInErrorKey = nil
            do {
                await self.meguriAddressDrain?.value
                let owner = try await self.meguriConnection()
                guard self.meguriCurrent(owner, expected, revision) else { return }
                if !self.meguriAddressDirty, self.meguriLoadedConnection?.owner !== owner {
                    try await self.readMeguriConnection()
                }
                guard self.meguriCurrent(owner, expected, revision), editRevision == self.meguriAddressEditRevision else { return }
                let unchanged = !self.meguriAddressDirty && self.meguriLoadedConnection?.owner === owner
                    && self.meguriLoadedConnection?.connection.endpoint == endpoint
                do {
                    if !unchanged {
                        // Configure is the sole endpoint parser. An explicit
                        // edit still drains/configures even if its text returns
                        // to the old spelling; programmatic loads do not edit.
                        try await owner.configure(endpoint: endpoint)
                        guard self.meguriCurrent(owner, expected, revision), editRevision == self.meguriAddressEditRevision else { return }
                        self.meguriPhase = await owner.phase
                        self.meguriUsername = await owner.username
                        let configured = try await owner.bootstrapService()
                        guard self.meguriCurrent(owner, expected, revision), editRevision == self.meguriAddressEditRevision else { return }
                        self.meguriLoadedConnection = (owner, configured.connection)
                        self.meguriEndpoint = configured.connection.endpoint
                    }
                } catch {
                    if let bridge = error as? BridgeError, case .invalidArgument = bridge {
                        self.meguriSignInErrorKey = "sync.meguri.error.address"
                    }
                    throw error
                }
                guard self.meguriCurrent(owner, expected, revision), editRevision == self.meguriAddressEditRevision else { return }
                self.meguriAddressDirty = false; self.meguriReady = false
                self.meguriBootstrap = nil; self.meguriRound = nil; self.meguriRoutine = nil
                self.meguriAuthenticating = true
                defer { self.meguriAuthenticating = false }
                do { try await owner.signIn(username: username, password: password) }
                catch {
                    let phase = await owner.phase
                    if self.meguriCurrent(owner, expected, revision) { self.meguriPhase = phase }
                    throw error
                }
                guard self.meguriCurrent(owner, expected, revision) else { return }
                self.meguriPhase = await owner.phase
                self.meguriUsername = await owner.username
                let facade = try await owner.bootstrapService()
                guard self.meguriCurrent(owner, expected, revision) else { return }
                self.meguriBootstrap = facade
                try await self.readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
            } catch {
                self.meguriLastError = error
                if self.generation == expected, self.hostRevision == revision, editRevision == self.meguriAddressEditRevision {
                    if self.meguriSignInErrorKey == nil { self.meguriSignInErrorKey = Self.meguriAuthenticationKey(error) }
                    if let owner = self.meguriConnectionOwner {
                        self.meguriPhase = await owner.phase
                        self.meguriUsername = await owner.username
                    }
                }
            }
        }
    }
    // NoCollection alone does not prove that this coordinator's Host survived.
    // Refresh the exact retained provider, whose real facade rejects a retired
    // Host, before allowing the entry owner to refresh local setup authority.
    func validateRetainedMeguriAttachment(generation expected: UInt64) async throws -> Bool {
        guard generation == expected, !closing, !replacing, !suspended, !drained,
              retainedHostAdmitted, !backendRetired, setupState == .attached,
              let backend, let provider else { return false }
        let revision = hostRevision
        do {
            let state = try await provider.refresh()
            guard generation == expected, revision == hostRevision, self.backend === backend,
                  self.provider === provider, !closing, !replacing, !suspended, !drained,
                  !backendRetired, setupState == .attached else { return false }
            return state.selection == .meguri && state.phase != .blocked && state.phase != .handoffRecovery
        } catch { meguriLastError = error; throw error }
    }
    static func meguriAuthenticationKey(_ error: any Error) -> String {
        if let failure = error as? MeguriSignInLocalFailure {
            return switch failure {
            case .busy: "sync.meguri.error.busy"
            case .attachment: "sync.meguri.error.localSetup"
            case .credentialStore: "sync.meguri.error.credentialStore"
            }
        }
        if error is MeguriProviderCredentialFailure { return "sync.meguri.error.credentialStore" }
        if case BridgeError.busy = error { return "sync.meguri.error.busy" }
        if let failure = error as? MeguriProviderAuthenticationFailure {
            return switch failure {
            case .username, .credentials: "sync.meguri.error.credentials"
            case .redirectRefused: "sync.meguri.error.redirect"
            case .network: "sync.meguri.error.network"
            case .connectionChanged: "sync.meguri.error.other"
            }
        }
        return "sync.meguri.error.other"
    }
    private func progressForMeguri(_ id: UUID) -> @Sendable (CoreProgress) -> Void {
        { [weak self] value in
            Task { @MainActor in if self?.meguriAction?.id == id { self?.meguriProgress = value } }
        }
    }
    private func retainMeguri(_ result: MeguriBootstrapCommandResult) {
        meguriLastCommand = result
        meguriProgressFailure = result.progressFailure
        meguriMediaProgress = result.mediaProgress
        if let retry = result.retryOperation { meguriOperation = retry; meguriReceipt = retry.receipt }
    }
    func retainMeguriCollectionResult(_ result: MeguriBootstrapCommandResult) {
        meguriCollectionCommand = result
        retainMeguri(result)
        applyMeguriOutcome(result.outcome)
    }
    func refreshMeguriAfterCollection() async throws {
        try await readMeguriConnection()
        guard meguriChoiceAllowed else { return }
        if meguriCollectionError != nil { meguriStage = .failed }
        else if let result = meguriCollectionCommand, case .refused = result.outcome { meguriStage = .failed }
    }
    private func showMeguriOperation(_ operation: MeguriBootstrapOperation) {
        meguriOperation = operation; meguriReceipt = operation.receipt
        let receipt = operation.receipt
        meguriChoiceAllowed = false
        if receipt.acceptance == .overtaken {
            // MP6.9 neither: settled and inactive. Its artifacts stay in the
            // core; setup runs again and the core gives the sheet.
            meguriOperation = nil; meguriFailureKey = MeguriSetupSequence.overtakenKey
            meguriStage = .failed; meguriChoiceAllowed = true
        } else if receipt.acceptance == .possiblyAccepted || receipt.boundary == .remoteCompletionUnknown {
            meguriStage = .acceptanceUnknown
        } else if MeguriSyncStatusPresentation.canRecover(receipt) {
            meguriStage = .acceptedLocalIncomplete
        } else if let stage = Self.meguriRetainedMediaStage(receipt, stopped: meguriMediaResume.stopped) {
            meguriStage = stage
        } else if MeguriSyncStatusPresentation.canDiscard(receipt) {
            meguriStage = .required; meguriChoiceAllowed = true
        } else { meguriStage = .failed }
    }
    private func applyMeguriOutcome(_ outcome: MeguriBootstrapOutcome) {
        switch outcome {
        case .preview(let preview): meguriPreview = preview; meguriReceipt = preview.receipt; meguriStage = .preview
        case .operation(let operation): showMeguriOperation(operation)
        case .mediaConsent(let consent): meguriReceipt = consent.receipt; meguriStage = .media
        case .ready(let accounting): meguriAccounting = accounting
        case .settled: break
        case .refused(let failure, let accounting):
            meguriAccounting = accounting; meguriFailureKey = MeguriSyncStatusPresentation.failureKey(failure)
            meguriStage = accounting == nil ? .failed : .mediaBlocked
            if let operation = meguriOperation {
                let receipt = operation.receipt
                if receipt.acceptance == .possiblyAccepted || receipt.boundary == .remoteCompletionUnknown { meguriStage = .acceptanceUnknown }
                else if MeguriSyncStatusPresentation.canRecover(receipt) { meguriStage = .acceptedLocalIncomplete }
            }
        }
    }
    public func prepareMeguri(_ direction: MeguriBootstrapDirection) {
        guard meguriCanChoose, providerChangeAllowed(), let facade = meguriBootstrap, let owner = meguriConnectionOwner else { return }
        issueMeguri { id in
            let expected = self.generation, revision = self.hostRevision
            self.meguriActiveFacade = facade; self.meguriFailureKey = nil; self.meguriChoiceAllowed = false
            do {
                // Cold NotSent must be explicitly discarded on the same facade.
                // Unknown/NeedsDecision never enter this path.
                if let pending = self.meguriOperation {
                    guard MeguriSyncStatusPresentation.canDiscard(pending.receipt) else { return }
                    let discarded = try await facade.discard(pending)
                    self.retainMeguri(discarded)
                    guard self.meguriCurrent(owner, expected, revision) else { return }
                    if case .refused = discarded.outcome { self.applyMeguriOutcome(discarded.outcome); return }
                    self.meguriOperation = nil
                }
                self.meguriStage = .preparing
                let result = try await facade.prepare(direction, progress: self.progressForMeguri(id))
                self.retainMeguri(result)
                guard self.meguriCurrent(owner, expected, revision) else { return }
                self.applyMeguriOutcome(result.outcome)
                if let retry = result.retryOperation { self.showMeguriOperation(retry) }
                if case .refused = result.outcome {
                    // Obtain actual local authority after a drained failed
                    // preparation; never retry to discover remote acceptance.
                    try await self.readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
                    if self.meguriChoiceAllowed { self.meguriStage = .failed }
                }
            } catch { self.meguriLastError = error; self.meguriStage = .failed }
        }
    }
    public func cancelMeguriPreparation() {
        guard meguriCancellation == nil, let original = meguriAction, let facade = meguriActiveFacade else { return }
        meguriCancelling = true
        cancelMeguriEditorDecision(original.id)
        meguriCancellation = Task {
            await facade.stop(.user)
            await original.task.value
            meguriCancelling = false; meguriCancellation = nil
        }
    }
    public func discardMeguriPreview() {
        guard let preview = meguriPreview, let facade = meguriBootstrap else { return }
        issueMeguri { _ in
            self.meguriActiveFacade = facade
            do {
                let result = try await facade.discard(preview)
                self.retainMeguri(result)
                self.applyMeguriOutcome(result.outcome)
                if case .refused = result.outcome { return }
                // Only the actual local discard lets the sheet go away.
                if self.meguriPreview === preview {
                    self.meguriPreview = nil; self.meguriOperation = nil; self.meguriStage = .required; self.meguriChoiceAllowed = true
                }
            } catch { self.meguriLastError = error; self.meguriStage = .failed }
        }
    }
    public func confirmMeguriPreview() {
        guard let preview = meguriPreview, let facade = meguriBootstrap, !settings.paused,
              let owner = meguriConnectionOwner else { return }
        issueMeguri { id in
            _ = await self.runMeguriCollection(id: id, facade: facade, owner: owner,
                held: { self.meguriPreview === preview },
                discard: { try await facade.discard(preview) },
                release: { self.meguriPreview = nil },
                perform: { try await facade.confirm(preview, progress: self.progressForMeguri(id)) })
        }
    }
    /// The one path that replaces a collection for Meguri Sync: the editor's
    /// decision, the activation reservation, the command, then the reload on
    /// whichever coordinator owns the collection afterwards. `held` says the
    /// presented decision is still the one being confirmed; `release` forgets it.
    private func runMeguriCollection(id: UUID, facade: MeguriProviderBootstrapService, owner: MeguriProviderConnectionOwner,
                                     held: @escaping @MainActor @Sendable () -> Bool,
                                     discard: @MainActor () async throws -> MeguriBootstrapCommandResult,
                                     release: @MainActor () -> Void,
                                     perform: @MainActor () async throws -> MeguriBootstrapCommandResult) async -> SyncCoordinator? {
        guard !settings.paused, let prepareEditorReplacement, let finishEditorReplacement,
              let beginMeguriActivation, let finishMeguriActivation else { return nil }
        let expected = self.generation, revision = self.hostRevision
        self.meguriActiveFacade = facade
        let current: @MainActor @Sendable () -> Bool = {
            self.meguriAction?.id == id && self.meguriBootstrap === facade && held() &&
                self.meguriCurrent(owner, expected, revision) && !self.meguriCancelling &&
                !self.settings.paused && !Task.isCancelled
        }
        let decision = ReviewCollection.EditorReplacementDecision()
        self.meguriEditorDecision = (id, decision)
        defer { if self.meguriEditorDecision?.owner == id { self.meguriEditorDecision = nil } }
        let admission = await prepareEditorReplacement("Meguri Sync", decision, current)
        defer { if let admission { finishEditorReplacement(admission.id) } }
        // MP6.5: a blocked phase starts no activation. It ends this unconfirmed
        // preparation as a refused admission does, so setup can be tried again.
        guard let admission, admission.accepted, !self.providerNeedsAttention else {
            // A discard that failed or was refused: the core says whether setup can
            // run again, so that a preparation with no sheet is not left without an
            // action. A failed read never infers that the preparation is gone.
            func recoverAfterUnfinishedDiscard() async {
                do { try await self.readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision) }
                catch { if current() { self.meguriStage = .failed }; return }
                if self.meguriChoiceAllowed { release(); self.meguriStage = .failed }
            }
            do {
                let result = try await discard()
                guard self.meguriAction?.id == id, self.meguriCurrent(owner, expected, revision), held() else { return nil }
                self.retainMeguri(result); self.applyMeguriOutcome(result.outcome)
                if case .refused = result.outcome { await recoverAfterUnfinishedDiscard(); return nil }
                release(); self.meguriOperation = nil
                self.meguriStage = .required; self.meguriChoiceAllowed = true
            } catch {
                guard current() else { return nil }
                self.meguriLastError = error
                await recoverAfterUnfinishedDiscard()
            }
            return nil
        }
        let reservation = admission.id
        guard current() else { return nil }
        do { try beginMeguriActivation(reservation) }
        catch { self.meguriLastError = error; return nil }
        let before = self.generation
        self.meguriActiveFacade = facade; self.meguriStage = .collection
        // Consume the presentation once. The original facade/preview live
        // in this task through its terminal read, including activated errors.
        release(); self.meguriChoiceAllowed = false
        self.meguriCollectionError = nil
        var candidateActive = false
        do {
            try decision.admitEffect()
            let result = try await perform()
            self.retainMeguriCollectionResult(result)
            if case .refused(.transfer(let failure), _) = result.outcome { candidateActive = failure.candidateActive }
        } catch {
            self.meguriLastError = error; self.meguriCollectionError = error
            candidateActive = (error as? ManualTransferFailure)?.candidateActive == true
            self.meguriStage = .failed
        }
        candidateActive = candidateActive || before != self.generation
        // Never drain this connection before its own confirmation. The
        // existing replacement path joins only the native facade, not us.
        guard let current = await finishMeguriActivation(candidateActive) else { return nil }
        if current !== self {
            current.meguriLastCommand = self.meguriLastCommand; current.meguriLastError = self.meguriLastError
            current.meguriReceipt = self.meguriReceipt; current.meguriProgressFailure = self.meguriProgressFailure
            current.meguriCollectionCommand = self.meguriCollectionCommand; current.meguriCollectionError = self.meguriCollectionError
            current.meguriFailureKey = self.meguriFailureKey
        }
        do {
            // Reload after Host adoption or validation of the retained
            // NoCollection attachment. Old opaque handles are never rebound.
            try await current.refreshMeguriAfterCollection()
        } catch { current.meguriLastError = error; current.meguriStage = .failed; return current }
        if let operation = current.meguriOperation,
           operation.receipt.phase == .collectionComplete || operation.receipt.phase == .mediaIncomplete {
            // T19.3: what setup did when it asked nothing, from the end of the collection's transfer.
            if let done = self.meguriSetupDonePending { current.meguriSetupDone = done }
            self.meguriSetupDonePending = nil
            await current.runMeguriMediaStep(operation, id: id)
        }
        return current
    }

    // MARK: Meguri Sync setup (MP6.3, MP6.9)

    /// Runs after a Sign In made to set the service up, from Try Again, and
    /// from the status action that asks which collection to keep. The core
    /// reads both sides and decides: it transfers with no question only what
    /// MP6.3 rows 1 to 3 allow, and otherwise the keep-collection sheet opens.
    /// `keepingService`: Cancel in that sheet changes nothing (V27.4).
    public func startMeguriSetup(keepingService: Bool = false) {
        guard meguriCanChoose, providerChangeAllowed(), meguriSetupDecision == nil,
              let facade = meguriBootstrap, let owner = meguriConnectionOwner else { return }
        let overtaken = meguriStage == .failed && meguriFailureKey == MeguriSetupSequence.overtakenKey
        meguriSetupKeepsService = MeguriSetupSequence.keepsService(requested: keepingService, stage: meguriStage,
                                                                   failureKey: meguriFailureKey)
        issueMeguri { id in
            // The sentence of the row that opened the sheet is its first line (T19.3).
            self.meguriSetupNoticeKey = overtaken ? MeguriSetupSequence.overtakenKey : nil
            await self.prepareMeguriSetup(id: id, facade: facade, owner: owner, reloads: 1)
        }
    }
    private func prepareMeguriSetup(id: UUID, facade: MeguriProviderBootstrapService, owner: MeguriProviderConnectionOwner,
                                    reloads: Int) async {
        let expected = self.generation, revision = self.hostRevision
        self.meguriActiveFacade = facade; self.meguriFailureKey = nil; self.meguriChoiceAllowed = false
        do {
            // A preparation that sent nothing is discarded before a new one.
            if let pending = self.meguriOperation {
                guard MeguriSyncStatusPresentation.canDiscard(pending.receipt) else { return }
                let discarded = try await facade.discard(pending)
                self.retainMeguri(discarded)
                guard self.meguriCurrent(owner, expected, revision) else { return }
                if case .refused = discarded.outcome { self.applyMeguriOutcome(discarded.outcome); return }
                self.meguriOperation = nil
            }
            self.meguriStage = .preparing
            let result = try await facade.prepareSetup(progress: self.progressForMeguri(id))
            self.retainMeguri(result)
            guard self.meguriCurrent(owner, expected, revision) else { return }
            guard case .preview(let preview) = result.outcome else {
                self.applyMeguriOutcome(result.outcome)
                if let retry = result.retryOperation { self.showMeguriOperation(retry) }
                if case .refused = result.outcome {
                    try await self.readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
                    if self.meguriChoiceAllowed { self.meguriStage = .failed }
                }
                return
            }
            self.meguriReceipt = preview.receipt
            let outcome: MeguriSetupDecisionOutcome
            do { outcome = try await facade.setupDecision(preview) } catch {
                // The read failed after a preparation that sent nothing: Try Again stays possible.
                self.meguriLastError = error
                guard self.meguriCurrent(owner, expected, revision) else { return }
                await self.abandonMeguriSetupPreview(preview, facade: facade, owner: owner, expected: expected, revision: revision)
                return
            }
            guard self.meguriCurrent(owner, expected, revision) else { return }
            if self.meguriCancelling {
                // Cancel during the read: no sheet opens and nothing runs after it (MP6.3).
                self.meguriFailureKey = MeguriSyncStatusPresentation.cancelledFailureKey
                await self.abandonMeguriSetupPreview(preview, facade: facade, owner: owner, expected: expected, revision: revision)
                return
            }
            guard case .decision(let decision) = outcome else {
                if case .refused(let failure) = outcome { self.meguriFailureKey = MeguriSyncStatusPresentation.failureKey(failure) }
                self.retainMeguri(try await facade.discard(preview))
                self.meguriStage = .failed; self.meguriChoiceAllowed = true
                return
            }
            switch Self.meguriSetupStep(decision.facts.row) {
            case .ask:
                // Nothing is replaced before the sheet's answer.
                self.meguriSetupPreview = preview; self.meguriSetupDecision = decision
                self.meguriStage = .preview
            case .run(let choice):
                self.meguriSetupPreview = preview
                await self.runMeguriSetup(id: id, facade: facade, owner: owner, decision: decision, preview: preview,
                                          choice: choice, reloads: reloads)
            }
        } catch { self.meguriLastError = error; self.meguriStage = .failed }
    }
    /// A prepared setup that no decision followed. It sent nothing: it is
    /// discarded, and after a discard that did not happen the core says
    /// whether setup can run again.
    private func abandonMeguriSetupPreview(_ preview: MeguriBootstrapPreview, facade: MeguriProviderBootstrapService,
                                           owner: MeguriProviderConnectionOwner, expected: UInt64, revision: UInt64) async {
        var discarded = false
        if let cancelled = try? await MeguriSetupSequence.cancel(facade, preview: preview) {
            retainMeguri(cancelled.result); discarded = cancelled.discarded
        }
        guard meguriCurrent(owner, expected, revision) else { return }
        meguriStage = .failed
        if discarded { meguriChoiceAllowed = true; return }
        try? await readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
        if meguriChoiceAllowed { meguriStage = .failed }
    }
    enum MeguriSetupStep: Equatable { case ask, run(MeguriSetupChoice) }
    /// Only rows 1 to 3 run without the sheet (MP6.3).
    static func meguriSetupStep(_ row: MeguriSetupRow) -> MeguriSetupStep {
        switch row {
        case .download, .downloadOverEmptyDevice, .uploadToEmptyServer: .run(.noQuestion)
        case .ask, .askUploadOnly, .askDownloadOnly: .ask
        }
    }
    /// The choices the sheet offers for a row; a row that asks nothing offers none.
    public static func meguriSetupChoices(_ row: MeguriSetupRow) -> [MeguriSetupChoice] {
        switch row {
        case .ask: [.keepDevice, .keepServer]
        case .askUploadOnly: [.keepDevice]
        case .askDownloadOnly: [.keepServer]
        case .download, .downloadOverEmptyDevice, .uploadToEmptyServer: []
        }
    }
    /// True when the prepared upload itself is confirmed; false when the download replaces it.
    static func meguriSetupConfirmsPreview(_ row: MeguriSetupRow, _ choice: MeguriSetupChoice) -> Bool? {
        switch (row, choice) {
        case (.download, .noQuestion), (.uploadToEmptyServer, .noQuestion), (.ask, .keepDevice), (.askUploadOnly, .keepDevice): true
        // With no collection on this device the prepared transfer is already the download.
        case (.askDownloadOnly, .keepServer): true
        case (.downloadOverEmptyDevice, .noQuestion), (.ask, .keepServer): false
        default: nil
        }
    }
    static func meguriSetupDone(_ row: MeguriSetupRow) -> MeguriSetupDone? {
        switch row {
        case .download: .downloaded(backup: false)
        case .downloadOverEmptyDevice: .downloaded(backup: true)
        case .uploadToEmptyServer: .uploaded
        case .ask, .askUploadOnly, .askDownloadOnly: nil
        }
    }
    /// The facts changed under a decision: both sides are read again (T19.3).
    static func meguriSetupChangeKey(_ outcome: MeguriBootstrapOutcome) -> String? {
        guard case .refused(let failure, _) = outcome else { return nil }
        switch failure {
        case .setupLocalChanged: return "sync.meguri.setup.error.localChanged"
        case .setupRemoteChanged: return "sync.meguri.setup.error.remoteChanged"
        case .transfer(let transfer):
            switch transfer.boundary {
            case .localChanged: return "sync.meguri.setup.error.localChanged"
            case .remoteChanged: return "sync.meguri.setup.error.remoteChanged"
            default: return nil
            }
        default: return nil
        }
    }
    /// The sheet's answer. A button is the confirmation (N15.3).
    public func answerMeguriSetup(_ choice: MeguriSetupChoice) {
        guard let decision = meguriSetupDecision, let preview = meguriSetupPreview,
              Self.meguriSetupChoices(decision.facts.row).contains(choice),
              let facade = meguriBootstrap, let owner = meguriConnectionOwner, !settings.paused else { return }
        issueMeguri { id in
            self.meguriSetupNoticeKey = nil
            await self.runMeguriSetup(id: id, facade: facade, owner: owner, decision: decision, preview: preview,
                                      choice: choice, reloads: 1)
        }
    }
    private func runMeguriSetup(id: UUID, facade: MeguriProviderBootstrapService, owner: MeguriProviderConnectionOwner,
                                decision: MeguriSetupDecision, preview: MeguriBootstrapPreview,
                                choice: MeguriSetupChoice, reloads: Int) async {
        guard Self.meguriSetupConfirmsPreview(decision.facts.row, choice) != nil else { return }
        meguriSetupDonePending = choice == .noQuestion ? Self.meguriSetupDone(decision.facts.row) : nil
        // This attempt's own answer: an earlier attempt's refusal reloads nothing.
        var attempt: MeguriBootstrapOutcome?
        let current = await runMeguriCollection(id: id, facade: facade, owner: owner,
            held: { self.meguriSetupPreview === preview },
            discard: { try await facade.discard(preview) },
            release: { self.meguriSetupPreview = nil; self.meguriSetupDecision = nil },
            perform: {
                guard let result = try await MeguriSetupSequence.transfer(facade, decision: decision, preview: preview,
                    choice: choice, progress: self.progressForMeguri(id),
                    proceed: { !self.meguriCancelling && !self.settings.paused && !self.providerNeedsAttention }) else { throw CancellationError() }
                attempt = result.outcome
                return result
            })
        meguriSetupDonePending = nil
        // Changed facts stopped the transfer with nothing replaced. Both sides
        // are read again once; the row they give now decides, and two
        // collections get the sheet with a new choice.
        // A Cancel during the transfer starts no new preparation.
        guard let current, !meguriCancelling,
              let key = MeguriSetupSequence.reloadNotice(after: attempt, reloads: reloads,
                                                         choiceAllowed: current.meguriChoiceAllowed),
              let facade = current.meguriBootstrap, let owner = current.meguriConnectionOwner else { return }
        current.meguriSetupNoticeKey = key
        current.meguriSetupKeepsService = self.meguriSetupKeepsService
        if current === self {
            await prepareMeguriSetup(id: id, facade: facade, owner: owner, reloads: reloads - 1)
        } else {
            current.startMeguriSetup(keepingService: current.meguriSetupKeepsService)
        }
    }
    /// Cancel in the sheet. The prepared upload sent nothing and is discarded.
    /// Returns whether the caller must also leave the service Off (MP6.3).
    public func cancelMeguriSetup() async -> Bool {
        let leavesOff = !meguriSetupKeepsService
        guard let preview = meguriSetupPreview, let facade = meguriBootstrap else {
            meguriSetupDecision = nil; meguriSetupPreview = nil
            // A transfer that already took the preview is not undone by leaving the service Off.
            return leavesOff && !meguriBusy
        }
        await waitForMeguriAction()
        // A confirmation that ran meanwhile consumed the preview: nothing is left to cancel.
        guard meguriSetupPreview === preview else { return leavesOff && meguriStage == .required }
        let owner = meguriConnectionOwner
        var discarded: Bool?
        issueMeguri { _ in
            let expected = self.generation, revision = self.hostRevision
            self.meguriActiveFacade = facade
            discarded = false
            do {
                let cancelled = try await MeguriSetupSequence.cancel(facade, preview: preview)
                self.retainMeguri(cancelled.result)
                discarded = cancelled.discarded
                if cancelled.discarded { self.meguriOperation = nil; self.meguriStage = .required; self.meguriChoiceAllowed = true }
                else { self.applyMeguriOutcome(cancelled.result.outcome) }
            } catch { self.meguriLastError = error; self.meguriStage = .failed }
            // A discard that failed leaves the core to say whether setup can run
            // again, so that Try Again is offered from its answer.
            if discarded != true, let owner {
                try? await self.readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
                if self.meguriChoiceAllowed { self.meguriStage = .failed }
            }
            // The sheet always closes: Cancel is never a dead end.
            if self.meguriSetupPreview === preview { self.meguriSetupPreview = nil; self.meguriSetupDecision = nil }
        }
        await waitForMeguriAction()
        if meguriSetupPreview === preview { meguriSetupPreview = nil; meguriSetupDecision = nil }
        // A download whose discard failed is not abandoned by removing the sign-in.
        return MeguriSetupSequence.cancelLeavesOff(keepsService: !leavesOff, discarded: discarded)
    }
    /// MP6.9 for a setup upload: one read of the server's facts. It starts no upload.
    public var meguriCanSettle: Bool {
        !meguriBusy && !settings.paused && meguriStage == .acceptanceUnknown && meguriBootstrap != nil
    }
    public func settleMeguriSetup() {
        guard meguriCanSettle, let facade = meguriBootstrap, let owner = meguriConnectionOwner else { return }
        issueMeguri { _ in
            let expected = self.generation, revision = self.hostRevision
            self.meguriActiveFacade = facade
            var finish = false
            do {
                let result = try await facade.settleSetup()
                self.retainMeguri(result)
                guard self.meguriCurrent(owner, expected, revision) else { return }
                switch MeguriSetupSequence.settled(result.outcome) {
                case .accepted(let operation, let hintUnchanged):
                    // Local activation remains. It is tried now only on the
                    // hint that this device did not change since the upload,
                    // and then only through the core's own test of that;
                    // otherwise Finish Setup asks for it (T19.5).
                    self.showMeguriOperation(operation)
                    finish = hintUnchanged
                case .overtaken:
                    // Neither: another client changed the server. The core
                    // recorded it, kept every artifact and released the
                    // exclusion; the sheet decides (MP6.9, T19.5).
                    self.meguriOperation = nil; self.meguriFailureKey = MeguriSetupSequence.overtakenKey
                    self.meguriStage = .failed; self.meguriChoiceAllowed = true
                case .notAccepted:
                    // Nothing reached the server and nothing was replaced.
                    self.meguriOperation = nil; self.meguriFailureKey = "sync.meguri.setup.error.server"
                    self.meguriStage = .failed; self.meguriChoiceAllowed = true
                case .unknown:
                    // Not readable: the state and the exclusion stay.
                    self.meguriStage = .acceptanceUnknown
                }
            } catch { self.meguriLastError = error; self.meguriStage = .acceptanceUnknown }
            if finish { self.meguriFinishAfterSettle = true }
        }
        Task {
            await waitForMeguriAction()
            guard meguriFinishAfterSettle else { return }
            meguriFinishAfterSettle = false
            finishMeguriSetup(asked: false)
        }
    }
    /// MP6.9 for a routine round: the next round's first step is the read of
    /// the server's facts against the last proved baseline; pending changes
    /// stay pending and that round sends them.
    public func settleMeguriRound() {
        // Unknown stays shown until a round, whose first step is the read, can be admitted.
        guard meguriRoutine == .acceptanceUnknown, meguriWake == nil, !meguriBusy,
              meguriCanRun, meguriSchedulingAllowed(automatic: false) else { return }
        meguriRoutine = .pending
        requestMeguriWake(automatic: false)
    }
    /// `asked` is false only for the continuation of a settled acceptance: the
    /// core then replaces this device's collection only while it is still what
    /// the upload sent, and otherwise leaves Finish Setup to the user (S1).
    public func finishMeguriSetup(asked: Bool = true) {
        guard meguriCanFinish, let operation = meguriOperation, let facade = meguriBootstrap,
              let owner = meguriConnectionOwner,
              let prepareEditorReplacement, let finishEditorReplacement,
              let beginMeguriActivation, let finishMeguriActivation else { return }
        issueMeguri { id in
            let expected = self.generation, revision = self.hostRevision
            self.meguriActiveFacade = facade
            let current: @MainActor @Sendable () -> Bool = {
                self.meguriAction?.id == id && self.meguriBootstrap === facade &&
                    self.meguriCurrent(owner, expected, revision) && !self.meguriCancelling &&
                    !self.settings.paused && !Task.isCancelled
            }
            let decision = ReviewCollection.EditorReplacementDecision()
            self.meguriEditorDecision = (id, decision)
            defer { if self.meguriEditorDecision?.owner == id { self.meguriEditorDecision = nil } }
            let admission = await prepareEditorReplacement("Meguri Sync", decision, {
                current() && self.meguriOperation === operation
            })
            defer { if let admission { finishEditorReplacement(admission.id) } }
            guard let admission, admission.accepted else { return }
            let reservation = admission.id
            guard current(), self.meguriOperation === operation else { return }
            do { try beginMeguriActivation(reservation) } catch { self.meguriLastError = error; return }
            let before = self.generation
            self.meguriActiveFacade = facade; self.meguriStage = .collection
            self.meguriCollectionError = nil
            var candidateActive = false
            do {
                try decision.admitEffect()
                // Finish Setup continues the original, already confirmed
                // ExactOk operation. Core's recovery preview is not a new
                // upload choice and is confirmed only on this same facade.
                let finished = try await MeguriSetupSequence.finish(facade, operation: operation, asked: asked,
                    progress: self.progressForMeguri(id), current: current)
                self.meguriRecoveryPreparation = finished.preparation
                self.retainMeguri(finished.preparation)
                let result = finished.confirmation ?? finished.preparation
                self.retainMeguriCollectionResult(result)
                if case .refused(.transfer(let failure), _) = result.outcome { candidateActive = failure.candidateActive }
            } catch {
                self.meguriLastError = error; self.meguriCollectionError = error
                candidateActive = (error as? ManualTransferFailure)?.candidateActive == true
                self.meguriStage = .failed
            }
            candidateActive = candidateActive || before != self.generation
            guard let current = await finishMeguriActivation(candidateActive) else { return }
            if current !== self {
                current.meguriLastCommand = self.meguriLastCommand; current.meguriLastError = self.meguriLastError
                current.meguriReceipt = self.meguriReceipt; current.meguriProgressFailure = self.meguriProgressFailure
                current.meguriCollectionCommand = self.meguriCollectionCommand; current.meguriCollectionError = self.meguriCollectionError
                current.meguriFailureKey = self.meguriFailureKey
            }
            do { try await current.refreshMeguriAfterCollection() }
            catch { current.meguriLastError = error; current.meguriStage = .failed; return }
            if let operation = current.meguriOperation,
               operation.receipt.phase == .collectionComplete || operation.receipt.phase == .mediaIncomplete {
                await current.runMeguriMediaStep(operation, id: id)
            }
        }
    }
    /// Try Again on a paused, stopped or blocked setup media step. It runs the
    /// step now and starts its automatic attempts again (MP6.5, resume fix).
    public func retryMeguriMedia() { runMeguriMedia(.user) }
    /// MP6.5 (resume fix): continue setup's unfinished media step without the
    /// person, when Meguri launches, comes back to the foreground, gets its
    /// network back, the blocked phase clears or a wait after a failure ends.
    /// Sync Automatically does not hold it: the person started setup, and
    /// setup ends only once its media finishes (MP5).
    func resumeMeguriMediaIfNeeded(_ trigger: MeguriMediaResume.Trigger) {
        guard meguriStage == .mediaPaused, meguriOperation != nil, meguriMediaResume.admits(trigger) else { return }
        // A setup action that still runs reads Core again when it ends.
        if meguriBusy { meguriMediaResumePending = trigger; return }
        runMeguriMedia(trigger)
    }
    private func runMeguriMedia(_ trigger: MeguriMediaResume.Trigger) {
        guard meguriCanRetryMedia, meguriMediaResume.admits(trigger) else { return }
        meguriMediaRetry?.cancel(); meguriMediaRetry = nil
        meguriMediaResume.started(trigger)
        issueMeguri { id in
            do {
                // The launch's own load has just read Core; any later attempt
                // reads the retained operation again first.
                if trigger != .launch { try await self.readMeguriConnection() }
            } catch {
                self.meguriLastError = error
                if self.meguriStage == .mediaBlocked || self.meguriStage == .mediaPaused || self.meguriStage == .mediaStopped {
                    self.recordMeguriMediaAttempt(.unfinished)
                }
                return
            }
            guard let current = self.meguriOperation else { return }
            await self.runMeguriMediaStep(current, id: id)
        }
    }
    /// One run of the media step and its record: whether and when it continues by itself.
    private func runMeguriMediaStep(_ operation: MeguriBootstrapOperation, id: UUID) async {
        let outcome: MeguriMediaResume.Outcome
        do { outcome = try await continueMeguriMedia(operation, id: id) }
        catch {
            meguriLastError = error
            // An error is not a refusal: the operation and its media stay for the next attempt.
            if meguriStage == .media { pauseMeguriMedia(operation) }
            outcome = .unfinished
        }
        recordMeguriMediaAttempt(outcome)
    }
    private func recordMeguriMediaAttempt(_ outcome: MeguriMediaResume.Outcome) {
        let wait = meguriMediaResume.finished(outcome)
        if meguriStage == .mediaPaused, meguriMediaResume.stopped { meguriStage = .mediaStopped }
        meguriMediaRetry?.cancel(); meguriMediaRetry = nil
        guard let wait = meguriMediaRetryDelay.map({ $0(wait) }) ?? wait else { return }
        let expected = generation, revision = hostRevision
        meguriMediaRetry = Task {
            try? await Task.sleep(for: wait)
            guard !Task.isCancelled, generation == expected, hostRevision == revision, !closing else { return }
            meguriMediaRetry = nil
            resumeMeguriMediaIfNeeded(.retry)
        }
    }
    /// The media step stopped before it finished: its operation and media stay,
    /// and the step continues (MP5).
    private func pauseMeguriMedia(_ operation: MeguriBootstrapOperation) {
        meguriOperation = operation; meguriReceipt = operation.receipt; meguriFailureKey = nil
        meguriStage = meguriMediaResume.stopped ? .mediaStopped : .mediaPaused
    }
    /// A refused media step: paused, stopped for the person, blocked by listed
    /// files, or setup's failure, as `MeguriMediaResume.outcome` says.
    private func refuseMeguriMedia(_ outcome: MeguriBootstrapOutcome, _ operation: MeguriBootstrapOperation) -> MeguriMediaResume.Outcome {
        guard case .refused(let failure, let accounting) = outcome else { return .notRun }
        let result = MeguriMediaResume.outcome(refused: failure, accounting: accounting)
        switch result {
        case .unfinished: pauseMeguriMedia(operation)
        case .stopped:
            meguriOperation = operation; meguriReceipt = operation.receipt; meguriFailureKey = nil
            meguriStage = .mediaStopped
        case .blocked: meguriOperation = operation; meguriStage = .mediaBlocked
        case .ready, .failed, .notRun: break
        }
        return result
    }
    private func continueMeguriMedia(_ operation: MeguriBootstrapOperation, id: UUID) async throws -> MeguriMediaResume.Outcome {
        guard let facade = meguriBootstrap, let owner = meguriConnectionOwner, !settings.paused else { return .notRun }
        let expected = generation, revision = hostRevision
        guard meguriCurrent(owner, expected, revision), MeguriMediaResume.continues(operation.receipt) else { return .notRun }
        // MP6.5: a blocked phase starts no media step. The operation keeps its
        // pending media, and the step continues once the phase recovers.
        guard !providerNeedsAttention else { pauseMeguriMedia(operation); return .notRun }
        meguriActiveFacade = facade; meguriStage = .media
        var finished = operation
        // A step that finished before a quit has only its completion left.
        if operation.receipt.phase != .complete {
            // Offline: the core reads the media folder again and binds this run to
            // the same operation, account and activated collection (MP4, MP5).
            let prepared = try await facade.prepareMedia(operation, progress: progressForMeguri(id))
            retainMeguri(prepared)
            guard meguriCurrent(owner, expected, revision) else { return .notRun }
            applyMeguriOutcome(prepared.outcome)
            guard case .mediaConsent(let consent) = prepared.outcome else { return refuseMeguriMedia(prepared.outcome, operation) }
            guard !providerNeedsAttention else { pauseMeguriMedia(operation); return .notRun }
            let result = try await facade.transferMedia(consent, progress: progressForMeguri(id), mediaProgress: { [weak self] value in
                Task { @MainActor in if self?.meguriAction?.id == id { self?.meguriMediaProgress = value } }
            })
            retainMeguri(result)
            guard meguriCurrent(owner, expected, revision) else { return .notRun }
            applyMeguriOutcome(result.outcome)
            guard case .operation(let transferred) = result.outcome else { return refuseMeguriMedia(result.outcome, operation) }
            // Stopped before it finished (a quit, a lost connection, a server
            // error, a blocked phase): the same operation continues later.
            guard transferred.receipt.phase == .complete else {
                pauseMeguriMedia(transferred)
                return providerNeedsAttention ? .notRun : .unfinished
            }
            finished = transferred
            meguriStage = .media
        }
        let outcome = try await facade.complete(finished)
        applyMeguriOutcome(outcome)
        guard meguriCurrent(owner, expected, revision) else { return .notRun }
        guard case .ready = outcome else { return refuseMeguriMedia(outcome, finished) }
        // Whole-operation Ready is Core's durable automatic-sync consent.
        let state = try await provider?.refresh()
        guard meguriCurrent(owner, expected, revision), let state, let committed = state.settings else { return .notRun }
        providerState = state; settings = committed
        if committed.automatic, !storedAutomatic {
            setDeviceSettings(paused: committed.paused, automatic: false)
        }
        try await readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
        guard meguriCurrent(owner, expected, revision), meguriReady else { return .notRun }
        meguriOperation = nil
        meguriReadyWakePending = true
        return .ready
    }
    private var meguriCanRun: Bool {
        meguriReady && meguriRound != nil && providerState?.selection == .meguri && settings.enabled
            && !settings.paused && desiredProviderSettings?.paused != true && meguriPauseDrain == nil && !meguriAddressDirty && !meguriBusy
            && !closing && !suspended && !replacing && !providerActionInProgress && !providerHandoffInProgress
            && !providerNeedsAttention && !entryActivationHeld
    }
    // Any settings write closes Core's ordinary admission. Reserve against new
    // rounds until it settles; durable Automatic OFF still allows manual rounds.
    static func meguriSchedulingAllowed(automatic: Bool, committed: CloudPolicyDeviceSettings,
                                       desired: CloudPolicyDeviceSettings?) -> Bool {
        desired == nil && committed.enabled && !committed.paused && (!automatic || committed.automatic)
    }
    private func meguriSchedulingAllowed(automatic: Bool) -> Bool {
        // P25.3: a stored off outlives a setup that committed automatic on.
        (!automatic || storedAutomatic)
            && Self.meguriSchedulingAllowed(automatic: automatic, committed: settings, desired: desiredProviderSettings)
    }
    public func retryMeguriRoundMedia() {
        guard meguriRoutine == .mediaBlocked, meguriWake == nil, !meguriBusy else { return }
        requestMeguriWake(automatic: false, retryMedia: true)
    }
    private func requestMeguriWake(automatic: Bool, retryMedia: Bool = false) {
        guard meguriCanRun, meguriSchedulingAllowed(automatic: automatic) else { return }
        switch meguriRoutine {
        case .mediaBlocked: if !retryMedia { return }
        case .acceptanceUnknown, .fullSyncRequired, .authentication, .failed: return
        default: break
        }
        meguriWakePending = true
        if !automatic { meguriWakeManual = true }
        guard meguriWake == nil, let round = meguriRound, let owner = meguriConnectionOwner else { return }
        let id = UUID(), expected = generation, revision = hostRevision
        let editRevision = meguriAddressEditRevision
        let availability = MeguriWakeAvailability(generation: expected)
        // Reserve the product task before the first actor/credential suspension.
        let task = Task {
            defer { if meguriWake?.id == id { meguriWake = nil } }
            while meguriWakePending, meguriCanRun, meguriRound === round,
                  meguriCurrent(owner, expected, revision), !Task.isCancelled {
                let manual = meguriWakeManual
                let recoveryRevision = meguriReachabilityRecoveryRevision
                meguriWakePending = false; meguriWakeManual = false
                guard meguriSchedulingAllowed(automatic: !manual) else { return }
                meguriRoutine = .exchanging
                let availabilityRevision = availability.revision
                do {
                    let report = try await round.fullRound()
                    // Retain actual terminal effects before checking retirement.
                    meguriRoundReport = report
                    meguriCollectionFinalization = nil; meguriMediaFinalization = nil
                    guard meguriWake?.id == id, meguriCurrent(owner, expected, revision), meguriRound === round else { return }
                    meguriAccounting = nil
                    reduceMeguriRound(report)
                    // Only this actual routine round can supply a new instant.
                    // Pending/skipped work retains the earlier complete exchange.
                    if meguriRoutine == .idle, !Task.isCancelled, !meguriCancelling,
                       editRevision == meguriAddressEditRevision, meguriCanRun {
                        meguriLastExchange = Int64(clock.now().timeIntervalSince1970 * 1_000)
                    }
                    #if DEBUG
                    // Finalization retries and bootstrap never reach this site.
                    // The owner/attempt guard above and cancellation check exclude
                    // retired or cancelled completions, even if they returned facts.
                    if !Task.isCancelled, meguriRoutine == .idle {
                        debugMeguriRoundSuccessSequence += 1
                    }
                    #endif
                    if meguriRoutine == .fullSyncRequired, let facade = meguriBootstrap {
                        try await readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
                        // T19.5 "round failed, full sync required": the needs-you row with its Choose action.
                        if meguriChoiceAllowed, meguriStage == .required {
                            meguriStage = .failed; meguriFailureKey = MeguriSetupSequence.fullSyncKey
                        }
                    }
                    // Preserve only an already queued recovery wake. The next
                    // round consumes the latest revision before suspending.
                    let recoveredNetwork = meguriRoutine == .network && reachable == true
                        && meguriReachabilityRecoveryRevision != recoveryRevision
                    // Available or SessionEnded may arrive while a not-started
                    // collection Busy is still returning. Replay once in this
                    // wake for that outcome; a Media Busy or terminal cannot qualify.
                    let missedAvailability = availability.consume(collectionOutcome: report.collection.outcome,
                        since: availabilityRevision, allowed: meguriSchedulingAllowed(automatic: true) && meguriCanRun && !meguriCancelling
                            && editRevision == meguriAddressEditRevision)
                    if missedAvailability { meguriWakePending = true }
                    else if meguriRoutine != .idle && meguriRoutine != .pending && !recoveredNetwork { meguriWakePending = false }
                } catch {
                    meguriLastError = error
                    guard meguriWake?.id == id, meguriCurrent(owner, expected, revision), meguriRound === round,
                          editRevision == meguriAddressEditRevision else { return }
                    if !applyMeguriRoundCredentialFailure(error) {
                        meguriRoutine = error is CancellationError ? .cancelled : .failed
                    }
                    meguriWakePending = false
                }
            }
        }
        meguriWake = (id, task, availability)
    }
    // Only fullRound's typed, pre-network secure-read failures use this local
    // recovery. The caller checks the original owner, round and attempt first.
    @discardableResult func applyMeguriRoundCredentialFailure(_ error: any Error) -> Bool {
        guard !Task.isCancelled, !meguriAddressDirty else { return false }
        let phase: MeguriProviderConnectionPhase
        switch error {
        case MeguriRoundAdmissionFailure.credentialsMissing: phase = .signedOut
        case MeguriProviderCredentialFailure.access(_),
             MeguriProviderCredentialFailure.malformedPayload,
             MeguriProviderCredentialFailure.wrongConnection: phase = .blocked
        default: return false
        }
        meguriPhase = phase
        if phase == .signedOut { meguriUsername = nil }
        // Reopen only through explicit sign-in or a checked local load. Keep
        // the connection, binding and all retained result/recovery evidence.
        meguriReady = false; meguriRound = nil; meguriRoutine = nil
        meguriWakePending = false; meguriWakeManual = false; meguriReadyWakePending = false
        return true
    }
    private func reduceMeguriRound(_ report: MeguriRoundReport) {
        let collectionOutcome = meguriCollectionFinalization ?? report.collection.outcome
        guard case .collection(let collection) = collectionOutcome else {
            meguriRoutine = Self.meguriUnstartedPhaseState(collectionOutcome); return
        }
        if collection.acceptanceUnknown { meguriRoutine = .acceptanceUnknown; return }
        if case .fullSyncRequired = collection.state { meguriRoutine = .fullSyncRequired; return }
        guard collection.recovery == nil, report.collection.trackingFailure == nil else { meguriRoutine = .failed; return }
        if let failure = collection.failure { meguriRoutine = Self.meguriRoundFailure(failure); return }
        guard collection.succeeded else { meguriRoutine = .failed; return }
        guard let mediaResult = report.media else { meguriRoutine = .failed; return }
        let mediaOutcome = meguriMediaFinalization ?? mediaResult.outcome
        guard case .media(let media) = mediaOutcome else {
            meguriRoutine = Self.meguriUnstartedPhaseState(mediaOutcome); return
        }
        guard media.recovery == nil, media.refusal == nil, media.accountingFailure == nil,
              mediaResult.trackingFailure == nil else { meguriRoutine = .failed; return }
        if let failure = media.failure { meguriRoutine = Self.meguriRoundFailure(failure); return }
        guard media.complete else { meguriRoutine = .mediaBlocked; return }
        meguriRoutine = collection.skipped > 0 ? .pending : .idle
    }
    static func meguriUnstartedPhaseState(_ outcome: MeguriRoundPhaseOutcome) -> MeguriRoutineState {
        // Only a typed start refusal proves this phase did no work. A later
        // legitimate wake may try again; terminal failures keep their guards.
        if case .refused(.busy) = outcome { return .waiting }
        return .failed
    }
    private static func meguriRoundFailure(_ failure: MeguriRoundFailure) -> MeguriRoutineState {
        switch failure.kind {
        case "auth_failed": .authentication
        case "network", "offline": .network
        case "server_error", "unexpected_status", "server_message": .server
        case "cancelled", "interrupted": .cancelled
        default: .failed
        }
    }
    public func retryMeguriFinalization() {
        guard meguriWake == nil, !meguriBusy, !closing, !replacing, let report = meguriRoundReport else { return }
        issueMeguri { _ in
            // Retry only the sealed local result; never resend a round to learn
            // whether its remote work completed.
            do {
                let expected = self.generation, revision = self.hostRevision
                if let finalization = report.collection.finalization { self.meguriCollectionFinalization = try await finalization.retry() }
                if let finalization = report.media?.finalization { self.meguriMediaFinalization = try await finalization.retry() }
                guard expected == self.generation, revision == self.hostRevision, !self.closing, !self.replacing else { return }
                self.reduceMeguriRound(report)
            } catch { self.meguriLastError = error }
        }
    }
    private func drainMeguriProduct() async {
        meguriWakePending = false; meguriWakeManual = false; meguriReadyWakePending = false
        meguriMediaResumePending = nil; meguriMediaRetry?.cancel(); meguriMediaRetry = nil
        let wake = meguriWake, action = meguriAction, facade = meguriActiveFacade, round = meguriRound
        cancelMeguriEditorDecision(action?.id)
        wake?.task.cancel()
        await facade?.stop(.system)
        if meguriAuthenticating { await meguriConnectionOwner?.cancelAndDrain() }
        await round?.cancelAndDrain()
        await wake?.task.value
        await action?.task.value
        await meguriCancellation?.value
        await meguriAddressDrain?.value
    }
    func configureProviderDirectory(_ directory: URL) {
        // Custom backend fixtures retain their injected owner. Ordinary native
        // composition and ServiceAppHarness opt in at construction; CoreHost's
        // durable gate remains mandatory in either case.
        if providerDirectory != nil { providerDirectory = directory }
    }
    private var providerAllowsAccountRead: Bool {
        desiredProviderSettings?.paused != true
            && (providerDirectory == nil || providerDiscovery != nil || providerState?.permitsICloudStartup == true)
    }

    func makeProviderSuccessor(generation expected: UInt64) async throws -> SyncCoordinator {
        let revision = hostRevision
        guard let prepared = providerPlatform else {
            throw BridgeError.unavailable(reason: "Provider platform preparation unavailable")
        }
        func current() -> Bool {
            drained && closing && closeTask == nil && backend == nil && provider == nil
                && providerHandoff == nil && !replacing && !entryActivationHeld
                && !providerAttachmentPending && generation == expected && prepared.generation == expected
                && providerPlatform?.owner === prepared.owner
                && hostRevision == revision
        }
        guard current() else {
            throw BridgeError.busy(runningOperation: "provider drain")
        }
        // This facade still validates its original Core Host/generation, even
        // when the retained Host has no collection. It grants no new admission.
        _ = try await prepared.owner.state()
        guard current() else {
            throw BridgeError.unavailable(reason: "collectionUnavailable")
        }
        let successor = SyncCoordinator(bootstrap: bootstrap, makeBackend: makeBackend,
            defaults: defaults, clock: clock, directory: directory, observeSignals: observeSignals,
            providerDirectory: providerDirectory, meguriCredentialStore: meguriCredentialStore, recordDiagnostic: recordDiagnostic)
        // The original already joined its native owners above. The successor
        // needs an attachment retry, not a strong chain of drained owners.
        successor.providerAttachmentPending = true
        successor.generation = expected
        successor.providerPlatform = prepared
        successor.operationFailed = operationFailed
        successor.failureRevision = failureRevision
        return successor
    }

    private func attachProvider(to backend: any SyncCoordinatorBackend,
                                startupForeground: (@MainActor () -> Bool)? = nil) async throws {
        guard providerDirectory != nil else { return }
        let expected = generation, revision = hostRevision
        let current: CoreProviderService
        if let issued = backend.providerService { current = issued }
        else { current = try await CoreProviderService.attached() }
        let state = try await current.refresh()
        guard !closing, !backendRetired, self.backend === backend,
              expected == generation, revision == hostRevision else { return }
        provider = current
        providerState = state
        guard let settings = state.settings else {
            throw BridgeError.unavailable(reason: "Provider settings require recovery")
        }
        self.settings = settings
        if providerNeedsAttention { operationFailed = true }
        if state.selection == .meguri, !meguriBusy { loadMeguriConnection(startupForeground: startupForeground) }
    }

    public convenience init(directory: URL, clock: AppClock = .system,
                            configuration: CloudBuildConfiguration? = .fromBundle, defaults: UserDefaults? = .standard,
                            recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in }) {
        let providerDirectory = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: directory, appGroupSnapshot: directory).manualTransferDirectory
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
        }, defaults: defaults, clock: clock, directory: directory, observeSignals: true,
           providerDirectory: providerDirectory, recordDiagnostic: recordDiagnostic)
    }

    init(bootstrap: @escaping @MainActor () async throws -> Void,
         makeBackend: @escaping @MainActor (@escaping @Sendable () async -> Void) async throws -> any SyncCoordinatorBackend,
         defaults: UserDefaults?, clock: AppClock = .system, directory: URL? = nil, observeSignals: Bool = false,
         providerDirectory: URL? = nil, meguriCredentialStore: (any MeguriProviderCredentialStore)? = nil,
         recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in }) {
        self.recordDiagnostic = recordDiagnostic
        self.bootstrap = bootstrap; self.makeBackend = makeBackend; self.defaults = defaults
        self.clock = clock; self.directory = directory
        self.providerDirectory = providerDirectory
        self.meguriCredentialStore = meguriCredentialStore
        self.observeSignals = observeSignals
        let stored = (try? CoreProviderService.legacySettings(defaults?.object(forKey: Self.settingsKey)))
            ?? .init(enabled: false, paused: false, automatic: true, media: true)
        // P25.1: a stored media off from an earlier build reads as on.
        settings = .init(enabled: stored.enabled, paused: stored.paused, automatic: stored.automatic, media: true)
    }

    /// Both open and prepareEntry call this before either can create a Host.
    /// Missing setup is local-only availability, not a local opening failure.
    func preparePlatform() async {
        guard !entryActivationHeld else { return }
        if let platformTask { await platformTask.value; return }
        if providerAttachmentPending, let prepared = providerPlatform {
            let revision = hostRevision
            let state: SyncProviderState
            do { state = try await prepared.owner.state() }
            catch {
                guard providerPlatform?.owner === prepared.owner, hostRevision == revision else { return }
                setupState = .failed; operationFailed = true
                return
            }
            guard providerAttachmentPending, providerPlatform?.owner === prepared.owner,
                  generation == prepared.generation, hostRevision == revision,
                  !entryActivationHeld, !replacing, !closing || drained else {
                setupState = .failed; operationFailed = true
                return
            }
            providerState = state
            if let committed = state.settings { settings = committed }
            // Reuse actual successful setup on this same open Host. Backend
            // construction and provider refresh still run on every attachment.
            platformPrepared = true
            setupState = .prepared
        }
        if drained { closing = false; drained = false }
        guard !platformPrepared else { return }
        let task = Task {
            defer { platformTask = nil }
            if let providerDirectory {
                do {
                    providerState = try await CoreProviderService.migrate(directory: providerDirectory,
                        legacyJSON: CoreProviderService.legacyMigrationJSON(defaults?.object(forKey: Self.settingsKey)))
                    if let migrated = providerState?.settings { settings = migrated }
                } catch { markProviderBlocked(.migration, error); operationFailed = true }
            }
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
    func completeOpenedHost(generation: UInt64, startupForeground: (@MainActor () -> Bool)? = nil) async {
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
                try await attachProvider(to: value, startupForeground: startupForeground)
                guard !closing, !Task.isCancelled, !replacing,
                      revision == hostRevision, generation == self.generation else { return }
                drained = false
                setupState = .attached
                providerAttachmentPending = false
                providerPlatform = nil
                attachmentFailure = nil; connectionFailure = nil
                let retained = await readRetainedRecovery(value)
                guard !closing, !suspended, !replacing, !Task.isCancelled,
                      backend === value, !backendRetired, !drained,
                      revision == hostRevision, generation == self.generation else { return }
                attachRetainedRecovery(retained, backend: value, generation: generation, revision: revision)
                await migrateRound2Settings(retained)
                guard !closing, !suspended, !replacing, !Task.isCancelled,
                      backend === value, !backendRetired, !drained,
                      revision == hostRevision, generation == self.generation else { return }
                enqueue(.policy(.settings(settings)))
                enqueue(.replayVisibleMedia)
                enqueue(.policy(.refresh(.cloud)))
                enqueue(.account(false))
                if observeSignals { observePlatformSignals() }
            } catch {
                #if DEBUG
                DebugCloudDiagnostics.record(.backendAttachment, error: error)
                #endif
                guard revision == hostRevision, generation == self.generation else { return }
                attachmentFailure = error
                recordConnectionFailure(error)
                setupState = .failed
            }
        }
        attachmentTask = task
        await task.value
    }

    func requireProviderAttachment() throws {
        guard setupState == .attached else {
            throw attachmentFailure ?? BridgeError.unavailable(reason: "Provider attachment unavailable")
        }
    }
    func providerAttachmentFailed(_ error: any Error) {
        guard providerAttachmentPending else { return }
        // Host preparation may report only that no connection was attached.
        // Preserve the factory/refresh failure from the actual attachment.
        if attachmentFailure == nil { attachmentFailure = error }
        recordConnectionFailure(attachmentFailure ?? error)
        setupState = .failed
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
            recoveryReadFailed = false
            if status != nil { retainedRecoveryStart = (backend, generation, revision) }
        case .failure:
            recoveryReadFailed = true
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
        if event.generation == generation, providerState?.selection == .meguri {
            meguriWake?.availability.observe(kind: event.kind, sequence: event.seq, generation: event.generation)
            switch event.kind {
            case .changes(let changes):
                if changes.source != .syncIncoming && changes.source != .cloudIncoming && changes.source != .cloudRekey {
                    requestMeguriWake(automatic: true)
                }
            case .sessionEnded: requestMeguriWake(automatic: true)
            case .available:
                if meguriWake == nil { requestMeguriWake(automatic: true) }
            default: break
            }
        }
        switch event.kind {
        case .sessionStarted, .sessionEnded, .changes, .available, .mediaArrived:
            // The Driver reads the actual session registry; focus is never a
            // SessionEnded input and cannot release the newest held answer.
            enqueue(.core(event))
        default: break
        }
    }

    /// The coordinator's one settings write. P25: the interface reaches it only
    /// through `setAutomatic` and `clearUpdateHold`, so no person can pause or
    /// turn media off; the pause stays for the coordinator and its tests.
    public func setDeviceSettings(paused: Bool, automatic: Bool) {
        // P25.1: no caller, the tests included, can turn media off.
        // The device's own value never disagrees with a value written here. It is
        // still first stored only by the switch or the migration (P25.3).
        if automaticIsStored, storedAutomatic != automatic { rememberAutomatic(automatic) }
        guard !closing, !replacing, !providerActionInProgress else { return }
        guard desiredProviderSettings == nil || providerState?.phase != .blocked else { return }
        // Meguri's controls read committed fields. Reject reentry so a rapid
        // ON or Pause/Resume cannot supersede a pending OFF with stale fields.
        guard providerState?.selection != .meguri || !deviceSettingsSettling else { return }
        if let provider {
            providerSettingsRevision &+= 1
            let revision = providerSettingsRevision
            let attachment = hostRevision
            let previous = providerSettingsTask
            let next = CloudPolicyDeviceSettings(enabled: settings.enabled, paused: paused, automatic: automatic, media: true)
            desiredProviderSettings = next
            if paused {
                initialOwnAdmission.setOpen(false)
                // Stop admission and cancel the original page before local
                // provider I/O. The original operation may join this settings
                // task, so the settings task must never join its drain.
                if initialAdoptionEnabledBefore != nil { drainInitialOwnWork() }
            }
            providerSettingsTask = Task {
                await previous?.value
                do {
                    let state = try await provider.saveSettings(next)
                    guard revision == providerSettingsRevision, attachment == hostRevision,
                          self.provider === provider, !closing, !replacing else { return }
                    guard let committed = state.settings else {
                        throw BridgeError.unavailable(reason: "Provider settings require recovery")
                    }
                    providerState = state
                    desiredProviderSettings = nil
                    applyDeviceSettings(paused: committed.paused, automatic: committed.automatic, enabled: committed.enabled)
                    if committed.paused, state.selection == .meguri, meguriPauseDrain == nil {
                        // This join cannot live in providerSettingsTask: a
                        // confirmation's replacement joins that settings task.
                        let expected = generation
                        meguriPauseDrain = Task {
                            await self.drainMeguriProduct()
                            self.meguriPauseDrain = nil
                            guard self.provider === provider, attachment == self.hostRevision,
                                  expected == self.generation else { return }
                            // A rapid Resume may have committed before the old
                            // wake released its slot. Recheck after the join.
                            self.requestMeguriWake(automatic: true)
                        }
                    }
                } catch {
                    guard attachment == hostRevision,
                          self.provider === provider, !closing, !replacing else { return }
                    // A queued later request cannot replace the original
                    // failed write retained by Core (in particular Pause).
                    if providerState?.phase != .blocked { desiredProviderSettings = next }
                    markProviderBlocked(.settingsWrite, error); operationFailed = true
                }
            }
            return
        }
        applyDeviceSettings(paused: paused, automatic: automatic)
    }
    /// P25.1: a committed value from an earlier build's ledger may still say media off; it is applied as on.
    private func applyDeviceSettings(paused: Bool, automatic: Bool, enabled: Bool? = nil) {
        let wasPaused = settings.paused
        let wasAutomatic = settings.automatic
        if paused { initialOwnAdmission.setOpen(false) }
        saveSettings(.init(enabled: enabled ?? settings.enabled, paused: paused, automatic: automatic, media: true))
        if wasPaused, !paused, settings.enabled, let pending = retainedRecoveryStart,
           pending.backend === backend, pending.generation == generation, pending.revision == hostRevision,
           !closing, !suspended, !replacing, !drained, !backendRetired, setupState == .attached,
           initialAdoptionEnabledBefore == nil {
            // Revisit this attachment through a new ordered read. The proof
            // that returned while paused grants no authority to this attempt.
            enqueue(.account(false))
        }
        if providerState?.selection == .meguri, (wasPaused && !paused) || (!wasAutomatic && automatic) {
            requestMeguriWake(automatic: true)
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
        cancelOwnEditorDecision()
        original?.cancel()
        initialOwnDrain = Task {
            await own.interruptAndDrain()
            await original?.value
            initialOwnDrain = nil
            retryAccountCompletion()
        }
    }
    private func saveSettings(_ value: CloudPolicyDeviceSettings) {
        // The assignment reads media as on (P25.1); store and send that value.
        settings = value
        defaults?.set(["enabled": settings.enabled, "paused": settings.paused,
            "automatic": settings.automatic, "media": true], forKey: Self.settingsKey)
        receive(.settings(settings))
    }
    private func persistSettings(_ settings: CloudPolicyDeviceSettings) async throws {
        if let provider { providerState = try await provider.saveSettings(settings) }
        saveSettings(settings)
    }
    public func syncNow(source: CloudPolicySource = .command) {
        if updateHold {
            // P25.2: write `paused: false` first; the request follows it.
            Task { if await clearUpdateHold() { syncNow(source: source) } }
            return
        }
        if providerState?.selection == .meguri {
            guard meguriStatus.route == .round else { return }
            requestMeguriWake(automatic: false)
            return
        }
        guard providerAllowsAccountRead else { return }
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
        let preparation = ResumePreparation()
        startOwnTask(backend: backend, generation: expected, revision: revision, resumePreparation: preparation) { [self] in
            defer { readingPreview = false; finishOwnWork() }
            do {
                if let provider { providerDiscovery = try await provider.beginICloudDiscovery() }
                guard !Task.isCancelled, !closing, !suspended, !replacing,
                      expected == generation, revision == hostRevision, self.backend === backend else { throw CancellationError() }
                // No native own work has started. Issue the real producer on
                // the tracked account lane and join its completion/outputs.
                discoveryAccountRead = (nil, nil)
                enqueue(.account(false))
                var prepared = false
                while !prepared {
                    await prepareConnectionAuthority()
                    for original in Array(operationTasks.values) { await original.value }
                    prepared = try await withCheckedThrowingContinuation {
                        enqueue(.discoveryPrepared(preparation: preparation, backend: backend,
                            generation: expected, revision: revision, acknowledged: $0))
                    }
                }
                guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused,
                      expected == generation, revision == hostRevision, self.backend === backend else { throw CancellationError() }
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
                    let enabledBefore = settings.enabled
                    try await saveEnabled(true)
                    guard !Task.isCancelled, !closing, !suspended, !replacing, !settings.paused,
                          expected == generation, revision == hostRevision else {
                        try await saveEnabled(enabledBefore)
                        try await connection.cancelPreview(handle)
                        return
                    }
                    initialAdoptionEnabledBefore = enabledBefore
                    initialOwnAdmission.setOpen(true)
                    observeInitialAdoptionAcceptance?()
                    do {
                        _ = try await connection.confirmAdoption(handle)
                        if let provider { providerState = try await provider.acceptICloudConnection() }
                        if expected == generation, revision == hostRevision, !closing { connectionFailure = nil }
                    }
                    catch {
                        // The call did not accept an operation. Restore even
                        // when authority/account changed during the call.
                        await restoreInitialAdoptionEnabled()
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
            cancelOwnEditorDecision()
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
    public func stopUsingICloud() async { await chooseLocalOnly(stopOnly: true) }
    public func keepLocalOnly() async { await chooseLocalOnly(stopOnly: false) }
    /// MP-E can present these typed commands later. No service picker or
    /// transfer producer is introduced here.
    @discardableResult public func changeProvider(to destination: SyncProviderSelection, removeMeguriCredentials: Bool = false) async -> Bool {
        guard providerChangeAllowed(), let provider, !suspended, !replacing,
              !providerActionInProgress, !entryActivationHeld,
              !closing || providerHandoff != nil else {
            connectionFailure = .busy; return false
        }
        providerActionInProgress = true
        defer { providerActionInProgress = false }
        let expected = generation, revision = hostRevision
        func current(_ attachment: UInt64) -> Bool {
            self.provider === provider && expected == generation && attachment == hostRevision
                && !replacing && !entryActivationHeld
        }
        do {
            if providerHandoff == nil {
                let handoff = try await provider.beginHandoff(to: destination, removeCredentials: removeMeguriCredentials)
                guard current(revision) else { return false }
                providerHandoff = handoff
                providerHandoffDestination = destination
            } else if providerHandoffDestination != destination { connectionFailure = .busy; return false }
            let state = try await provider.state()
            guard current(revision) else { return false }
            providerState = state
            if removeMeguriCredentials, providerState?.removalPending != true { connectionFailure = .busy; return false }
            guard providerChangeAllowed() else { connectionFailure = .busy; return false }
            // Both Core admissions closed durably before cancellation. Keep
            // original native owners until their actual results and ACKs join.
            await drainMeguriProduct()
            await meguriConnectionOwner?.cancelAndDrain()
            await drainManualProviderOwner?()
            guard current(revision), providerChangeAllowed() else { return false }
            let joinedRevision = drained ? revision : revision &+ 1
            guard await drainProviderHost(), let providerHandoff else { return false }
            guard current(joinedRevision), providerChangeAllowed() else { return false }
            if providerState?.removalPending == true {
                let credentials = try await meguriConnection()
                try await credentials.deleteCredentialsAfterDrain()
                guard current(joinedRevision) else { return false }
            }
            let committed = try await provider.finishHandoff(providerHandoff)
            guard current(joinedRevision) else { return false }
            providerState = committed
            self.providerHandoff = nil; providerHandoffDestination = nil; providerDiscovery = nil
            if let committed = providerState?.settings { settings = committed }
            self.provider = nil; meguriConnectionOwner = nil
            // The original coordinator stays closed with its completed drain.
            // ReviewCollection composes and attaches a distinct current owner.
            guard let attachProviderSuccessor else {
                throw BridgeError.unavailable(reason: "Provider successor composition unavailable")
            }
            try await attachProviderSuccessor(self)
            return true
        } catch {
            guard (self.provider === provider || self.provider == nil && drained && closing),
                  expected == generation, !replacing else { return false }
            operationFailed = true
            do { providerState = try await provider.state() }
            catch { meguriLastError = error }
            recordConnectionFailure(error)
            if case BridgeError.busy = error { connectionFailure = .busy }
            return false
        }
    }
    public func meguriConnection() async throws -> MeguriProviderConnectionOwner {
        guard !replacing else { throw BridgeError.busy(runningOperation: "collection replacement") }
        if let meguriConnectionOwner { return meguriConnectionOwner }
        guard let provider else { throw BridgeError.unavailable(reason: "Open the local collection first") }
        let owner: MeguriProviderConnectionOwner
        if let store = meguriCredentialStore {
            owner = MeguriProviderConnectionOwner(service: provider, store: store)
        } else {
            owner = MeguriProviderConnectionOwner(service: provider)
        }
        meguriConnectionOwner = owner
        return owner
    }
    public func removeMeguriConnection() async -> Bool { await changeProvider(to: .localOnly, removeMeguriCredentials: true) }
    /// Recover a persisted intent whose original process handle is gone. Only
    /// this attachment's actual tasks are cancelled/joined; Core owns all
    /// generation, publication, uncertainty and credential-evidence checks.
    @discardableResult public func recoverProviderHandoffToLocalOnly() async -> Bool {
        guard !providerActionInProgress, providerChangeAllowed(), let provider,
              providerHandoff == nil, !suspended, !replacing, !entryActivationHeld,
              providerState?.phase == .handoffRecovery || providerRecoveryDrain != nil,
              !closing || providerRecoveryDrain != nil else { return false }
        providerActionInProgress = true
        defer { providerActionInProgress = false }
        let token = providerRecoveryDrain ?? UUID()
        providerRecoveryDrain = token
        let expected = generation, revision = hostRevision
        func current(_ attachment: UInt64) -> Bool {
            providerRecoveryDrain == token && self.provider === provider && expected == generation
                && attachment == hostRevision && !replacing && !entryActivationHeld
        }
        do {
            await drainMeguriProduct()
            await meguriConnectionOwner?.cancelAndDrain()
            await drainManualProviderOwner?()
            guard current(revision), providerChangeAllowed() else { return false }
            let joinedRevision = drained ? revision : revision &+ 1
            guard await drainProviderHost(), current(joinedRevision), providerChangeAllowed() else { return false }
            let state = try await provider.recoverHandoffToLocalOnly()
            guard current(joinedRevision) else { return false }
            try await attachRecoveredProviderSuccessor(state)
            return true
        } catch {
            guard providerRecoveryDrain == token, self.provider === provider, expected == generation, !replacing else { return false }
            operationFailed = true
            recordConnectionFailure(error)
            return false
        }
    }
    private func attachRecoveredProviderSuccessor(_ state: SyncProviderState) async throws {
        guard let attachProviderSuccessor else {
            throw BridgeError.unavailable(reason: "Provider successor composition unavailable")
        }
        providerState = state
        if let committed = state.settings { settings = committed }
        providerRecoveryDrain = nil; providerDiscovery = nil
        provider = nil; meguriConnectionOwner = nil; desiredProviderSettings = nil
        do { try await attachProviderSuccessor(self) }
        catch { recordConnectionFailure(error); throw error }
    }
    /// Retry Core's retained failed publication, never a refresh or a newly
    /// guessed settings write. Simple settings retry remains usable in review.
    @discardableResult public func retryFailedProviderPublication() async -> Bool {
        guard !providerActionInProgress, let provider, !suspended, !replacing, !entryActivationHeld else { return false }
        let recoveryDrain = providerRecoveryDrain
        guard recoveryDrain != nil || desiredProviderSettings != nil || providerState?.phase == .blocked else { return false }
        guard recoveryDrain == nil ? !closing && !drained && !backendRetired : drained && providerChangeAllowed() else { return false }
        providerActionInProgress = true
        defer { providerActionInProgress = false }
        let expected = generation, revision = hostRevision
        func current() -> Bool {
            self.provider === provider && expected == generation && revision == hostRevision
                && !replacing && !entryActivationHeld && providerRecoveryDrain == recoveryDrain
                && (recoveryDrain != nil || !closing && !backendRetired)
        }
        await providerSettingsTask?.value
        guard current(), recoveryDrain == nil || providerChangeAllowed() else { return false }
        guard recoveryDrain != nil || desiredProviderSettings != nil || providerState?.phase == .blocked else { return false }
        do {
            let state = try await provider.retryFailedPublication()
            guard current() else { return false }
            if recoveryDrain != nil {
                try await attachRecoveredProviderSuccessor(state)
            } else {
                guard let committed = state.settings else {
                    throw BridgeError.unavailable(reason: "Provider settings require recovery")
                }
                providerState = state
                desiredProviderSettings = nil
                applyDeviceSettings(paused: committed.paused, automatic: committed.automatic, enabled: committed.enabled)
            }
            return true
        } catch {
            guard current() else { return false }
            markProviderBlocked(.publicationRetry, error); operationFailed = true
            recordConnectionFailure(error)
            return false
        }
    }
    /// Retry retains the original process's actual handoff ownership. A restart
    /// only refreshes typed recovery; explicit recovery has a separate action.
    public func retryProviderAuthority() async {
        if providerAttachmentPending {
            guard setupState == .failed, !retryingProviderAttachment, !suspended, !replacing,
                  providerChangeAllowed(), let retryProviderAttachment else { return }
            retryingProviderAttachment = true
            defer { retryingProviderAttachment = false }
            let expected = generation
            do { try await retryProviderAttachment(self) }
            catch { if generation == expected { providerAttachmentFailed(error) } }
            return
        }
        if let destination = providerHandoffDestination {
            _ = await changeProvider(to: destination)
        } else if let provider, !closing, !drained, !backendRetired, !replacing, !providerActionInProgress {
            let expected = generation, revision = hostRevision
            do {
                let state = try await provider.refresh()
                guard self.provider === provider, expected == generation, revision == hostRevision, !closing, !replacing else { return }
                providerState = state
                if providerNeedsAttention { operationFailed = true }
                else if providerAllowsAccountRead { enqueue(.account(false)) }
            } catch {
                guard self.provider === provider, expected == generation, revision == hostRevision, !closing, !replacing else { return }
                markProviderBlocked(.authorityRefresh, error); operationFailed = true
            }
        }
    }
    private func chooseLocalOnly(stopOnly: Bool) async {
        if provider != nil {
            guard providerChangeAllowed() else { connectionFailure = .busy; return }
            if stopOnly || (preview == nil && recovery == nil) {
                await changeProvider(to: .localOnly)
                return
            }
        }
        guard !closing, !suspended, !replacing, setupState == .attached,
              initialAdoptionEnabledBefore == nil,
              let backend, let connection = backend.connection else { return }
        guard ownTask == nil, !cancellingConnectionOperation else { connectionFailure = .busy; return }
        let expected = generation, revision = hostRevision
        let ready: CloudConnectionPreview?
        if case .ready(let handle, _) = preview {
            guard let owner = readyPreviewOwner, owner.service === connection,
                  owner.generation == expected, owner.revision == revision else { return }
            ready = handle
        } else { ready = nil }
        let previewOwner = readyPreviewOwner?.id
        let preparation = ResumePreparation()
        connectionFailure = nil
        cancellingConnectionOperation = true
        var completedLocalOnlyOwner: UUID?
        let action = startOwnTask(backend: backend, generation: expected, revision: revision, resumePreparation: preparation) { [self] in
            defer { finishOwnWork(); cancellingConnectionOperation = false }
            guard let originalOwner = ownOwner else { return }
            @MainActor func currentOwner() -> Bool {
                !Task.isCancelled && !closing && !suspended && !replacing && !backendRetired
                    && expected == generation && revision == hostRevision && self.backend === backend
                    && backend.connection === connection && ownOwner == originalOwner && ownTask?.isCancelled == false
            }
            @MainActor func current() -> Bool {
                currentOwner() && ownResumePreparation?.owner == originalOwner && ownResumePreparation?.token === preparation
            }
            do {
                var checkedPreview = ready
                var checkedPreviewOwner = previewOwner
                guard current() else { return }
                let pending = try await connection.recoveryStatus()
                guard current(), readyPreviewOwner?.id == checkedPreviewOwner else { return }
                if let pending {
                    guard !stopOnly, pending.publicationConflictCancellable, let own = backend.own else {
                        throw BridgeError.cloudConnection(.busy)
                    }
                    try await own.cancel(pending.operation)
                    guard current(), readyPreviewOwner?.id == checkedPreviewOwner else { return }
                    let currentRecovery = try await connection.recoveryStatus()
                    guard current(), readyPreviewOwner?.id == checkedPreviewOwner else { return }
                    recovery = currentRecovery
                    guard currentRecovery == nil else { throw BridgeError.cloudConnection(.busy) }
                    preview = nil; readyPreviewOwner = nil; checkedPreview = nil
                    checkedPreviewOwner = nil
                }
                var accepted = false
                while !accepted {
                    let admission: LocalOnlyAdmission = try await withCheckedThrowingContinuation {
                        enqueue(.localOnly(stopOnly: stopOnly, preview: checkedPreview, previewOwner: checkedPreviewOwner,
                            preparation: preparation, backend: backend, service: connection,
                            generation: expected, revision: revision, acknowledged: $0))
                    }
                    switch admission {
                    case .cancelled: return
                    case .accepted: accepted = true
                    case .pending:
                        await prepareConnectionAuthority()
                        // These originals return only after their actual result
                        // ACKs. Never join this local action's ownTask.
                        for original in Array(operationTasks.values) { await original.value }
                    }
                }
                guard currentOwner() else { return }
                preview = nil; readyPreviewOwner = nil
                try await saveEnabled(false)
                guard currentOwner() else { return }
                receive(.refresh(.cloud))
                completedLocalOnlyOwner = originalOwner
            } catch {
                if expected == generation, revision == hostRevision, self.backend === backend,
                   backend.connection === connection, ownOwner == originalOwner, !closing {
                    recordConnectionFailure(error)
                    receive(.refresh(.cloud))
                }
            }
        }
        await action.task.value
        // finishOwnWork released the active slot before this waiter resumed.
        // Supersession still revokes this exact completed action's handoff.
        guard completedLocalOnlyOwner == action.owner, latestOwnOwner == action.owner,
              ownOwner == nil, ownTask == nil, !action.task.isCancelled, !Task.isCancelled,
              !closing, !suspended, !replacing, !backendRetired, !providerActionInProgress,
              expected == generation, revision == hostRevision, self.backend === backend,
              backend.connection === connection, provider != nil, connectionFailure == nil else { return }
        await changeProvider(to: .localOnly)
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
            let confirmationOwner = ownOwner
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
                case .adoption:
                    guard try await prepareOwnEditorReplacement(backend, generation: expected, revision: revision) else {
                        try await connection.cancelPreview(handle)
                        return
                    }
                    // The original task still owns this exact consumed preview.
                    // No acceptance or provider/settings write precedes the guard.
                    try ownEditorReplacement?.decision.admitEffect()
                    _ = try await connection.confirmAdoption(handle)
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
                if intent == .adoption, let provider {
                    let settingsRevision = providerSettingsRevision
                    let acceptedSettings = CloudPolicyDeviceSettings(enabled: true, paused: settings.paused,
                        automatic: settings.automatic && storedAutomatic, media: true)
                    @MainActor func requireCurrentAdoption() throws {
                        guard let confirmationOwner, ownOwner == confirmationOwner, latestOwnOwner == confirmationOwner,
                              ownTask?.isCancelled == false, !Task.isCancelled,
                              expected == generation, revision == hostRevision,
                              self.backend === backend, backend.connection === connection, self.provider === provider,
                              settingsRevision == providerSettingsRevision,
                              !closing, !suspended, !replacing, !settings.paused,
                              !cancellingConnectionOperation, !providerActionInProgress, !providerHandoffInProgress else {
                            throw CancellationError()
                        }
                        if let held = ownEditorReplacement {
                            guard held.owner == confirmationOwner,
                                  editorReplacementIsCurrent?(held.reservation) == true else { throw CancellationError() }
                            try held.decision.admitEffect()
                        } else if prepareEditorReplacement != nil { throw CancellationError() }
                    }
                    try requireCurrentAdoption()
                    let state = try await provider.acceptICloudConnection()
                    try requireCurrentAdoption()
                    guard state.selection == .iCloud, state.permitsICloudStartup,
                          let committed = state.settings, committed == acceptedSettings else {
                        throw BridgeError.unavailable(reason: "Provider settings require recovery")
                    }
                    // Acceptance already persisted consent while CloudAdopt owns
                    // the slot; project it without another generic provider save.
                    providerState = state
                    saveSettings(committed)
                } else {
                    if let provider { providerState = try await provider.acceptICloudConnection() }
                    try await persistSettings(.init(enabled: true, paused: settings.paused,
                        automatic: settings.automatic && storedAutomatic, media: true))
                }
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
    /// The person's Resume of a waiting op (`resume_recovery_op`). It clears
    /// the update hold first (P25.2), then resumes the kind of op that waits.
    public func resumeRecoveryOperation() async {
        guard await clearUpdateHold() else { return }
        if recovery?.adoptionTransportWait == true { resumeWaitingAdoption() } else { resumeConnection() }
    }
    /// V27.5: Off, or a change of service, is refused while an `adopt` or
    /// `publish` op could restore the binding before, and while the op could
    /// not be read. A `publish` op whose switch conflict is set permits it.
    public var serviceChangeRefusedForOperation: Bool {
        Self.serviceChangeRefused(selection: providerState?.selection, operationSwitchConflict: recovery?.switchConflict,
            readFailed: recoveryReadFailed)
    }
    /// `operationSwitchConflict` is nil with no op.
    static func serviceChangeRefused(selection: SyncProviderSelection?, operationSwitchConflict: Bool?, readFailed: Bool) -> Bool {
        guard selection == .iCloud else { return false }
        if readFailed { return true }
        guard let operationSwitchConflict else { return false }
        return !operationSwitchConflict
    }
    /// V27.5's first two rows: an active binding with no op stops; every other permitted state keeps local only.
    static func offStopsBinding(bindingActive: Bool?, operationPresent: Bool) -> Bool {
        bindingActive == true && !operationPresent
    }
    /// V27.5's rows for Off with iCloud selected. Writes nothing when refused.
    @discardableResult public func turnOffICloud() async -> Bool {
        guard providerState?.selection == .iCloud, !serviceChangeRefusedForOperation else { return false }
        if Self.offStopsBinding(bindingActive: status?.controls.bindingActive, operationPresent: recovery != nil) {
            await stopUsingICloud()
        } else { await keepLocalOnly() }
        return providerState?.selection != .iCloud
    }
    /// User action only. Automatic connection/policy wakes cannot release a wait.
    public func resumeWaitingAdoption() {
        guard !closing, !suspended, !replacing, !settings.paused, !awaitingInitialOwnProof,
              initialOwnDrain == nil, !initialActivationReturned, ownTask == nil, !cancellingConnectionOperation,
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
                if resumed {
                    if initialAdoptionActive {
                        guard !closing, !suspended, !replacing, !settings.paused, !initialAdoptionCancelled,
                              !awaitingInitialOwnProof, initialOwnDrain == nil else { return }
                        initialOwnAdmission.setOpen(true)
                    }
                    await runOwnWork(backend, generation: expected, revision: revision)
                }
            } catch {
                if expected == generation, revision == hostRevision, !closing { operationFailed = true }
            }
        }
    }
    public func resumeConnection() {
        guard !closing, !suspended, !replacing, !settings.paused, !awaitingInitialOwnProof,
              initialOwnDrain == nil, !initialActivationReturned, ownTask == nil, !cancellingConnectionOperation, let backend, let connection = backend.connection else { return }
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
    @discardableResult private func startOwnTask(backend original: any SyncCoordinatorBackend, generation expected: UInt64, revision: UInt64,
                              resumePreparation: ResumePreparation? = nil, operation: @escaping @MainActor () async -> Void) -> (owner: UUID, task: Task<Void, Never>) {
        let id = UUID()
        ownOwner = id
        latestOwnOwner = id
        ownResumePreparation = resumePreparation.map { (id, $0) }
        let task = Task { [self] in
            let admission: @Sendable (CloudOriginalCommandProofYield) async throws -> Void = { [self] proof in
                try await admitProof(proof, owner: .own(id), backend: original, generation: expected, revision: revision)
            }
            await CloudOriginalCommandProofYield.$admission.withValue(admission) { await operation() }
        }
        ownTask = task
        return (id, task)
    }
    private func finishOwnWork() {
        if let held = ownEditorReplacement, held.owner == ownOwner {
            let original = ownTask
            let finish = finishCloudEditorReplacement
            ownEditorReplacement = nil
            Task {
                // Never join our own task from inside its activation callback.
                // Add remains excluded through the original terminal and the
                // actual replacement/collection-host completion, if one exists.
                await original?.value
                await waitForReplacement()
                await finish?(held.reservation)
            }
        }
        if let id = ownOwner {
            for wait in proofWaits.values.filter({ $0.owner == .own(id) }) {
                proofWaits.removeValue(forKey: wait.proof.id); wait.proof.finish(CancellationError())
            }
        }
        discoveryAccountRead = nil
        ownResumePreparation = nil; ownOwner = nil; ownTask = nil
        retryAccountCompletion()
        if ownWorkRequested, !cancellingConnectionOperation, !closing, !replacing, !suspended, !settings.paused, !awaitingInitialOwnProof, initialOwnDrain == nil { ownWorkRequested = false; resumeConnection() }
    }
    public func prepareConnectionCancellation() async -> CloudRecoveryCancellation? {
        // `cancel_recovery_op` is a clearing act of the update hold (P25.2).
        guard await clearUpdateHold() else { return nil }
        guard !closing, !suspended, !replacing, !activatingInitialAdoption,
              let backend, let own = backend.own else { return nil }
        let expected = generation, revision = hostRevision
        do {
            let confirmation = try await own.prepareCancellation()
            guard !closing, !suspended, !replacing, expected == generation,
                  revision == hostRevision, self.backend === backend else { return nil }
            recovery = confirmation.recovery
            connectionFailure = nil
            return confirmation
        } catch {
            guard !closing, expected == generation, revision == hostRevision, self.backend === backend else { return nil }
            if case BridgeError.cloudConnection(let failure) = error { connectionFailure = failure }
            return nil
        }
    }
    public func cancelConnectionOperation(confirmation: CloudRecoveryCancellation? = nil) async {
        guard !closing, !suspended, !replacing, !activatingInitialAdoption,
              let backend, let own = backend.own else { return }
        guard let operation = recovery?.operation else {
            if confirmation != nil { connectionFailure = .busy }
            return
        }
        guard !cancellingConnectionOperation else { connectionFailure = .busy; return }
        cancellingConnectionOperation = true
        defer { cancellingConnectionOperation = false }
        let expected = generation, revision = hostRevision
        let originalTask = ownTask
        connectionFailure = nil
        let earlierInitialCancellation = initialAdoptionCancelled
        initialAdoptionCancelled = true
        do {
            // Only the current own task can hold this editor decision. Wake
            // it before the executor joins its original activation callback.
            cancelOwnEditorDecision()
            try await own.cancel(confirmation?.recovery.operation ?? operation, confirmation: confirmation)
            await originalTask?.value
            guard !closing, expected == generation, revision == hostRevision else { return }
            let current = try await backend.connection?.recoveryStatus()
            guard !closing, expected == generation, revision == hostRevision, self.backend === backend else { return }
            recovery = current
            await reconcileInitialAdoption(backend)
            receive(.refresh(.cloud))
        } catch {
            guard !closing, expected == generation, revision == hostRevision, self.backend === backend else { return }
            initialAdoptionCancelled = earlierInitialCancellation
            #if DEBUG
            DebugCloudDiagnostics.record(.connectionOperationCancel, error: error)
            #endif
            if case BridgeError.cloudConnection(let failure) = error { connectionFailure = failure }
            operationFailed = true
        }
    }
    private func saveEnabled(_ enabled: Bool) async throws {
        try await persistSettings(.init(enabled: enabled, paused: settings.paused,
            automatic: settings.automatic && storedAutomatic, media: true))
    }
    private func restoreInitialAdoptionEnabled() async {
        guard let earlier = initialAdoptionEnabledBefore else { return }
        initialAdoptionEnabledBefore = nil
        initialOwnResumeRead = nil
        awaitingInitialOwnProof = false
        do { try await saveEnabled(earlier) }
        catch { markProviderBlocked(.adoptionRestore, error); operationFailed = true }
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
            else if usable == false { await restoreInitialAdoptionEnabled() }
        } catch { /* Unknown authority preserves the accepted operation's setting. */ }
    }
    private func willActivateInitialAdoption(generation expected: UInt64, revision: UInt64) throws {
        guard !closing, !suspended, !replacing, !settings.paused, !initialAdoptionCancelled,
              !awaitingInitialOwnProof, initialOwnDrain == nil,
              ownTask?.isCancelled == false, expected == generation, revision == hostRevision else { throw CancellationError() }
        activatingInitialAdoption = true
    }
    private func prepareOwnEditorReplacement(_ original: any SyncCoordinatorBackend,
                                            generation expected: UInt64, revision: UInt64) async throws -> Bool {
        guard let taskOwner = ownOwner else { throw CancellationError() }
        let current: @MainActor @Sendable () -> Bool = {
            self.ownOwner == taskOwner && self.latestOwnOwner == taskOwner && self.ownTask?.isCancelled == false &&
                self.backend === original && self.generation == expected && self.hostRevision == revision &&
                !self.closing && !self.suspended && !self.replacing && !self.settings.paused &&
                !self.cancellingConnectionOperation && !Task.isCancelled
        }
        guard current() else { throw CancellationError() }
        if let held = ownEditorReplacement {
            guard held.owner == taskOwner, editorReplacementIsCurrent?(held.reservation) == true else { throw CancellationError() }
            return true
        }
        // Standalone coordinators have no collection editor. Production owners
        // always configure this seam, without constructing an absent Add flow.
        guard let prepareEditorReplacement else { return true }
        let decision = ReviewCollection.EditorReplacementDecision()
        ownEditorReplacement = (taskOwner, decision.id, decision)
        guard let admission = await prepareEditorReplacement("iCloud Adoption", decision, current) else { return false }
        guard admission.accepted else { return false }
        guard current(), editorReplacementIsCurrent?(admission.id) == true else {
            throw CancellationError()
        }
        return true
    }
    private func willActivateAdoption(_ original: any SyncCoordinatorBackend,
                                      generation expected: UInt64, revision: UInt64) async throws {
        guard try await prepareOwnEditorReplacement(original, generation: expected, revision: revision) else { throw CancellationError() }
        if initialAdoptionEnabledBefore != nil {
            try willActivateInitialAdoption(generation: expected, revision: revision)
        } else {
            // The existing replacement join must protect this local activation
            // too, rather than cancelling its original task on its own event.
            activatingInitialAdoption = true
        }
    }
    private func didActivateAdoption(generation expected: UInt64, initial: Bool) async {
        defer { activatingInitialAdoption = false }
        if initial, case .ready(let actual) = try? await CoreCollectionService().lifecycle(), actual > expected {
            initialActivationReturned = true
        }
    }
    private func runOwnWork(_ backend: any SyncCoordinatorBackend, generation expected: UInt64, revision: UInt64,
                            onFailure: (@MainActor (CloudConnectionFailure?) -> Void)? = nil) async {
        guard let own = backend.own else { return }
        let executorOwner = ownOwner
        do {
            while !closing, !Task.isCancelled, revision == hostRevision {
                await waitForInputs()
                guard !closing, !Task.isCancelled, !replacing, !suspended,
                      expected == generation, revision == hostRevision, self.backend === backend else { return }
                let performCore: @MainActor @Sendable (@escaping @Sendable () async throws -> Void) async throws -> Void = { [self] operation in
                    try await self.performOwnCore(operation, backend: backend, generation: expected, revision: revision,
                        beforeEffect: {
                            guard self.activatingInitialAdoption else { return }
                            guard self.ownOwner == executorOwner, self.latestOwnOwner == executorOwner,
                                  self.ownTask?.isCancelled == false, self.backend === backend,
                                  self.generation == expected, self.hostRevision == revision,
                                  !self.closing, !self.replacing, !self.suspended, !self.settings.paused,
                                  !self.cancellingConnectionOperation, !Task.isCancelled else { throw CancellationError() }
                            if let held = self.ownEditorReplacement {
                                guard held.owner == executorOwner,
                                      self.editorReplacementIsCurrent?(held.reservation) == true else { throw CancellationError() }
                                try held.decision.admitEffect()
                            } else if self.prepareEditorReplacement != nil { throw CancellationError() }
                        })
                }
                let progress: CloudConnectionProgress
                if initialAdoptionEnabledBefore != nil {
                    guard !initialAdoptionCancelled, !settings.paused, !suspended, !awaitingInitialOwnProof else { return }
                    progress = try await own.runNext(admission: initialOwnAdmission,
                        performCore: performCore,
                        beforeActivation: { [self] in try await self.willActivateAdoption(backend, generation: expected, revision: revision) },
                        afterActivation: { [self] in await self.didActivateAdoption(generation: expected, initial: true) })
                } else {
                    progress = try await own.runNext(performCore: performCore,
                        beforeActivation: { [self] in try await self.willActivateAdoption(backend, generation: expected, revision: revision) },
                        afterActivation: { [self] in await self.didActivateAdoption(generation: expected, initial: false) })
                }
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
                                backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                                beforeEffect: @MainActor () throws -> Void = {}) async throws {
        let id = UUID()
        try await withCheckedThrowingContinuation {
            enqueue(.ownCore(id, backend: backend, generation: generation, revision: revision, acknowledged: $0))
        }
        // Run on the original executor task, retaining its job cancellation and
        // exact outcome. Only local Core work borrows the input lane's phase.
        let outcome: Result<Void, any Error>
        do { try beforeEffect(); try await operation(); outcome = .success(()) }
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
        if providerState?.selection == .meguri {
            requestMeguriWake(automatic: true)
            resumeMeguriMediaIfNeeded(.foreground)
        }
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
        if !settings.paused, !awaitingInitialOwnProof, initialOwnDrain == nil { initialOwnAdmission.setOpen(true) }
        receive(.settings(settings))
        enqueue(.replayVisibleMedia)
        if let reachable { receive(.network(reachable: reachable)) }
        if pendingExpiry { pendingExpiry = false; receive(.backgroundExpiring) }
        if pendingLocalRefresh { pendingLocalRefresh = false; receive(.refresh(.local)) }
        reportAvailableSpace(); receive(.foreground)
        if initialAdoptionEnabledBefore != nil, !initialAdoptionCancelled { resumeConnection() }
    }
    @discardableResult func backgroundAfterLocalFlush(deadlineMilliseconds: Int64?) async -> Bool {
        cancelOwnEditorDecision()
        cancelMeguriEditorDecision(meguriAction?.id)
        await replacementTask?.value
        guard !replacing else { return false }
        if initialAdoptionEnabledBefore != nil { initialOwnAdmission.setOpen(false) }
        if providerState?.selection == .meguri {
            suspended = true
            await drainMeguriProduct()
        }
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
            cancelCommands(.expiry)
            latestOwnOwner = nil
            cancelOwnEditorDecision()
            ownTask?.cancel()
            receive(.backgroundExpiring)
        }
    }

    // The application's existing quit owner supplies its completion. No native
    // library terminates the application or invents a quit owner.
    func quitAfterLocalFlush(completion: @escaping @MainActor () -> Void) async {
        cancelOwnEditorDecision()
        cancelMeguriEditorDecision(meguriAction?.id)
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
        initialOwnDrain == nil && operationTasks.keys.allSatisfy { id in
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
        for wait in proofWaits.values {
            if let id = wait.owner?.commandID, operationTasks[id]?.isCancelled == true,
               let context = operationContexts[id] {
                wait.proof.revoke(cancellation: context.cancellationIntent)
            } else { wait.proof.finish(CancellationError()) }
        }
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
            case .localOnly: observeCausalBoundary?(.localOnlyPolicyWaiting, nil)
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
            // This read's own CancelExchange/StopEngine must not replace its
            // exact unsuccessful proof at the original native latch. Keep the
            // yield pending until outputs return; scheduling finish earlier
            // alone would still race native cancellation.
            if let error = result.error, result.afterOutputs == nil,
               !closing, !suspended, !replacing, !backendRetired, accountTask?.isCancelled == false,
               expected == generation, revision == hostRevision, backend === original,
               control === accountTaskControl, pendingAccountRead.map({ $0 != read }) != true {
                for wait in proofWaits.values where wait.read == read && containsProofOwner(wait.owner) {
                    wait.proof.sealFailureBeforeOutputs(error)
                }
            }
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
            if discoveryAccountRead?.read == read, ownResumePreparation != nil,
               expected == generation, revision == hostRevision, backend === original,
               !closing, !suspended, !replacing {
                discoveryAccountRead?.result = result
            }
            if result.error == nil {
                await reduce(.ownAccountProof(read, generation: expected, revision: revision, verified: result.accountVerified))
            }
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
        case .discoveryPrepared(let preparation, let original, let expected, let revision, let acknowledged):
            guard !closing, !suspended, !replacing, !settings.paused, ownTask?.isCancelled == false,
                  expected == generation, revision == hostRevision, backend === original,
                  ownResumePreparation?.owner == ownOwner, ownResumePreparation?.token === preparation else {
                acknowledged.resume(throwing: CancellationError()); return
            }
            let queuedResult = inputQueue.contains { if case .result = $0 { return true }; return false }
            guard operationTasks.isEmpty, accountTask == nil, pendingAccountRead == nil, !queuedResult else {
                acknowledged.resume(returning: false); return
            }
            guard let result = discoveryAccountRead?.result else {
                acknowledged.resume(throwing: BridgeError.unavailable(reason: "Cloud account proof unavailable")); return
            }
            if let error = result.error { acknowledged.resume(throwing: error); return }
            guard result.accountVerified else {
                acknowledged.resume(throwing: BridgeError.unavailable(reason: "Cloud account proof unavailable")); return
            }
            // Revoke the pre-work account admission on its ordered lane.
            // Preview uses Core's real resulting account/collection guards.
            discoveryAccountRead = nil
            ownResumePreparation = nil
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
        case .localOnly(let stopOnly, let handle, let previewOwner, let preparation, let original, let service, let expected, let revision, let acknowledged):
            guard !closing, !suspended, !replacing, !backendRetired, ownTask?.isCancelled == false,
                  expected == generation, revision == hostRevision, backend === original, original.connection === service,
                  ownResumePreparation?.owner == ownOwner, ownResumePreparation?.token === preparation,
                  readyPreviewOwner?.id == previewOwner else {
                acknowledged.resume(returning: .cancelled)
                return
            }
            if let owner = readyPreviewOwner {
                guard owner.service === service, owner.generation == expected, owner.revision == revision,
                      case .ready = preview else {
                    acknowledged.resume(returning: .cancelled)
                    return
                }
            }
            let queuedResult = inputQueue.contains { if case .result = $0 { return true }; return false }
            guard operationTasks.isEmpty, accountTask == nil, pendingAccountRead == nil, !queuedResult else {
                #if DEBUG
                observeCausalBoundary?(.localOnlyPolicyWaiting, nil)
                #endif
                acknowledged.resume(returning: .pending)
                return
            }
            // Both mutations rotate Core's guard. Keep later inputs queued
            // through the actual call, after all original result/ACK owners end.
            ownResumePreparation = nil
            do {
                if stopOnly { try await original.stopSyncing() }
                else { try await service.keepLocalOnly(generation: expected, preview: handle) }
                acknowledged.resume(returning: .accepted)
            } catch { acknowledged.resume(throwing: error) }
        case .result(let result, let expected, let revision, let shutdownCancelled, let supersededAccountCancellation, let nativeCancellationOwned, let acknowledged):
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
                if let error = result.error, !(closing && shutdownCancelled && error is CancellationError && result.fetchProofOutcome?.preservesFailure != true) {
                    #if DEBUG
                    if let failure = result.diagnosticFailure {
                        DebugCloudDiagnostics.record(failure)
                    } else {
                        DebugCloudDiagnostics.record(.commandResult, error: error)
                    }
                    #endif
                    if result.originalGuardRefusal == nil, result.dependentFetchRefusal == nil, result.fetchProofOutcome?.accountStopCancellation != true, !supersededAccountCancellation, !nativeCancellationOwned, !(result.mediaCancellation && error is CancellationError) {
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
            providerPlatform = nil
            if desiredVisibleMedia?.generation != next { desiredVisibleMedia = nil }
            deliveredVisibleMedia = nil
            generation = next; status = nil; preview = nil; recovery = nil
            retainedRecoveryStart = nil
            hasDeferredNativeWork = false
            hostRevision &+= 1
            providerSettingsRevision &+= 1
            providerRecoveryDrain = nil; desiredProviderSettings = nil
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
        case .ownAccountProof(let read, let expected, let revision, let verified):
            guard read == initialOwnResumeRead, expected == generation, revision == hostRevision,
                  initialAdoptionActive, !closing, !suspended, !replacing, !settings.paused, initialOwnDrain == nil,
                  let backend, let connection = backend.connection else { return }
            do {
                // A successful account read can name another account. Ask Core
                // to revalidate the retained adoption before reopening native
                // admission. Enumeration claims no adoption task; the executor
                // will obtain and execute its work normally after admission.
                let work = try await connection.nextPublicationWork()
                guard read == initialOwnResumeRead, expected == generation, revision == hostRevision,
                      self.backend === backend, !closing, !suspended, !replacing,
                      !settings.paused, !initialAdoptionCancelled, initialOwnDrain == nil else { return }
                if verified, case .adoption = work {
                    initialOwnResumeRead = nil
                    awaitingInitialOwnProof = false
                    initialOwnAdmission.setOpen(true)
                    resumeConnection()
                } else {
                    // Core settles account loss/mismatch without queueing or
                    // installing anything. An explicit transport Resume still
                    // has to pass its own Core authority check before reopening.
                    await reconcileInitialAdoptionOnInput(backend)
                    if verified, read == initialOwnResumeRead, expected == generation, revision == hostRevision,
                       self.backend === backend, !closing, !suspended, !replacing, !settings.paused,
                       initialOwnDrain == nil, recovery?.adoptionTransportWait == true {
                        initialOwnResumeRead = nil
                        awaitingInitialOwnProof = false
                    }
                }
            } catch {
                guard read == initialOwnResumeRead, expected == generation, revision == hostRevision,
                      self.backend === backend, !closing, !suspended, !replacing,
                      !settings.paused, !initialAdoptionCancelled, initialOwnDrain == nil else { return }
                operationFailed = true
            }
        case .account(let transition):
            guard providerAllowsAccountRead else { return }
            guard !closing, !replacing, !suspended, let backend else { return }
            if transition, initialAdoptionActive, !activatingInitialAdoption {
                awaitingInitialOwnProof = true
                drainInitialOwnWork()
            }
            do {
                // Establish Rust's read generation on the ORDERED input lane,
                // before awaiting any network resolution. A later signal makes
                // an earlier in-flight reply stale immediately.
                pendingAccountRead = try await backend.beginAccountRead(transition: transition)
                if discoveryAccountRead != nil, ownResumePreparation != nil {
                    discoveryAccountRead = (pendingAccountRead, nil)
                }
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
                if discoveryAccountRead != nil, ownResumePreparation != nil {
                    discoveryAccountRead = (nil, .init(outputs: [], error: error, deferred: false))
                }
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
        if case .network(let value) = event {
            let recovered = reachable == false && value
            reachable = value
            if recovered {
                meguriReachabilityRecoveryRevision &+= 1
                requestMeguriWake(automatic: true)
                resumeMeguriMediaIfNeeded(.network)
            }
        }
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
            if case .visibleMedia = event, backend.connection != nil {
                // A local-only Host has no adapter binding: its guard's
                // generation is zero. Retain desired names until real Bind,
                // rather than issuing a native command that cannot qualify.
                let authority = try await CoreCloudWorkflowService().currentGuard()
                guard expected == generation, revision == hostRevision, self.backend === backend,
                      !closing, !suspended, !replacing, !backendRetired,
                      mediaEnabledRevision == revision,
                      authority.generation == expected, !authority.binding.isEmpty else { return }
            }
            let output = try await backend.input(event)
            guard expected == generation, revision == hostRevision, !replacing else { return }
            if case .settings(let value) = event {
                mediaEnabledRevision = value.media ? revision : nil
                if !value.media { deliveredVisibleMedia = nil }
            }
            if case .visibleMedia(let names) = event { deliveredVisibleMedia = (revision, names) }
            consume(output, generation: expected)
            if case .settings(let value) = event, value.media { await replayVisibleMedia() }
            if case .refresh = event { await replayVisibleMedia() }
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
        let context = SyncCommandTaskContext(generation: generation, revision: revision, backend: backend,
                                             originalCommands: issued.map(\.command))
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
                for (index, value) in issued.enumerated() {
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
                    context.activeCommandIndex = index
                    result = await CloudPolicyCancellationContext.$current.withValue(context.cancellationIntent) {
                    await CloudQuiescentDependencyWait.$transition.withValue(park) {
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
                                diagnosticStatus.observe(snapshot, generation: value.generation, revision: revision, record: recordDiagnostic)
                                #if DEBUG
                                DebugCloudDiagnostics.recordStatus(snapshot.error)
                                DebugCloudDiagnostics.recordPolicyStatus(snapshot, generation: value.generation, hostRevision: revision)
                                #endif
                            }
                        case .recordExchange(let exchange):
                            // Core can return a typed failure inside an output.
                            // Only the original completed/admitted action counts.
                            let rejected = result.outputs.contains { output in
                                if case .commands = output { false } else { true }
                            }
                            if !closing, value.generation == generation, !result.deferred, !rejected {
                                FieldDiagnosticCloudStatus.completed(exchange, record: recordDiagnostic)
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
                                shutdownCancelled: context.shutdownCancelled,
                                nativeCancellationOwned: context.ownsNativeCancellation(result, command: command,
                                    backend: backend, generation: value.generation, revision: revision), acknowledged: acknowledged))
                        }
                        observeCausalBoundary?(.commandAcknowledgementJoined, command)
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
        cancelCommands(.replacement)
        cancelAccountTask(shutdown: false)
        revokeProofWaits()
        await initialOwnDrain?.value
        // Activation is a local Rust call, with no held SDK callback. Waiting
        // for that call cannot wait for this replacement task. All transport
        // work and unrelated replacements use the ordinary cancel/drain path.
        if activatingInitialAdoption { await ownTask?.value }
        else { cancelOwnEditorDecision(); ownTask?.cancel() }
        do {
            await providerSettingsTask?.value
            meguriWakePending = false
            let originalWake = meguriWake
            originalWake?.task.cancel()
            await meguriConnectionOwner?.cancelAndDrain()
            await originalWake?.task.value
            meguriReady = false; meguriBootstrap = nil; meguriRound = nil; meguriOperation = nil
            try await backend?.quiesce()
            await accountTask?.value
            await ownTask?.value
            await drainOperations()
            try await backend?.close()
            backendRetired = true
            backend = nil
            provider = nil; providerDiscovery = nil; meguriConnectionOwner = nil
            providerHandoff = nil; providerHandoffDestination = nil
            readyPreviewOwner = nil
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
                try await attachProvider(to: successor)
                guard !closing, expected == generation, revision == hostRevision else {
                    try await successor.close()
                    backend = nil
                    continue
                }
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
        await providerSettingsTask?.value
        await withCheckedContinuation { enqueue(.barrier($0)) }
    }
    // Internal so real-command fixtures can join execution and result ACKs.
    func drainOperations() async {
        repeat {
            let originals = Array(operationTasks.values)
            for task in originals { await task.value }
            #if DEBUG
            let drainingOwner = ownOwner
            if let drainingOwner { observeCausalBoundary?(.ownDrainWaiting(drainingOwner), nil) }
            #endif
            await ownTask?.value
            #if DEBUG
            if let drainingOwner { observeCausalBoundary?(.ownDrainJoined(drainingOwner), nil) }
            #endif
            await waitForInputs()
        } while !operationTasks.isEmpty || ownTask != nil
    }
    private func cancelCommands(_ intent: SyncCommandTaskContext.Cancellation) {
        let event = UUID()
        // Record the common request on every original before any onCancel can
        // run. Dictionary iteration order must not choose the native cause.
        for (id, task) in operationTasks {
            if let context = operationContexts[id] {
                let originalOwner = context.generation == generation && context.revision == hostRevision
                context.recordCancellation(intent == .shutdown && !originalOwner ? .other : intent,
                    alreadyCancelled: task.isCancelled, event: event)
            }
        }
        for task in operationTasks.values { task.cancel() }
    }
    private func cancelCommandsForShutdown() { cancelCommands(.shutdown) }

    // Local entry must finish original completion/ACK/refresh before Core's
    // exclusive activation can invalidate the original generation.
    func beginEntryActivation() {
        guard !entryActivationHeld else { return }
        providerPlatform = nil
        providerRecoveryDrain = nil
        entryActivationHeld = true
        closing = true
        cancelOwnEditorDecision()
        cancelMeguriEditorDecision(meguriAction?.id)
        cancelCommands(.replacement)
        cancelAccountTask(shutdown: true)
        revokeProofWaits()
    }
    func drainEntryActivation() async -> Bool {
        guard entryActivationHeld else { return false }
        return await cancelAndDrain(entryActivation: true)
    }
    @discardableResult func releaseEntryActivation() throws -> Bool {
        guard entryActivationHeld else { return false }
        guard drained, closeTask == nil else { throw BridgeError.busy(runningOperation: "iCloud drain") }
        entryActivationHeld = false
        closing = false; drained = false; replacing = false
        if platformPrepared { setupState = .prepared }
        return true
    }

    /// Stops native admissions and joins original SDK/own/media callbacks while
    /// Core is OPEN. A failed drain leaves the backend retained for close retry.
    func cancelAndDrain() async -> Bool {
        providerPlatform = nil
        providerRecoveryDrain = nil
        return await cancelAndDrain(entryActivation: false)
    }
    func drainProviderAttachment() async -> Bool {
        guard providerAttachmentPending else { return false }
        return await drainProviderHost()
    }
    private func drainProviderHost() async -> Bool {
        if platformPrepared, retainedHostAdmitted, !replacing, !entryActivationHeld, let provider {
            providerPlatform = (generation, provider)
        }
        guard providerPlatform?.generation == generation else { return false }
        return await cancelAndDrain(entryActivation: false)
    }
    private func cancelAndDrain(entryActivation: Bool) async -> Bool {
        if let closeTask { return await closeTask.value }
        if drained {
            if !entryActivation { platformPrepared = false; entryActivationHeld = false }
            return true
        }
        closing = true
        cancelOwnEditorDecision()
        cancelMeguriEditorDecision(meguriAction?.id)
        // Cancellation belongs to the original commands, before revoking an
        // inner proof can report CancellationError as a provider failure.
        if entryActivation { cancelCommands(.replacement) } else { cancelCommandsForShutdown() }
        cancelAccountTask(shutdown: true)
        revokeProofWaits()
        retainedRecoveryStart = nil
        initialOwnAdmission.setOpen(false)
        let task = Task { () -> Bool in
            await providerSettingsTask?.value
            await drainMeguriProduct()
            await meguriConnectionOwner?.cancelAndDrain()
            await attachmentTask?.value
            await replacementTask?.value
            await initialOwnDrain?.value
            await initialOwnResume?.value
            networkTask?.cancel(); accountEventsTask?.cancel(); networkMonitor?.cancel()
            if let accountObserver { NotificationCenter.default.removeObserver(accountObserver); self.accountObserver = nil }
            flushTimer?.invalidate(); flushTimer = nil
            // A core-phase barrier joins original results. Cancel their jobs
            // before joining it, including queued work that has no effect yet.
            if entryActivation { cancelCommands(.replacement) } else { cancelCommandsForShutdown() }
            cancelAccountTask(shutdown: true); ownTask?.cancel()
            retryAccountCompletion()
            await waitForInputs()
            if entryActivation { cancelCommands(.replacement) } else { cancelCommandsForShutdown() }
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
                providerDiscovery = nil
                desiredVisibleMedia = nil; deliveredVisibleMedia = nil
                hasDeferredNativeWork = false
                hostRevision &+= 1; drained = true; retainedHostAdmitted = false
                if !entryActivation { platformPrepared = false; entryActivationHeld = false }
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

extension SyncCoordinator {
    public enum RenderState: String, Equatable, CaseIterable, Sendable {
        case off, meguriConnected, meguriSignIn, meguriNeedsYou, iCloudSelected, iCloudConnected, iCloudNeedsYou, iCloudIssues
        /// N23's panel states (round 10).
        case meguriSyncing, meguriSetupMedia, meguriRoundFailed, meguriBlocked, meguriSignInRefused, iCloudSyncing
        case meguriPreparing, iCloudNoAccount, meguriMediaBlocked, meguriAcceptanceUnknown, iCloudSetupNeeded
        /// MP6.5 (resume fix): setup's media step paused, and stopped after its automatic attempts.
        case meguriSetupMediaPaused, meguriSetupMediaStopped
    }
    /// Builds synthetic Sync section examples. The coordinator is never
    /// attached: it owns no collection, account, binding or server, and its
    /// actions find nothing to act on.
    public static func renderFixture(_ state: RenderState, clock: AppClock = .system) -> SyncCoordinator {
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in throw SyncOwnerFailure.configurationUnavailable },
            defaults: nil, clock: clock)
        sync.applyRenderFixture(state)
        return sync
    }
}
