import CoreBridge
import Foundation
import Network
import Observation
import Synchronization

/// The retained provider owner. Focused lifecycle tests substitute only this seam.
@MainActor protocol SyncCoordinatorBackend: AnyObject, Sendable {
    var providerService: CoreProviderService? { get }
    func attached(settings: SyncDeviceSettings) async
    func coreEvent(_ event: CoreEvent)
    func freezeForSuspend() async throws
    func resumeAfterCoreResume() async
    func quiesce() async throws
    func close() async throws
}
extension SyncCoordinatorBackend {
    var providerService: CoreProviderService? { nil }
    func attached(settings: SyncDeviceSettings) async {}
    func coreEvent(_ event: CoreEvent) {}
    func freezeForSuspend() async throws {}
    func resumeAfterCoreResume() async {}
}
enum SyncOwnerFailure: Error { case configurationUnavailable }
@MainActor final class ProviderOnlySyncBackend: SyncCoordinatorBackend {
    let providerService: CoreProviderService?
    init(provider: CoreProviderService) { providerService = provider }
    func quiesce() async throws {}
    func close() async throws {}
}
enum MeguriSignInLocalFailure: Error { case busy, attachment, credentialStore }
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

@Observable @MainActor
public final class SyncCoordinator {
    public enum SetupState: Equatable { case configurationUnavailable, prepared, attached, failed }
    public var setupState: SetupState { ownerState }
    private var ownerState: SetupState = .configurationUnavailable
    public private(set) var serviceChangeFailure: SyncServiceChangeFailure?
    @ObservationIgnored private var providerChangeReason: ProviderFailureReason = .other
    public private(set) var settings: SyncDeviceSettings
    public private(set) var generation: UInt64 = 0 {
        didSet { if generation != oldValue { meguriLastExchange = nil } }
    }
    #if DEBUG
    public private(set) var debugMeguriRoundSuccessSequence: UInt64 = 0
    #endif
    public private(set) var operationFailed = false
    var syncOwnerAttached: Bool { ownerState == .attached && !replacing && backend != nil && provider != nil }
    enum PlatformSignalStage: Equatable { case started, drained }
    @ObservationIgnored var observePlatformSignalLifetime: (@MainActor (PlatformSignalStage) -> Void)?
    @ObservationIgnored private let bootstrap: @MainActor () async throws -> Void
    @ObservationIgnored private let makeBackend: @MainActor () async throws -> any SyncCoordinatorBackend
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let clock: AppClock
    @ObservationIgnored private let recordDiagnostic: FieldDiagnosticRecorder
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
    private var backendRetired = false
    private var replacementTask: Task<Void, Never>?
    private var replacing = false
    private var suspended = false
    private var reachable: Bool?
    private var networkTask: Task<Void, Never>?
    private var networkMonitor: NWPathMonitor?
    private var closing = false
    private var entryActivationHeld = false
    private var drained = false
    private var closeTask: Task<Bool, Never>?
    private var attachmentTask: Task<Void, Never>?
    private var attachmentFailure: (any Error)?
    private var hostRevision: UInt64 = 0 {
        didSet { if hostRevision != oldValue { meguriLastExchange = nil } }
    }
    // Preserve the old defaults key for installed collections.
    private static let settingsKey = "meguri.cloud.deviceSettings"
    private static let automaticKey = "meguri.sync.automatic"
    private static let round2Key = "meguri.sync.round2Migrated"
    /// Rendering fixtures only: no binding stands behind this value.
    private var renderConnected: Bool?
    /// Synthetic examples only: the confirmation to open, and its facts.
    public internal(set) var renderServiceSwitch: RenderServiceSwitch?
    private var renderChoice = false
    private var renderRetryMedia = false
    /// P25.3: on with no stored value. Written only when the person changes
    /// the switch or P25.2's migration writes off.
    public var storedAutomatic: Bool { (defaults?.object(forKey: Self.automaticKey) as? Bool) ?? memoryAutomatic }
    private var memoryAutomatic = true
    fileprivate func applyRenderFixture(_ state: RenderState) {
        let recent = Int64(clock.now().timeIntervalSince1970 * 1_000) - 240_000
        ownerState = .attached
        switch state {
        case .off:
            providerState = .renderFixture(selection: .localOnly, phase: .localOnly)
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
            // Signed in earlier, then signed out: T21.2's needs-you case.
            meguriPhase = .setupRequired; meguriPhase = .signedOut; meguriEndpoint = "https://sync.example.test/"
            renderConnected = false
        case .meguriNeverSignedIn:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .unconfigured; meguriEndpoint = ""
            renderConnected = false
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
        case .meguriStructureUploadPending:
            providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
            meguriPhase = .setupRequired; meguriEndpoint = "https://sync.example.test/"; meguriUsername = "study-account"
            meguriRoutine = .idle; meguriLastExchange = recent
            meguriStructureUploadPending = true
            renderConnected = true
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
        // A paused setup media step waited for the hold (MP6.5).
        if !updateHold { resumeMeguriMediaIfNeeded(.recovered) }
        return !settings.paused
    }
    private func migrateRound2Settings() async {
        if let pending = providerSettingsTask { await pending.value }
        guard let defaults, round2MigrationPending else { return }
        var next = settings
        if defaults.object(forKey: Self.automaticKey) == nil,
           next.paused || (next.enabled && !next.automatic && providerState?.selection != .localOnly) {
            rememberAutomatic(false)
        }
        if next.paused { next = .init(enabled: next.enabled, paused: false, automatic: false, media: true) }
        if next != settings || !storedSettingsAreCurrent(next) {
            guard await commitMigratedSettings(next) else { return }
        }
        defaults.set(true, forKey: Self.round2Key)
    }
    private func storedSettingsAreCurrent(_ value: SyncDeviceSettings) -> Bool {
        guard let stored = defaults?.dictionary(forKey: Self.settingsKey) as? [String: Bool] else { return true }
        return stored == ["enabled": value.enabled, "paused": value.paused, "automatic": value.automatic, "media": true]
    }
    /// The migration's one write, in order with every other settings write.
    private func commitMigratedSettings(_ next: SyncDeviceSettings) async -> Bool {
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
            } else if oldValue?.phase == .handoffRecovery, !providerNeedsAttention {
                Task { self.resumeMeguriMediaIfNeeded(.recovered) }
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
        case .localOnly: "off"
        case nil: "unread"
        }
        let status = mainStatus
        let connection: MeguriProviderConnectionPhase = meguriAuthenticating ? .authenticating : meguriPhase
        return SyncDebugFacts(service: service, providerPhase: providerState.map { "\($0.phase)" } ?? "unread",
            blockedSource: providerBlockedCause?.source, blockedReason: providerBlockedCause?.reason,
            meguriConnection: providerState?.selection == .meguri ? "\(connection)" : nil,
            meguriSetupStage: meguriStage.map { "\($0)" }, meguriRoutine: meguriRoutine.map { "\($0)" },
            statusTitleKey: status.titleKey, statusDetailKey: status.detailKey,
            serviceChangeFailure: serviceChangeFailure)
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
    /// The files media blocked lists as not transferred (T19.5): from the
    /// setup's accounting or the last round's report; nil when neither is known.
    public var meguriAffectedFileCount: UInt64? {
        if let accounting = meguriAccounting {
            return UInt64(accounting.files.filter { $0.state != .transferred }.count)
        }
        if let result = meguriRoundReport?.media, case .media(let media) = result.outcome { return UInt64(media.affected.count) }
        return nil
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
    public var panelProgress: SyncPanel.Progress? {
        SyncPanel.progress(mainStatus, setupValue: meguriProgressValue,
            moving: meguriAuthenticating || meguriStructureStage != nil
                || (meguriStage == nil && meguriRoutine == .exchanging))
    }
    public var panelServiceSelected: Bool { providerState?.selection == .meguri }
    public var needsUser: Bool {
        providerNeedsAttention || (providerState?.selection == .meguri && meguriStatus.needsUser && !meguriSetupNotFinished)
    }
    public var mainStatus: SyncMainStatus {
        providerState?.selection == .meguri || providerNeedsAttention ? SyncKinds.meguri(meguriKindFacts) : SyncKinds.off
    }
    public var meguriKindFacts: MeguriKindFacts {
        .init(handoffRecovery: providerState?.phase == .handoffRecovery, providerBlocked: providerState?.phase == .blocked,
            connection: meguriAuthenticating ? .authenticating : meguriPhase, stage: meguriStage, routine: meguriRoutine,
            failureKey: meguriFailureKey, finishOffered: meguriCanFinish, automatic: settings.automatic && storedAutomatic,
            mediaFilesLeft: meguriSetupMediaLeft, mediaListKnown: (meguriAffectedFileCount ?? 0) > 0,
            structureStage: meguriStructureStage, structureUploadPending: meguriStructureUploadPending,
            signedInBefore: meguriSignedInBefore, signInRefused: meguriFirstSignInRefused)
    }
    public var providerHandoffInProgress: Bool { providerHandoff != nil || providerState?.phase == .handoffRecovery }
    private var providerHandoff: ProviderHandoffOwnership?
    private var providerHandoffDestination: SyncProviderSelection?
    private var providerSettingsTask: Task<Void, Never>?
    private var providerSettingsRevision: UInt64 = 0
    private var desiredProviderSettings: SyncDeviceSettings?
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
    public private(set) var meguriPhase: MeguriProviderConnectionPhase = .unconfigured {
        // T21.2: a sign-in on this connection makes a later sign-out a problem;
        // a removed connection starts over as setup.
        didSet {
            if meguriPhase == .setupRequired { meguriSignedInBefore = true; meguriFirstSignInRefused = false }
            else if meguriPhase == .unconfigured { meguriSignedInBefore = false; meguriFirstSignInRefused = false }
        }
    }
    /// Kept in this device's defaults, so a relaunch gives the same answer; the
    /// service change that deletes the sign-in clears it (T21.2).
    private var meguriSignedInBefore = false {
        didSet { if meguriSignedInBefore != oldValue { defaults?.set(meguriSignedInBefore, forKey: Self.signedInBeforeKey) } }
    }
    private static let signedInBeforeKey = "meguri.sync.signedInBefore"
    /// A first Sign In from the fields failed (T21.2). Kept apart from the
    /// error text, which an edit of the address clears.
    private var meguriFirstSignInRefused = false
    /// T21.2: Meguri Sync is selected and has not been signed in yet; the Sync
    /// section shows the sign-in form alone and the status stays quiet.
    public var meguriSetupNotFinished: Bool {
        providerState?.selection == .meguri && SyncKinds.meguriSetupNotFinished(meguriKindFacts)
    }
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
    /// MP7: the phase of the note type structure save job (or Upload…) that runs.
    public private(set) var meguriStructureStage: MeguriStructureStage?
    /// MP7 step 5: a structure save is committed here and its upload has not
    /// run. Read from the core on load, after every job and after note type
    /// changes (an undo withdraws it); no round runs while it holds.
    public private(set) var meguriStructureUploadPending = false
    /// The running job, cancelled where MP7 allows.
    @ObservationIgnored private var meguriStructureJob: Task<MeguriStructureOutcome, any Error>?
    /// MP7 step 5's Upload… while its upload holds the binding (no ready round
    /// service then): the connection's own, which the core admits without readiness.
    @ObservationIgnored private var meguriStructureRound: CoreMeguriRoundService?
    @ObservationIgnored private var meguriStructureRun: UUID?
    @ObservationIgnored private var meguriStructureCancelRequested = false
    /// A save waits for a round or action that runs first.
    @ObservationIgnored private var meguriStructureWaiting = false
    /// Tests only: replaces the bridge's structure jobs and the provider's pending read.
    @ObservationIgnored var meguriStructureJobsOverride: (any MeguriStructureJobService)?
    /// D7: a completed upload clears the undo history; the Edit menu reads it again.
    @ObservationIgnored var meguriStructureJobEnded: (@MainActor () -> Void)?
    var meguriCollectionWorkHeld: Bool {
        meguriPreview != nil || meguriSetupPreview != nil || meguriStage == .preparing || meguriStage == .collection
    }
    /// The reason under the Service control while a change must wait: the
    /// setup media step names itself (resume fix), anything else keeps the
    /// review-or-collection-change sentence.
    public var serviceChangeBusyKey: String {
        guard meguriStage == .media else { return "sync.switch.busy" }
        return meguriSetupMediaLeft != nil ? "sync.switch.busy.mediaDownload" : "sync.switch.busy.media"
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
        return !meguriBusy && !updateHold && meguriOperation != nil
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
            stage: meguriStage, routine: meguriRoutine, settling: deviceSettingsSettling,
            structureRunning: meguriStructureStage != nil, structureUploadPending: meguriStructureUploadPending)
    }
    var prepareEditorReplacement: (@MainActor (String, ReviewCollection.EditorReplacementDecision, @MainActor () -> Bool) async -> ReviewCollection.EditorReplacementAdmission?)?
    var editorReplacementIsCurrent: (@MainActor (UUID) -> Bool)?
    var finishEditorReplacement: (@MainActor (UUID) -> Void)?
    private var meguriEditorDecision: (owner: UUID, decision: ReviewCollection.EditorReplacementDecision)?
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
    @ObservationIgnored private var meguriReadyAdoptionPending = false
    /// Tests only: replaces the wait before an automatic attempt, and sees each trigger.
    @ObservationIgnored var meguriMediaRetryDelay: ((Duration?) -> Duration?)?
    @ObservationIgnored var meguriMediaResumeObserver: ((MeguriMediaResume.Trigger) -> Void)?
    private var meguriWake: (id: UUID, task: Task<Void, Never>, availability: MeguriWakeAvailability)?
    /// MP6.10: a review session's changes wait for one window before their round.
    @ObservationIgnored lazy var meguriReviewRounds = MeguriReviewRounds { [weak self] in
        self?.requestMeguriWake(automatic: true)
    }
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
    @discardableResult private func issueMeguri(_ body: @escaping @MainActor (UUID) async -> Void) -> Bool {
        guard !meguriBusy, !meguriCancelling, !closing, !replacing, !suspended, !providerActionInProgress,
              providerState?.selection == .meguri, !providerNeedsAttention else { return false }
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
        return true
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
    var meguriSignInAttached: Bool { provider != nil && ownerState == .attached && !backendRetired }
    func failMeguriSignInPreparation(_ failure: MeguriSignInLocalFailure) {
        meguriLastError = failure
        meguriSignInErrorKey = Self.meguriAuthenticationKey(failure)
    }
    func clearMeguriSignInPreparationError() { meguriSignInErrorKey = nil }
    /// Tests only: the state a refused first Sign In leaves (T21.2).
    func renderFirstSignInRefusedForTests() {
        meguriPhase = .blocked; meguriSignInErrorKey = "sync.meguri.error.credentials"; meguriFirstSignInRefused = true
    }
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
            meguriReady = true; meguriRound = round; meguriStage = nil; meguriOperation = nil; meguriStructureRound = nil
            if meguriRoutine == nil { meguriRoutine = .unreported }
            // MP7 step 5's state survives a relaunch in the core: it is read with the binding.
            await refreshMeguriStructurePending()
        case .unbound(let retained):
            meguriReady = false; meguriRound = nil; meguriRoutine = nil
            // A structure upload that held the binding still waits (MP7 step 4);
            // its Upload… runs on the connection's round service.
            meguriStructureRound = nil
            if await refreshMeguriStructurePending() == true, meguriCurrent(owner, expected, revision) {
                meguriStructureRound = try? await owner.roundService()
            }
            guard meguriCurrent(owner, expected, revision), meguriBootstrap === facade else { return }
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
                // A sign-in that setup's row asked for lets a paused media step continue (MP6.5).
                if self.meguriStage == .mediaPaused || (self.meguriReady && self.meguriReadyAdoptionPending) {
                    self.meguriMediaResumePending = .recovered
                }
            } catch {
                self.meguriLastError = error
                if self.generation == expected, self.hostRevision == revision, editRevision == self.meguriAddressEditRevision {
                    if self.meguriSignInErrorKey == nil { self.meguriSignInErrorKey = Self.meguriAuthenticationKey(error) }
                    if !self.meguriSignedInBefore { self.meguriFirstSignInRefused = true }
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
              retainedHostAdmitted, !backendRetired, ownerState == .attached,
              let backend, let provider else { return false }
        let revision = hostRevision
        do {
            let state = try await provider.refresh()
            guard generation == expected, revision == hostRevision, self.backend === backend,
                  self.provider === provider, !closing, !replacing, !suspended, !drained,
                  !backendRetired, ownerState == .attached else { return false }
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
        meguriMediaResumeObserver?(trigger)
        // Setup completed but its settings were not read yet: every trigger
        // tries that read again, after the timed retries are used up too.
        if meguriReadyAdoptionPending, meguriReady {
            Task { if await self.adoptMeguriReady() { self.requestMeguriWakeIfReadyPending() } }
            return
        }
        guard meguriStage == .mediaPaused, meguriOperation != nil, meguriMediaResume.admits(trigger) else { return }
        // A setup action that still runs reads Core again when it ends. A wait
        // that ends during an attempt is that attempt's: it schedules its own.
        if meguriBusy { if trigger != .retry { meguriMediaResumePending = trigger }; return }
        runMeguriMedia(trigger)
    }
    private func runMeguriMedia(_ trigger: MeguriMediaResume.Trigger) {
        guard meguriCanRetryMedia, meguriMediaResume.admits(trigger) else { return }
        // A refused start (suspended, replaced, another provider action) keeps
        // the scheduled retry and the counts as they were; an automatic trigger
        // waits for the next Meguri action's end, or its own trigger.
        let accepted = issueMeguri { id in
            self.meguriMediaRetry?.cancel(); self.meguriMediaRetry = nil
            self.meguriMediaResume.started(trigger)
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
            // The read may find setup already complete (a stop after completion):
            // setup's last steps then run, with no media attempt.
            if self.meguriReady {
                if await self.adoptMeguriReady() { self.recordMeguriMediaAttempt(.ready) }
                else { self.meguriReadyAdoptionPending = true; self.retryMeguriReadyAdoption() }
                return
            }
            // An edited address reloads nothing until it is signed in to.
            guard !self.meguriAddressDirty, let current = self.meguriOperation,
                  [.mediaPaused, .mediaStopped, .mediaBlocked].contains(self.meguriStage) else { return }
            await self.runMeguriMediaStep(current, id: id)
        }
        if !accepted, trigger.automatic, trigger != .retry || meguriMediaRetry == nil { meguriMediaResumePending = trigger }
    }
    /// One run of the media step and its record: whether and when it continues by itself.
    private func runMeguriMediaStep(_ operation: MeguriBootstrapOperation, id: UUID) async {
        let outcome: MeguriMediaResume.Outcome
        do { outcome = try await continueMeguriMedia(operation, id: id) }
        catch {
            meguriLastError = error
            if suspended || closing || replacing || providerHandoffInProgress {
                // The app or the connection ended the step: it waits, uncounted.
                outcome = interruptMeguriMedia(operation)
            } else {
                // An error is not a refusal: the operation and its media stay for the next attempt.
                if meguriStage == .media { pauseMeguriMedia(operation) }
                outcome = .unfinished
            }
        }
        recordMeguriMediaAttempt(outcome)
    }
    func recordMeguriMediaAttempt(_ outcome: MeguriMediaResume.Outcome) {
        // An attempt that ended while this device had no network is not counted:
        // the network's return starts the next one (MP6.5).
        let counted: MeguriMediaResume.Outcome = outcome == .unfinished && reachable == false ? .notRun : outcome
        let wait = meguriMediaResume.finished(counted)
        // Paused and stopped follow the record: Try Again from stopped that ends
        // unfinished is paused again, and continues by itself.
        if meguriStage == .mediaPaused || meguriStage == .mediaStopped, counted == .unfinished || counted == .notRun {
            meguriStage = meguriMediaResume.stopped ? .mediaStopped : .mediaPaused
        }
        meguriMediaRetry?.cancel(); meguriMediaRetry = nil
        guard let wait = meguriMediaRetryDelay.map({ $0(wait) }) ?? wait else { return }
        let expected = generation, revision = hostRevision
        meguriMediaRetry = Task { [weak self] in
            try? await Task.sleep(for: wait)
            guard !Task.isCancelled, let self, self.generation == expected, self.hostRevision == revision,
                  !self.closing else { return }
            self.meguriMediaRetry = nil
            self.resumeMeguriMediaIfNeeded(.retry)
        }
    }
    private func adoptMeguriReadySettings(_ state: SyncProviderState, _ committed: SyncDeviceSettings) {
        providerState = state; settings = committed
        // P25.3: a stored off outlives a setup that committed automatic on.
        if committed.automatic, !storedAutomatic {
            setDeviceSettings(paused: committed.paused, automatic: false)
        }
    }
    /// Setup completed, but a stop or an error came before its last steps:
    /// read the settings Ready committed, and ask for the first round.
    @discardableResult private func adoptMeguriReady() async -> Bool {
        let expected = generation, revision = hostRevision, settingsRevision = providerSettingsRevision
        // A service change owns the provider state while it runs: read after it.
        guard !providerActionInProgress, !providerHandoffInProgress else { meguriReadyAdoptionPending = true; return false }
        guard let state = try? await provider?.refresh(), let committed = state.settings,
              generation == expected, hostRevision == revision, !closing, !replacing else { return false }
        guard !providerActionInProgress, !providerHandoffInProgress else { meguriReadyAdoptionPending = true; return false }
        // A settings write made meanwhile is newer than this read: read again later.
        guard settingsRevision == providerSettingsRevision else { meguriReadyAdoptionPending = true; return false }
        adoptMeguriReadySettings(state, committed)
        meguriOperation = nil
        meguriReadyWakePending = true
        meguriReadyAdoptionPending = false
        return true
    }
    /// A read of the settings Ready committed that failed is tried again after
    /// the same waits as the media step, at most as often (MP6.5).
    private func retryMeguriReadyAdoption(attempt: Int = 0) {
        guard attempt < MeguriMediaResume.delays.count else { return }
        let wait = meguriMediaRetryDelay.map { $0(MeguriMediaResume.delays[attempt]) } ?? MeguriMediaResume.delays[attempt]
        guard let wait else { return }
        let expected = generation, revision = hostRevision
        Task { [weak self] in
            try? await Task.sleep(for: wait)
            guard let self, self.generation == expected, self.hostRevision == revision, !self.closing,
                  self.meguriReady else { return }
            if await self.adoptMeguriReady() { self.requestMeguriWakeIfReadyPending() }
            else { self.retryMeguriReadyAdoption(attempt: attempt + 1) }
        }
    }
    /// A provider action (a service change) refused a Meguri start while it ran:
    /// the trigger kept for it is tried when the action ends.
    private func replayMeguriMediaResume() {
        guard meguriMediaResumePending != nil || meguriReadyAdoptionPending, !meguriBusy else { return }
        let trigger = meguriMediaResumePending ?? .recovered
        meguriMediaResumePending = nil
        Task { self.resumeMeguriMediaIfNeeded(trigger) }
    }
    private func requestMeguriWakeIfReadyPending() {
        guard meguriReadyWakePending, !meguriBusy else { return }
        meguriReadyWakePending = false
        requestMeguriWake(automatic: true)
    }
    /// The step was stopped from outside (the app went to the background, the
    /// connection was edited or replaced): it is paused, not counted, and the
    /// return to the foreground continues it.
    private func interruptMeguriMedia(_ operation: MeguriBootstrapOperation) -> MeguriMediaResume.Outcome {
        if meguriStage == .media { pauseMeguriMedia(operation) }
        return .notRun
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
        guard let facade = meguriBootstrap, let owner = meguriConnectionOwner, !updateHold else { return .notRun }
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
            guard meguriCurrent(owner, expected, revision) else { return interruptMeguriMedia(operation) }
            applyMeguriOutcome(prepared.outcome)
            guard case .mediaConsent(let consent) = prepared.outcome else { return refuseMeguriMedia(prepared.outcome, operation) }
            guard !providerNeedsAttention else { pauseMeguriMedia(operation); return .notRun }
            let result = try await facade.transferMedia(consent, progress: progressForMeguri(id), mediaProgress: { [weak self] value in
                Task { @MainActor in if self?.meguriAction?.id == id { self?.meguriMediaProgress = value } }
            })
            retainMeguri(result)
            guard meguriCurrent(owner, expected, revision) else {
                if case .operation(let transferred) = result.outcome { return interruptMeguriMedia(transferred) }
                return interruptMeguriMedia(operation)
            }
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
        guard meguriCurrent(owner, expected, revision) else { return interruptMeguriMedia(finished) }
        applyMeguriOutcome(outcome)
        guard case .ready = outcome else { return refuseMeguriMedia(outcome, finished) }
        // Whole-operation Ready is Core's durable automatic-sync consent.
        let state = try await provider?.refresh()
        guard meguriCurrent(owner, expected, revision), let state, let committed = state.settings else { return .notRun }
        adoptMeguriReadySettings(state, committed)
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
    static func meguriSchedulingAllowed(automatic: Bool, committed: SyncDeviceSettings,
                                       desired: SyncDeviceSettings?) -> Bool {
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
        // MP7 step 5: no round runs while a structure upload waits (the core refuses it too).
        guard meguriCanRun, !meguriStructureUploadPending, meguriSchedulingAllowed(automatic: automatic) else { return }
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
                // MP6.10: this round carries every change committed so far.
                meguriReviewRounds.roundStarting()
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
                    // MP6.10: after the exchange is recorded, a change made during the
                    // round keeps the status pending until its window's round.
                    meguriRoutine = Self.meguriRoutine(meguriRoutine, changesWaiting: meguriReviewRounds.waiting)
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
        // A round runs only on a signed-in connection: its sign-out needs the person (T21.2).
        meguriSignedInBefore = true
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
        // MP7: a structure job (a save, or an Upload… on the connection's own
        // round service) stops where MP7 allows; a stopped upload stays pending.
        meguriStructureCancelRequested = true
        meguriStructureJob?.cancel()
        if meguriStructureWaiting { meguriWake?.task.cancel() }
        await round?.cancelAndDrain()
        if let structureRound = meguriStructureRound, structureRound !== round { await structureRound.cancelAndDrain() }
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
            providerDirectory: providerDirectory, meguriCredentialStore: meguriCredentialStore,
            recordDiagnostic: recordDiagnostic)
        successor.serviceChangeFailure = serviceChangeFailure
        // The original already joined its native owners above. The successor
        // needs an attachment retry, not a strong chain of drained owners.
        successor.providerAttachmentPending = true
        successor.generation = expected
        successor.providerPlatform = prepared
        successor.operationFailed = operationFailed
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
                            defaults: UserDefaults? = .standard,
                            meguriCredentialStore: (any MeguriProviderCredentialStore)? = nil,
                            recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in }) {
        let providerDirectory = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: directory, appGroupSnapshot: directory).manualTransferDirectory
        self.init(bootstrap: {}, makeBackend: {
            ProviderOnlySyncBackend(provider: try await CoreProviderService.attached())
        }, defaults: defaults, clock: clock, directory: directory, observeSignals: true,
           providerDirectory: providerDirectory, meguriCredentialStore: meguriCredentialStore,
           recordDiagnostic: recordDiagnostic)
    }

    init(bootstrap: @escaping @MainActor () async throws -> Void,
         makeBackend: @escaping @MainActor () async throws -> any SyncCoordinatorBackend,
         defaults: UserDefaults?, clock: AppClock = .system, directory: URL? = nil, observeSignals: Bool = false,
         providerDirectory: URL? = nil, meguriCredentialStore: (any MeguriProviderCredentialStore)? = nil,
         recordDiagnostic: @escaping FieldDiagnosticRecorder = { _ in }) {
        self.recordDiagnostic = recordDiagnostic
        self.bootstrap = bootstrap; self.makeBackend = makeBackend; self.defaults = defaults
        meguriSignedInBefore = defaults?.bool(forKey: Self.signedInBeforeKey) ?? false
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
                ownerState = .failed; operationFailed = true
                return
            }
            guard providerAttachmentPending, providerPlatform?.owner === prepared.owner,
                  generation == prepared.generation, hostRevision == revision,
                  !entryActivationHeld, !replacing, !closing || drained else {
                ownerState = .failed; operationFailed = true
                return
            }
            providerState = state
            if let committed = state.settings { settings = committed }
            // Reuse actual successful setup on this same open Host. Backend
            // construction and provider refresh still run on every attachment.
            platformPrepared = true
            ownerState = .prepared
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
            do { try await bootstrap(); platformPrepared = true; ownerState = .prepared }
            catch SyncOwnerFailure.configurationUnavailable { ownerState = .configurationUnavailable }
            catch {
                ownerState = .failed; operationFailed = true
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
            if generation > self.generation { collectionReplaced(generation) }
            await original.value
            return
        }
        guard platformPrepared else {
            self.generation = max(self.generation, generation)
            return
        }
        guard generation >= self.generation else { return }
        retainedHostAdmitted = true
        if backend != nil || replacing { collectionReplaced(generation); return }
        hostRevision &+= 1
        let revision = hostRevision
        self.generation = generation
        let task = Task { [self] in
            defer { attachmentTask = nil }
            do {
                let value = try await self.makeBackend()
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
                try await attachProvider(to: value, startupForeground: startupForeground)
                guard !closing, !Task.isCancelled, !replacing,
                      revision == hostRevision, generation == self.generation else { return }
                drained = false
                ownerState = .attached
                providerAttachmentPending = false
                providerPlatform = nil
                attachmentFailure = nil
                await migrateRound2Settings()
                await value.attached(settings: settings)
                if observeSignals { observePlatformSignals() }
            } catch {
                guard revision == hostRevision, generation == self.generation else { return }
                attachmentFailure = error
                recordConnectionFailure(error)
                ownerState = .failed
            }
        }
        attachmentTask = task
        await task.value
    }

