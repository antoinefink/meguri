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
protocol SyncCoordinatorBackend: AnyObject {
    var connection: CoreCloudConnectionService? { get }
    var own: CloudOwnExecutor? { get }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult
    func beginAccountRead(transition: Bool) async throws -> UInt64
    func readAccount(_ read: UInt64) async -> SyncCommandResult
    // Resolve only. The completion commits proof only when admitted == true;
    // cancellation still returns its original failure through result admission.
    func prepareAccountRead(_ read: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult
    func takeAccountReadRequest() async -> Bool
    func freezeForSuspend() async throws
    func resumeAfterCoreResume() async
    func quiesce() async throws
    func close() async throws
}
extension SyncCoordinatorBackend {
    // Transport-only test results already completed; retain their failures
    // even when admission closes. Native backends defer the actual proof.
    func prepareAccountRead(_ read: UInt64) async -> @MainActor (Bool) async -> SyncCommandResult {
        let result = await readAccount(read)
        return { _ in result }
    }
    func takeAccountReadRequest() async -> Bool { false }
    func freezeForSuspend() async throws {}
    func resumeAfterCoreResume() async {}
}
nonisolated struct SyncCommandResult: Sendable {
    let outputs: [CloudPolicyOutput]
    let error: (any Error)?
    let deferred: Bool
    let accountVerified: Bool
    let mediaCancellation: Bool
    init(outputs: [CloudPolicyOutput], error: (any Error)?, deferred: Bool, accountVerified: Bool = false, mediaCancellation: Bool = false) {
        self.outputs = outputs; self.error = error; self.deferred = deferred; self.accountVerified = accountVerified
        self.mediaCancellation = mediaCancellation
    }
}
enum SyncOwnerFailure: Error { case configurationUnavailable }

// Inherited by native child work, but marked only when the coordinator cancels
// its retained original command task for shutdown. Account reads have no context.
@MainActor final class SyncCommandTaskContext {
    @TaskLocal nonisolated static var current: SyncCommandTaskContext?
    let generation: UInt64
    let revision: UInt64
    var shutdownCancellationRequested = false
    var shutdownCancelled: Bool { shutdownCancellationRequested && Task.isCancelled }
    init(generation: UInt64, revision: UInt64) { self.generation = generation; self.revision = revision }
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
        case core(CoreEvent)
        case lifecycle(CollectionLifecycle)
        case replaced(UInt64)
        case persisted(revision: UInt64)
        case account(Bool)
        case ownAccountProof(UInt64, generation: UInt64, revision: UInt64)
        case result(SyncCommandResult, generation: UInt64, revision: UInt64, shutdownCancelled: Bool,
                    supersededAccountCancellation: Bool = false, acknowledged: CheckedContinuation<Void, Never>?)
        case completeAccount(UInt64, backend: any SyncCoordinatorBackend, generation: UInt64, revision: UInt64,
                             complete: @MainActor (Bool) async -> SyncCommandResult,
                             acknowledged: CheckedContinuation<Bool, Never>)
        case resumeBinding(CloudConnectionPreview, service: CoreCloudConnectionService, generation: UInt64, revision: UInt64,
                           acknowledged: CheckedContinuation<ResumeAdmission, any Error>)
        case barrier(CheckedContinuation<Void, Never>)
    }
    private enum ResumeAdmission { case pending, cancelled, resumed }
    #if DEBUG
    enum CausalBoundary {
        case commandResultEnqueue, commandAcknowledgementJoined
        case proofResultReduced, proofAcknowledgementSending
    }
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
    @ObservationIgnored private let bootstrap: @MainActor () async throws -> Void
    @ObservationIgnored private let makeBackend: @MainActor (@escaping @Sendable () async -> Void) async throws -> any SyncCoordinatorBackend
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let clock: AppClock
    @ObservationIgnored private let directory: URL?
    @ObservationIgnored private let observeSignals: Bool
    private var platformPrepared = false
    private var platformTask: Task<Void, Never>?
    private var backend: (any SyncCoordinatorBackend)?
    private var inputQueue: [Input] = []
    private var inputTask: Task<Void, Never>?
    private var operationTasks: [UUID: Task<Void, Never>] = [:]
    private var operationContexts: [UUID: SyncCommandTaskContext] = [:]
    private var backendRetired = false
    private var replacementTask: Task<Void, Never>?
    private var replacing = false
    private var suspended = false
    private var awaitingResumeProof = false
    private var pendingPreviewCancellation = false
    private var pendingExpiry = false
    private var pendingLocalRefresh = false
    private var reachable: Bool?
    private var quitCompletion: (@MainActor () -> Void)?
    private var accountTask: Task<Void, Never>?
    private var pendingAccountRead: UInt64?
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
            catch { setupState = .failed; operationFailed = true }
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
        if backend != nil || replacing { enqueue(.replaced(generation)); return }
        hostRevision &+= 1
        let revision = hostRevision
        self.generation = generation
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
                enqueue(.policy(.refresh(.cloud)))
                enqueue(.account(false))
                if observeSignals { observePlatformSignals() }
            } catch { operationFailed = true; setupState = .failed }
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
            recovery = status
            if status != nil { retainedRecoveryStart = (backend, generation, revision) }
        case .failure:
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
    public func visibleMedia(_ names: [String]) { receive(.visibleMedia(names: names)) }
    /// Discovery is read-only. Only Rust's explicit no-file directive transfers
    /// the ready authority to a separate, app-owned adoption operation.
    public func beginConnectionPreview() {
        guard !closing, !suspended, !replacing, !settings.paused, setupState == .attached, initialAdoptionEnabledBefore == nil, ownTask == nil, preview == nil, let backend, let own = backend.own else { return }
        let expected = generation, revision = hostRevision
        connectionFailure = nil
        readingPreview = true
        ownTask = Task {
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
        ownTask = Task {
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
                    guard try await resumeBindingAfterPolicyDrain(handle, service: connection,
                        generation: expected, revision: revision) else {
                        try await connection.cancelPreview(handle)
                        return
                    }
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
                    enqueue(.account(false))
                    await prepareConnectionAuthority()
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
    private func resumeBindingAfterPolicyDrain(_ handle: CloudConnectionPreview, service: CoreCloudConnectionService,
                                              generation expected: UInt64, revision: UInt64) async throws -> Bool {
        while true {
            let admission: ResumeAdmission = try await withCheckedThrowingContinuation {
                enqueue(.resumeBinding(handle, service: service, generation: expected, revision: revision, acknowledged: $0))
            }
            switch admission {
            case .resumed: return true
            case .cancelled: return false
            case .pending:
                observeResumePolicyDrain?(.waiting)
                await prepareConnectionAuthority()
                // Do not join ownTask: it owns this confirmation. Results and
                // their follow-up commands still run on the ordinary input lane.
                for original in Array(operationTasks.values) { await original.value }
            }
        }
    }
    public func resumeConnection() {
        guard !closing, !suspended, !replacing, !settings.paused, !awaitingInitialOwnProof,
              initialOwnDrain == nil, !initialActivationReturned, ownTask == nil, let backend, let connection = backend.connection else { return }
        let expected = generation, revision = hostRevision
        ownTask = Task {
            defer { finishOwnWork() }
            do {
                if initialAdoptionEnabledBefore == nil { try await connection.trigger() }
                await runOwnWork(backend, generation: expected, revision: revision)
            }
            catch { if expected == generation, !closing { operationFailed = true } }
        }
    }
    private func finishOwnWork() {
        ownTask = nil
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
        } catch { operationFailed = true }
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
                let progress: CloudConnectionProgress
                if initialAdoptionEnabledBefore != nil {
                    guard !initialAdoptionCancelled, !settings.paused, !suspended, !awaitingInitialOwnProof else { return }
                    progress = try await own.runNext(admission: initialOwnAdmission,
                        beforeActivation: { [self] in try await self.willActivateInitialAdoption(generation: expected, revision: revision) },
                        afterActivation: { [self] in await self.didActivateInitialAdoption(generation: expected) })
                } else { progress = try await own.runNext() }
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
            if revision == hostRevision, !replacing { await reconcileInitialAdoption(backend) }
        }
    }
    func foregroundAfterLocalResume() async {
        guard !closing, !replacing else { return }
        let expected = generation, revision = hostRevision
        let needsProof = suspended || awaitingResumeProof
        await backend?.resumeAfterCoreResume()
        guard !closing, !replacing, expected == generation, revision == hostRevision else { return }
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
        accountTask?.cancel()
        await waitForInputs()
        do {
            try await backend?.freezeForSuspend()
            return true
        } catch { suspended = false; await backend?.resumeAfterCoreResume(); operationFailed = true; return false }
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
        await drainOperations()
    }

    static func changeCause(_ source: OpSource) -> CloudPolicyChangeCause {
        switch source {
        case .cloudIncoming, .cloudRekey: .cloud
        default: .local
        }
    }

    private func enqueue(_ input: Input) {
        inputQueue.append(input)
        guard inputTask == nil else { return }
        inputTask = Task {
            defer { inputTask = nil }
            await SyncCommandTaskContext.$current.withValue(nil) {
                while !inputQueue.isEmpty { await reduce(inputQueue.removeFirst()) }
            }
        }
    }
    private func reduce(_ input: Input) async {
        switch input {
        case .barrier(let continuation): continuation.resume()
        case .completeAccount(let read, let original, let expected, let revision, let complete, let acknowledged):
            guard !closing, !suspended, !replacing, accountTask?.isCancelled == false,
                  expected == generation, revision == hostRevision, backend === original else {
                let result = await complete(false)
                await reduce(.result(result, generation: expected, revision: revision, shutdownCancelled: false, acknowledged: nil))
                acknowledged.resume(returning: true)
                return
            }
            guard operationTasks.isEmpty, !inputQueue.contains(where: { if case .result = $0 { true } else { false } }) else {
                acknowledged.resume(returning: false)
                return
            }
            // Lookup has returned, but no proof has been committed yet. Only
            // this lane may rotate authority, after all original command ACKs.
            observeAccountPolicyDrain?(.finished)
            let result = await complete(true)
            // A different read already begun on this same owner is the exact
            // supersession witness. Keep its predecessor's error and outputs;
            // only the generic failure presentation changes for cancellation.
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
            if result.accountVerified { await reduce(.ownAccountProof(read, generation: expected, revision: revision)) }
            if result.error == nil, result.accountVerified {
                startRetainedRecoveryAfterProof(original, generation: expected, revision: revision)
            }
            #if DEBUG
            observeCausalBoundary?(.proofAcknowledgementSending, nil)
            #endif
            acknowledged.resume(returning: true)
        case .resumeBinding(let handle, let service, let expected, let revision, let acknowledged):
            guard !closing, !suspended, !replacing, !settings.paused, ownTask?.isCancelled == false,
                  expected == generation, revision == hostRevision, backend?.connection === service else {
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
            do {
                try await service.resumeBinding(handle)
                acknowledged.resume(returning: .resumed)
            } catch { acknowledged.resume(throwing: error) }
        case .result(let result, let expected, let revision, let shutdownCancelled, let supersededAccountCancellation, let acknowledged):
            if expected == generation, revision == hostRevision, !replacing, !suspended {
                for output in result.outputs {
                    consume(output, generation: expected)
                }
                if result.deferred { hasDeferredNativeWork = true }
                if let error = result.error, !(closing && shutdownCancelled && error is CancellationError) {
                    #if DEBUG
                    DebugCloudDiagnostics.record(.commandResult, error: error)
                    #endif
                    if !supersededAccountCancellation, !(result.mediaCancellation && error is CancellationError) {
                        operationFailed = true; failureRevision &+= 1
                    }
                }
                if !closing, result.accountVerified, awaitingResumeProof { foregroundAfterAccountProof() }
                if !closing, initialAdoptionEnabledBefore != nil, ownTask == nil, let backend {
                    await reconcileInitialAdoption(backend)
                }
            }
            acknowledged?.resume()
        case .replaced(let next):
            guard next > generation || (next == generation && setupState == .failed) else { return }
            generation = next; status = nil; preview = nil; recovery = nil
            retainedRecoveryStart = nil
            hasDeferredNativeWork = false
            hostRevision &+= 1
            // Local collection generations still advance without an iCloud
            // installation. Only successful platform preparation admits an owner.
            guard platformPrepared else { return }
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
                if awaitingInitialOwnProof { initialOwnResumeRead = pendingAccountRead }
                startAccountRead()
            } catch { operationFailed = true }
        }
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
            consume(output, generation: expected)
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
        var ordered: [Issued] = []
        for command in issued {
            if closing, !Self.requiredDuringClose(command.action) { continue }
            commandSequence &+= 1
            let value = Issued(command: command, generation: expected, sequence: commandSequence)
            switch command.action {
            case .stopEngine, .pauseMedia, .cancelExchange, .cancelBackgroundFlush, .cancelFlushTimer:
                startActions([value], backend: backend, revision: revision)
            default: ordered.append(value)
            }
        }
        if !ordered.isEmpty { startActions(ordered, backend: backend, revision: revision) }
    }
    private func startActions(_ issued: [Issued], backend: any SyncCoordinatorBackend, revision: UInt64, separateOperation: Bool = false) {
        let id = UUID()
        let context = SyncCommandTaskContext(generation: generation, revision: revision)
        operationContexts[id] = context
        operationTasks[id] = Task { [self] in
            defer { operationTasks.removeValue(forKey: id); operationContexts.removeValue(forKey: id) }
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
                    let result = await backend.execute(command)
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
                    await withCheckedContinuation { acknowledged in
                        #if DEBUG
                        observeCausalBoundary?(.commandResultEnqueue, command)
                        #endif
                        enqueue(.result(result, generation: value.generation, revision: revision,
                            shutdownCancelled: context.shutdownCancelled, acknowledged: acknowledged))
                    }
                    #if DEBUG
                    observeCausalBoundary?(.commandAcknowledgementJoined, command)
                    #endif
                }
            }
        }
    }
    private func replaceBackend(after originalAttachment: Task<Void, Never>?) async {
        defer { replacementTask = nil }
        await originalAttachment?.value
        guard platformPrepared else { return }
        for task in operationTasks.values { task.cancel() }
        accountTask?.cancel()
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
                enqueue(.policy(.refresh(.cloud)))
                if let reachable { enqueue(.policy(.network(reachable: reachable))) }
                enqueue(.account(false))
                if observeSignals { observePlatformSignals() }
                await reconcileInitialAdoption(successor)
                return
            }
        } catch { replacing = true; operationFailed = true; setupState = .failed }
    }
    private static func requiredDuringClose(_ action: CloudPolicyAction) -> Bool {
        switch action {
        case .stopEngine, .cancelExchange, .recordExchange, .recordQuietPoint, .statusChanged,
             .updatePending, .applyReady, .probeTracking: true
        default: false
        }
    }
    private func startAccountRead() {
        guard accountTask == nil, !closing, !replacing, !suspended, let backend else { return }
        accountTask = Task {
            defer {
                accountTask = nil
                if pendingAccountRead != nil, !closing { startAccountRead() }
            }
            while let read = pendingAccountRead, !closing, !Task.isCancelled {
                pendingAccountRead = nil
                let expected = generation, revision = hostRevision
                let complete = await backend.prepareAccountRead(read)
                while true {
                    let finished = await withCheckedContinuation {
                        enqueue(.completeAccount(read, backend: backend, generation: expected, revision: revision,
                            complete: complete, acknowledged: $0))
                    }
                    if finished { break }
                    observeAccountPolicyDrain?(.waiting)
                    // Never join accountTask here: this is that original task.
                    // Its completion/result must leave the input lane available
                    // to acknowledge older operations and their follow-up work.
                    for original in Array(operationTasks.values) { await original.value }
                    await waitForInputs()
                }
            }
        }
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
    private func drainOperations() async {
        repeat {
            let originals = Array(operationTasks.values)
            for task in originals { await task.value }
            await ownTask?.value
            await waitForInputs()
        } while !operationTasks.isEmpty || ownTask != nil
    }

    /// Stops native admissions and joins original SDK/own/media callbacks while
    /// Core is OPEN. A failed drain leaves the backend retained for close retry.
    func cancelAndDrain() async -> Bool {
        if let closeTask { return await closeTask.value }
        if drained { return true }
        closing = true
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
            await waitForInputs()
            for (id, task) in operationTasks {
                // Preserve the first cancellation origin (for example expiry).
                if !task.isCancelled, let context = operationContexts[id],
                   context.generation == generation, context.revision == hostRevision {
                    context.shutdownCancellationRequested = true
                }
                task.cancel()
            }
            accountTask?.cancel(); ownTask?.cancel()
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
                hasDeferredNativeWork = false
                hostRevision &+= 1; drained = true; platformPrepared = false
                return true
            } catch { operationFailed = true; return false }
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