    func requireProviderAttachment() throws {
        guard ownerState == .attached else {
            throw attachmentFailure ?? BridgeError.unavailable(reason: "Provider attachment unavailable")
        }
    }
    func providerAttachmentFailed(_ error: any Error) {
        guard providerAttachmentPending else { return }
        // Host preparation may report only that no connection was attached.
        // Preserve the factory/refresh failure from the actual attachment.
        if attachmentFailure == nil { attachmentFailure = error }
        recordConnectionFailure(attachmentFailure ?? error)
        ownerState = .failed
    }

    private func recordConnectionFailure(_ error: any Error) {
        operationFailed = true
    }
    func receiveLifecycle(_ lifecycle: CollectionLifecycle) {
        if case .ready(let next) = lifecycle, next != generation { collectionReplaced(next) }
    }
    func collectionReplaced(_ next: UInt64) {
        guard !closing, next > generation || (next == generation && ownerState == .failed) else { return }
        providerPlatform = nil
        generation = next
        meguriReviewRounds.reset()
        hostRevision &+= 1
        providerSettingsRevision &+= 1
        providerRecoveryDrain = nil; desiredProviderSettings = nil
        guard platformPrepared, retainedHostAdmitted else { return }
        replacing = true
        if replacementTask == nil {
            let originalAttachment = attachmentTask
            replacementTask = Task { await replaceBackend(after: originalAttachment) }
        }
    }
    func coreEvent(_ event: CoreEvent) {
        guard !closing else { return }
        if event.generation == generation { backend?.coreEvent(event) }
        // MP6.10: sessions are followed whatever the service, so a later switch to
        // Meguri Sync sees the right state.
        if event.generation == generation {
            switch event.kind {
            case .sessionStarted(let id, _): meguriReviewRounds.sessionStarted(id)
            case .sessionEnded(let id): meguriReviewRounds.sessionEnded(id, request: false)
            default: break
            }
        }
        if event.generation == generation, providerState?.selection == .meguri {
            meguriWake?.availability.observe(kind: event.kind, sequence: event.seq, generation: event.generation)
            switch event.kind {
            case .changes(let changes):
                if changes.source != .syncIncoming,
                   meguriReviewRounds.localChange() {
                    // MP6.10: waiting for the window is pending, never up to date.
                    meguriRoutine = Self.meguriRoutine(meguriRoutine, changesWaiting: true)
                }
            case .sessionEnded: meguriReviewRounds.requestAfterSessionEnd()
            case .available:
                if meguriWake == nil { requestMeguriWake(automatic: true) }
            default: break
            }
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
            let next = SyncDeviceSettings(enabled: settings.enabled, paused: paused, automatic: automatic, media: true)
            desiredProviderSettings = next
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
    private func applyDeviceSettings(paused: Bool, automatic: Bool, enabled: Bool? = nil) {
        let wasPaused = settings.paused, wasAutomatic = settings.automatic
        saveSettings(.init(enabled: enabled ?? settings.enabled, paused: paused, automatic: automatic, media: true))
        if providerState?.selection == .meguri, (wasPaused && !paused) || (!wasAutomatic && automatic) {
            requestMeguriWake(automatic: true)
        }
    }
    private func saveSettings(_ value: SyncDeviceSettings) {
        // The assignment reads media as on (P25.1); store and send that value.
        settings = value
        defaults?.set(["enabled": settings.enabled, "paused": settings.paused,
            "automatic": settings.automatic, "media": true], forKey: Self.settingsKey)
    }
    public func syncNow(source: SyncSource = .command) {
        if updateHold {
            Task { if await clearUpdateHold() { syncNow(source: source) } }
            return
        }
        guard providerState?.selection == .meguri, meguriStatus.route == .round else { return }
        requestMeguriWake(automatic: false)
    }
    @discardableResult public func changeProvider(to destination: SyncProviderSelection, removeMeguriCredentials: Bool = false) async -> Bool {
        guard providerChangeAllowed(), let provider, !suspended, !replacing,
              !providerActionInProgress, !entryActivationHeld,
              !closing || providerHandoff != nil else {
            providerChangeReason = self.provider == nil ? .unavailable : .busy
            return false
        }
        // A return below that names no reason was superseded by another owner.
        providerChangeReason = .cancelled
        providerActionInProgress = true
        defer { providerActionInProgress = false; replayMeguriMediaResume() }
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
            } else if providerHandoffDestination != destination { return false }
            let state = try await provider.state()
            guard current(revision) else { return false }
            providerState = state
            if removeMeguriCredentials, providerState?.removalPending != true { return false }
            guard providerChangeAllowed() else { return false }
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
                // The sign-in is gone: choosing Meguri Sync again is setup again (T21.2).
                meguriSignedInBefore = false
                guard current(joinedRevision) else { return false }
            }
            let committed = try await provider.finishHandoff(providerHandoff)
            guard current(joinedRevision) else { return false }
            providerState = committed
            self.providerHandoff = nil; providerHandoffDestination = nil
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
            providerChangeReason = ProviderFailureReason(error)
            do { providerState = try await provider.state() }
            catch { meguriLastError = error }
            recordConnectionFailure(error)
            return false
        }
    }
    /// The reason of the last `changeProvider` that returned false.
    var providerChangeFailureReason: ProviderFailureReason { providerChangeReason }
    func serviceChangeBegan() { serviceChangeFailure = nil }
    func serviceChangeFailed(_ failure: SyncServiceChangeFailure) { serviceChangeFailure = failure }
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
        defer { providerActionInProgress = false; replayMeguriMediaResume() }
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
        providerRecoveryDrain = nil
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
        defer { providerActionInProgress = false; replayMeguriMediaResume() }
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
            guard ownerState == .failed, !retryingProviderAttachment, !suspended, !replacing,
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
                else { reloadMeguriAfterRecoveryIfNeeded() }
            } catch {
                guard self.provider === provider, expected == generation, revision == hostRevision, !closing, !replacing else { return }
                markProviderBlocked(.authorityRefresh, error); operationFailed = true
            }
        }
    }
    func foregroundAfterLocalResume() async {
        guard !closing, !replacing else { return }
        await backend?.resumeAfterCoreResume()
        suspended = false
        if providerState?.selection == .meguri {
            requestMeguriWake(automatic: true)
            resumeMeguriMediaIfNeeded(.foreground)
        }
    }
    @discardableResult func backgroundAfterLocalFlush(deadlineMilliseconds: Int64?) async -> Bool {
        cancelMeguriEditorDecision(meguriAction?.id)
        await replacementTask?.value
        guard !replacing else { return false }
        if providerState?.selection == .meguri {
            if meguriReviewRounds.flush(), let wake = meguriWake {
                await waitForMeguriRound(wake.task, atMost: Self.meguriBackgroundBudget(deadlineMilliseconds: deadlineMilliseconds,
                    now: Int64(clock.now().timeIntervalSince1970 * 1_000)))
            }
            suspended = true
            await drainMeguriProduct()
        }
        suspended = true
        do { try await backend?.freezeForSuspend(); return true }
        catch {
            suspended = false
            await backend?.resumeAfterCoreResume()
            operationFailed = true
            return false
        }
    }
    /// MP6.10: the status while changes wait is pending; a round that ends idle
    /// with changes made during it stays pending until their window's round.
    static func meguriRoutine(_ routine: MeguriRoutineState?, changesWaiting: Bool) -> MeguriRoutineState? {
        changesWaiting && routine == .idle ? .pending : routine
    }
    /// MP6.10: the share of the system's background time a waiting round may take,
    /// leaving the rest for the local flush and the suspension that follow.
    static func meguriBackgroundBudget(deadlineMilliseconds: Int64?, now: Int64) -> Duration {
        guard let deadlineMilliseconds else {
            // The Mac keeps running when it is hidden: no system deadline, the same bound.
            #if os(macOS)
            return .seconds(20)
            #else
            return .zero
            #endif
        }
        let left = max(0, deadlineMilliseconds - now - 5_000)
        return .milliseconds(min(left, 20_000))
    }
    /// The background or quit wait for a round, which a foreground return or
    /// the system's expiry ends early; the round itself is drained after it.
    private var meguriRoundWait: Task<Void, Never>?
    private func waitForMeguriRound(_ round: Task<Void, Never>, atMost budget: Duration) async {
        let wait = Task { await Self.wait(for: round, atMost: budget) }
        meguriRoundWait = wait
        await wait.value
        if meguriRoundWait == wait { meguriRoundWait = nil }
    }
    /// MP6.10: the app is back, or its background time is up: stop waiting.
    func endMeguriRoundWait() { meguriRoundWait?.cancel() }
    /// Returns when `task` ends, `budget` passes or the caller is cancelled,
    /// whichever comes first. It never cancels `task`.
    static func wait(for task: Task<Void, Never>, atMost budget: Duration) async {
        guard budget > .zero, !Task.isCancelled else { return }
        let gate = WaitGate()
        await withTaskCancellationHandler {
            await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in
                gate.install(continuation)
                let finished = Task { await task.value; gate.open() }
                let timeout = Task { try? await Task.sleep(for: budget); gate.open() }
                gate.onOpen = { finished.cancel(); timeout.cancel() }
            }
        } onCancel: { gate.open() }
    }
    private nonisolated final class WaitGate: @unchecked Sendable {
        private let lock = NSLock()
        private var continuation: CheckedContinuation<Void, Never>?
        private var opened = false
        private var cleanup: (@Sendable () -> Void)?
        var onOpen: (@Sendable () -> Void)? {
            get { lock.withLock { cleanup } }
            set {
                let runNow = lock.withLock { () -> Bool in
                    if opened { return true }
                    cleanup = newValue; return false
                }
                if runNow { newValue?() }
            }
        }
        func install(_ continuation: CheckedContinuation<Void, Never>) {
            let resumeNow = lock.withLock { () -> Bool in
                if opened { return true }
                self.continuation = continuation; return false
            }
            if resumeNow { continuation.resume() }
        }
        func open() {
            let (continuation, cleanup) = lock.withLock { () -> (CheckedContinuation<Void, Never>?, (@Sendable () -> Void)?) in
                guard !opened else { return (nil, nil) }
                opened = true
                defer { self.continuation = nil; self.cleanup = nil }
                return (self.continuation, self.cleanup)
            }
            cleanup?()
            continuation?.resume()
        }
    }
    public func backgroundExpired() {
        cancelMeguriEditorDecision(meguriAction?.id)
        endMeguriRoundWait()
        meguriWake?.task.cancel()
    }
    func quitAfterLocalFlush(completion: @escaping @MainActor () -> Void) async {
        cancelMeguriEditorDecision(meguriAction?.id)
        if providerState?.selection == .meguri {
            _ = meguriReviewRounds.flush()
            if let wake = meguriWake { await waitForMeguriRound(wake.task, atMost: .seconds(10)) }
        }
        completion()
    }
    private func replaceBackend(after originalAttachment: Task<Void, Never>?) async {
        defer { replacementTask = nil }
        await originalAttachment?.value
        guard platformPrepared else { return }
        do {
            await providerSettingsTask?.value
            meguriWakePending = false
            let originalWake = meguriWake
            originalWake?.task.cancel()
            await meguriConnectionOwner?.cancelAndDrain()
            await originalWake?.task.value
            meguriReady = false; meguriBootstrap = nil; meguriRound = nil; meguriOperation = nil
            try await backend?.quiesce()
            try await backend?.close()
            backendRetired = true
            backend = nil
            provider = nil; meguriConnectionOwner = nil
            providerHandoff = nil; providerHandoffDestination = nil
            while !closing {
                // generation/revision retain the newest event's successor intent
                // while this task is joining or awaiting the original factory.
                let expected = generation, revision = hostRevision
                let successor: any SyncCoordinatorBackend
                do {
                    successor = try await makeBackend()
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
                ownerState = .attached
                try await attachProvider(to: successor)
                guard !closing, expected == generation, revision == hostRevision else {
                    try await successor.close()
                    backend = nil
                    continue
                }
                replacing = false
                await migrateRound2Settings()
                await successor.attached(settings: settings)
                if observeSignals { observePlatformSignals() }
                return
            }
        } catch {
            replacing = true; operationFailed = true; ownerState = .failed
        }
    }
    func waitForReplacement() async { await replacementTask?.value; await providerSettingsTask?.value }
    func waitForInputs() async { await providerSettingsTask?.value }
    func beginEntryActivation() {
        guard !entryActivationHeld else { return }
        providerPlatform = nil
        providerRecoveryDrain = nil
        entryActivationHeld = true
        closing = true
        cancelMeguriEditorDecision(meguriAction?.id)
    }
    func drainEntryActivation() async -> Bool {
        guard entryActivationHeld else { return false }
        return await cancelAndDrain(entryActivation: true)
    }
    @discardableResult func releaseEntryActivation() throws -> Bool {
        guard entryActivationHeld else { return false }
        guard drained, closeTask == nil else { throw BridgeError.busy(runningOperation: "sync drain") }
        entryActivationHeld = false
        closing = false; drained = false; replacing = false
        if platformPrepared { ownerState = .prepared }
        return true
    }

    /// Stops provider admissions and joins original collection/media work while
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
        cancelMeguriEditorDecision(meguriAction?.id)
        let task = Task { () -> Bool in
            await providerSettingsTask?.value
            await drainMeguriProduct()
            await meguriConnectionOwner?.cancelAndDrain()
            await attachmentTask?.value
            await replacementTask?.value
            networkTask?.cancel(); networkMonitor?.cancel()
            do {
                try await backend?.quiesce()
                await networkTask?.value
                if networkMonitor != nil { observePlatformSignalLifetime?(.drained) }
                try await backend?.close()
                backendRetired = true
                backend = nil; suspended = false; networkMonitor = nil
                hostRevision &+= 1; drained = true; retainedHostAdmitted = false
                if !entryActivation { platformPrepared = false; entryActivationHeld = false }
                return true
            } catch {
                operationFailed = true; return false
            }
        }
        closeTask = task
        let result = await task.value
        closeTask = nil
        return result
    }

    func networkChanged(reachable: Bool) {
        guard !closing else { return }
        let recovered = self.reachable == false && reachable
        self.reachable = reachable
        if recovered {
            meguriReachabilityRecoveryRevision &+= 1
            requestMeguriWake(automatic: true)
            resumeMeguriMediaIfNeeded(.network)
        }
    }
    private func observePlatformSignals() {
        guard networkMonitor == nil else { return }
        let monitor = NWPathMonitor()
        let (paths, sink) = AsyncStream<Bool>.makeStream(bufferingPolicy: .unbounded)
        monitor.pathUpdateHandler = { sink.yield($0.status == .satisfied) }
        networkMonitor = monitor
        monitor.start(queue: DispatchQueue(label: "Meguri.sync.reachability"))
        networkTask = Task { [weak self] in
            for await reachable in paths {
                guard let self, !Task.isCancelled else { return }
                self.networkChanged(reachable: reachable)
            }
        }
        observePlatformSignalLifetime?(.started)
    }
}

extension SyncCoordinator {
    public enum RenderState: String, Equatable, CaseIterable, Sendable {
        case off, meguriConnected, meguriSignIn, meguriNeedsYou
        /// N23's panel states (round 10).
        case meguriSyncing, meguriSetupMedia, meguriRoundFailed, meguriBlocked, meguriSignInRefused
        case meguriPreparing, meguriMediaBlocked, meguriAcceptanceUnknown
        /// MP6.5 (resume fix): setup's media step paused, and stopped after its automatic attempts.
        case meguriSetupMediaPaused, meguriSetupMediaStopped
        /// MP7 step 5: a note type structure save waits for Upload….
        case meguriStructureUploadPending
        /// Meguri Sync chosen and never signed in on this device: no address, no account.
        case meguriNeverSignedIn
    }
    /// Builds synthetic Sync section examples. The coordinator is never
    /// attached: it owns no collection, account, binding or server, and its
    /// actions find nothing to act on.
    public static func renderFixture(_ state: RenderState, clock: AppClock = .system,
                                     serviceSwitch: RenderServiceSwitch? = nil) -> SyncCoordinator {
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { throw SyncOwnerFailure.configurationUnavailable },
            defaults: nil, clock: clock)
        sync.applyRenderFixture(state)
        sync.renderServiceSwitch = serviceSwitch
        return sync
    }
    /// A synthetic example asks the Service control to open its confirmation
    /// for `destination` ("localOnly", "meguri"), with invented facts.
    public struct RenderServiceSwitch: Sendable, Equatable {
        public let destination: String
        public let facts: ServiceSwitchFacts
        public init(destination: String, facts: ServiceSwitchFacts) { self.destination = destination; self.facts = facts }
    }
}

// MARK: Note type structure saves (`sync/meguri-provider` MP7, `notetypes/draft-save` D6, D7)

/// Collects a job's answer across the provider action that runs it.
@MainActor private final class MeguriStructureAnswer {
    var value: Result<NoteTypeSyncSaveResult, any Error>?
}

/// A deck package import's own answer, kept across the structure job that runs it.
nonisolated private final class MeguriDeckImportAnswer: Sendable {
    let value = Mutex<MeguriDeckImportOutcome?>(nil)
}

extension SyncCoordinator: DeckImportMeguriSync {
    /// MP7.1: the same readiness as a note type save's Save and Upload.
    public var deckImportSyncReady: Bool { noteTypeSyncState == .ready }

    public func deckImportSyncReachable() async -> Bool { await noteTypeSyncReachable() }

    /// MP7.1's job, as the provider's one action, with what a structure job's
    /// outcome does to the sync status afterwards.
    public func importDeckPackage(_ session: DeckImportSession, options: DeckImportOptions,
                                  confirmation: DeckImportConfirmation, reachable: Bool,
                                  stage: @escaping @MainActor (NoteTypeSyncStage) -> Void) async throws
        -> MeguriDeckImportOutcome {
        let answer = MeguriDeckImportAnswer()
        let saved = try await runMeguriStructure(progress: { shown, _, _ in stage(shown) }) { jobs, onStage in
            let outcome: MeguriDeckImportOutcome
            do {
                outcome = try await jobs.importDeckPackage(session, options: options, confirmation: confirmation,
                                                           reachable: reachable, onStage: onStage)
            } catch {
                // Thrown only before the job starts (its admission): nothing was imported.
                outcome = .notStarted(.unavailable)
            }
            answer.value.withLock { $0 = outcome }
            return outcome.structureOutcome
        }
        if let outcome = answer.value.withLock({ $0 }) { return outcome }
        switch saved {
        // The job did not run: another one, or a round, held the provider.
        case .notSaved: return .notStarted(.busy)
        case .cancelled: return .cancelled
        case .syncStopped: return .unavailable
        // The job ended with an error after a commit, or without saying whether it committed.
        case .saved, .unconfirmed: return .importedWithoutResult(upload: nil)
        }
    }

    public func cancelDeckImport() { cancelNotetypeStructureSave() }
}

extension SyncCoordinator: NoteTypesMeguriSync {
    /// W12/D3: only proved absence permits local deletion. Presentation's
    /// notSetUp also covers stale/unread bindings and cannot prove absence.
    func noteTypeDeletionService() async -> NoteTypeSaveService? {
        guard !closing, !replacing, !suspended, !providerHandoffInProgress,
              !providerNeedsAttention, let selection = providerState?.selection else { return nil }
        guard selection == .meguri else { return .other }
        guard meguriLocalActionAvailable, meguriWake == nil, meguriStructureStage == nil,
              !meguriStructureUploadPending, let provider else { return nil }
        let expected = generation, revision = hostRevision
        let providerRevision = providerState?.revision
        let editRevision = meguriAddressEditRevision
        var answer: NoteTypeSaveService?
        // Reserve the coordinator's action while inspecting: setup, sign-in,
        // and regular rounds cannot change the binding between these reads.
        let issued = issueMeguri { _ in
            @MainActor func current() -> Bool {
                self.generation == expected && self.hostRevision == revision && self.provider === provider
                    && self.providerState?.revision == providerRevision && self.providerState?.selection == .meguri
                    && self.meguriAddressEditRevision == editRevision
                    && !self.closing && !self.replacing && !self.suspended
                    && !self.providerHandoffInProgress && !self.providerNeedsAttention
            }
            do {
                let pending = try await provider.structureUploadPending()
                guard current(), !pending else { return }
                let configured = try await provider.configuredMeguri()
                guard current() else { return }
                guard let configured else { answer = .other; return }
                guard let owner = self.meguriConnectionOwner else { return }
                let facade = try await owner.bootstrapService()
                guard current(), self.meguriConnectionOwner === owner,
                      facade.connection == configured.connection else { return }
                let readiness = try await facade.readiness()
                guard current(), self.meguriConnectionOwner === owner else { return }
                switch readiness {
                case .ready:
                    if self.noteTypeSyncState == .ready { answer = .meguriSync }
                case .unbound(let retained):
                    // An interrupted transfer or unresolved acceptance keeps
                    // data-first protection even though it has no ready binding.
                    guard retained.map({ !Self.meguriRetainsEffects($0.receipt) }) ?? true else { return }
                    let preparation = try await facade.pendingPreparation()
                    guard current(), self.meguriConnectionOwner === owner, preparation == nil else { return }
                    answer = .other
                case .notSelected, .stale: break
                }
                // A connection replacement during an await invalidates this
                // answer, including a replacement with the same endpoint.
                let latest = try await provider.configuredMeguri()
                guard current(), self.meguriConnectionOwner === owner,
                      latest?.connection == configured.connection else { answer = nil; return }
            } catch { answer = nil }
        }
        guard issued, let task = meguriAction?.task else { return nil }
        await task.value
        return answer
    }

    /// The jobs for the ready binding, and the pending read for any binding.
    private var meguriStructureJobs: (any MeguriStructureJobService)? {
        if let meguriStructureJobsOverride { return meguriStructureJobsOverride }
        guard let provider else { return nil }
        return CoreMeguriStructureJobs(round: meguriRound ?? meguriStructureRound, provider: provider)
    }

    /// D3 with Meguri Sync chosen: Save and Upload runs only with a ready binding
    /// and no earlier upload waiting.
    public var noteTypeSyncState: NoteTypeSyncState {
        guard providerState?.selection == .meguri, !providerNeedsAttention else { return .notSetUp }
        if meguriStructureUploadPending { return .uploadPending }
        if meguriReady, meguriRound != nil || meguriStructureJobsOverride != nil, meguriStage == nil,
           meguriPhase == .setupRequired, !closing, !replacing, !suspended { return .ready }
        // Signed out, or no collection chosen yet: setting up is the person's step.
        if meguriPhase != .setupRequired || meguriStage == .required || meguriStage == .failed || meguriStage == .preview {
            return .notSetUp
        }
        return .busy
    }

    /// D4: no network path, or the server gives no HTTP answer to a HEAD
    /// request (no sign-in is sent). An unknown answer counts as reachable:
    /// the job's round then finds out.
    public func noteTypeSyncReachable() async -> Bool {
        if reachable == false { return false }
        let endpoint = meguriEndpoint
        guard !endpoint.isEmpty, meguriStructureJobsOverride == nil else { return true }
        switch await EndpointReachability.check(endpoint) {
        case .response, .cancelled: return true
        case .unavailable, .timedOut, .secureConnectionFailed: return false
        }
    }

    public func saveNotetypeStructure(_ draft: NotetypeStructureDraft, reachable: Bool,
                                      progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void)
        async throws -> NoteTypeSyncSaveResult {
        try await runMeguriStructure(progress: progress) { jobs, onStage in
            try await jobs.saveNotetypeStructure(draft, reachable: reachable, onStage: onStage)
        }
    }

    public func removeNotetypeStructure(_ request: NotetypeRemoval, reachable: Bool,
                                        progress: @escaping @MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void)
        async throws -> NoteTypeSyncSaveResult {
        try await runMeguriStructure(progress: progress) { jobs, onStage in
            try await jobs.removeNotetypeStructure(request, reachable: reachable, onStage: onStage)
        }
    }

    public func cancelNotetypeStructureSave() {
        meguriStructureCancelRequested = true
        meguriStructureJob?.cancel()
        // A save still waiting for the round before it: that round stops too,
        // as a drain stops it, so leaving never waits for it.
        if meguriStructureWaiting { meguriWake?.task.cancel() }
    }

    /// MP7 step 5's Upload…: the server's facts and the upload, no round and
    /// no question (the sheet was the consent). Runs from the status row and
    /// the panel.
    @discardableResult public func uploadNotetypeStructure() -> Task<Void, Never>? {
        guard meguriCanUploadStructure else { return nil }
        return Task { _ = try? await runMeguriStructure(progress: nil) { jobs, onStage in try await jobs.uploadNotetypeStructure(onStage: onStage) } }
    }

    /// Upload… can run now.
    public var meguriCanUploadStructure: Bool {
        meguriStructureUploadPending && meguriStructureStage == nil && !meguriBusy && !closing && !replacing && !suspended
            && !providerActionInProgress && !providerHandoffInProgress && !providerNeedsAttention
    }

    /// A note type changed (an undo or redo among them): the waiting upload is
    /// read again, since undoing the save withdraws it and redoing it brings
    /// it back (D7, MP7 step 6).
    func notetypesChanged() {
        guard providerState?.selection == .meguri, meguriStructureStage == nil else { return }
        Task { await refreshMeguriStructurePending() }
    }

    /// Reads MP7 step 5's state. When it is withdrawn (the save was undone),
    /// ordinary rounds run again.
    @discardableResult func refreshMeguriStructurePending() async -> Bool? {
        guard providerState?.selection == .meguri, let jobs = meguriStructureJobs else { return nil }
        let expected = generation, revision = hostRevision
        guard let value = try? await jobs.structureUploadPending(), generation == expected, hostRevision == revision else { return nil }
        let withdrawn = meguriStructureUploadPending && !value
        meguriStructureUploadPending = value
        if withdrawn, meguriStructureStage == nil {
            if !meguriReady, let facade = meguriBootstrap, let owner = meguriConnectionOwner {
                // The upload had held the binding: read it again, so a binding the
                // core gave back is ready before the next round is asked for.
                let expected = generation, revision = hostRevision
                Task {
                    try? await readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
                    requestMeguriWake(automatic: true)
                }
            } else {
                requestMeguriWake(automatic: true)
            }
        }
        return value
    }

    /// One job as the provider's one action: a round or another action that
    /// runs first ends (the job's own round replaces the next one), then the
    /// job, then what its outcome shows.
    private func runMeguriStructure(progress: (@MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void)?,
        _ start: @escaping @Sendable (any MeguriStructureJobService,
            @escaping @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void) async throws -> MeguriStructureOutcome)
        async throws -> NoteTypeSyncSaveResult {
        meguriStructureCancelRequested = false
        meguriStructureWaiting = true
        for _ in 0..<8 where !meguriStructureCancelRequested {
            if let wake = meguriWake {
                meguriWakePending = false; meguriWakeManual = false
                await wake.task.value
            } else if let action = meguriAction {
                await action.task.value
            } else { break }
        }
        meguriStructureWaiting = false
        guard !meguriStructureCancelRequested else { return .cancelled }
        guard meguriWake == nil, meguriAction == nil, meguriStructureStage == nil, let jobs = meguriStructureJobs else { return .notSaved }
        let answer = MeguriStructureAnswer()
        let run = UUID()
        let issued = issueMeguri { [weak self] _ in
            guard let self else { return }
            self.meguriStructureRun = run
            let onStage: @Sendable (MeguriStructureStage, AsyncStream<CoreProgress>?) -> Void = { [weak self] stage, stream in
                Task { @MainActor in self?.meguriStructureStarted(stage, stream, run: run, progress: progress) }
            }
            let job = Task { try await start(jobs, onStage) }
            self.meguriStructureJob = job
            if self.meguriStructureCancelRequested { job.cancel() }
            let result = await job.result
            self.meguriStructureJob = nil
            answer.value = await self.finishMeguriStructure(result)
            self.meguriStructureRun = nil
            self.meguriStructureStage = nil
            self.meguriStructureJobEnded?()
        }
        guard issued, let task = meguriAction?.task else { return .notSaved }
        await task.value
        guard let value = answer.value else { return .notSaved }
        return try value.get()
    }

    private func meguriStructureStarted(_ stage: MeguriStructureStage, _ stream: AsyncStream<CoreProgress>?, run: UUID,
                                        progress: (@MainActor (NoteTypeSyncStage, UInt64, UInt64?) -> Void)?) {
        // A phase reported after its job ended changes nothing.
        guard meguriStructureRun == run else { return }
        meguriStructureStage = stage
        let shown = MeguriStructureEffect.stage(stage)
        progress?(shown, 0, nil)
        // The save reports "Updating <i> of <n> notes" (D5 step 5).
        guard stage == .save, let stream, let progress else { return }
        Task { @MainActor [weak self] in
            for await report in stream {
                guard self?.meguriStructureRun == run, self?.meguriStructureStage == .save else { return }
                progress(.saving, report.completed, report.total)
            }
        }
    }

    /// The pending state is read again after every job, failed ones included:
    /// an error after the commit still leaves a save this device holds.
    private func finishMeguriStructure(_ result: Result<MeguriStructureOutcome, any Error>) async -> Result<NoteTypeSyncSaveResult, any Error> {
        let pending = await refreshMeguriStructurePending()
        let outcome: MeguriStructureOutcome
        switch result {
        case .success(let value): outcome = value
        case .failure(let error):
            // The op's own refusals saved nothing for certain; they are the
            // section's to show, not sync evidence.
            if Self.meguriStructureRefusal(error) { return .failure(error) }
            meguriLastError = error
            // Saved, though the job did not say so: never "nothing was changed".
            if pending == true { return .success(.saved(uploaded: false)) }
            // Unknown: neither "saved" (the draft would go) nor "nothing was changed".
            if pending == nil { return .success(.unconfirmed) }
            return .failure(error)
        }
        var effect = MeguriStructureEffect.after(outcome)
        // A round stopped because leaving the app asked it to is a cancel, not a sync failure.
        if meguriStructureCancelRequested, effect.result == .syncStopped { effect.result = .cancelled }
        if let report = effect.round {
            meguriRoundReport = report
            meguriCollectionFinalization = nil; meguriMediaFinalization = nil
            reduceMeguriRound(report)
        }
        if let routine = effect.routine { meguriRoutine = routine }
        if effect.exchanged { meguriLastExchange = Int64(clock.now().timeIntervalSince1970 * 1_000) }
        let choose = effect.choose || meguriRoutine == .fullSyncRequired
        if effect.readiness || choose, let facade = meguriBootstrap, let owner = meguriConnectionOwner {
            let expected = generation, revision = hostRevision
            try? await readMeguriReadiness(facade, owner: owner, expected: expected, revision: revision)
            // T19.5 "full sync required": the needs-you row with its Choose action.
            if choose, meguriCurrent(owner, expected, revision), meguriChoiceAllowed, meguriStage == .required {
                meguriStage = .failed; meguriFailureKey = MeguriSetupSequence.fullSyncKey
            }
        }
        if effect.wake { meguriReadyWakePending = true }
        return .success(effect.result)
    }

    /// The op refused before writing (`LossesChanged`, `Stale`, a failed backup, a cancelled plan).
    private static func meguriStructureRefusal(_ error: any Error) -> Bool {
        switch error as? BridgeError {
        case .notetypeStructureRefused?, .stale?, .staleGeneration?: true
        default: false
        }
    }

    /// Tests only: a coordinator with a ready Meguri Sync binding over `jobs`.
    func stageMeguriStructureForTests(_ jobs: any MeguriStructureJobService) {
        meguriStructureJobsOverride = jobs
        ownerState = .attached
        providerState = .renderFixture(selection: .meguri, phase: .setupRequired)
        meguriPhase = .setupRequired; meguriReady = true; meguriRoutine = .idle
    }
}
