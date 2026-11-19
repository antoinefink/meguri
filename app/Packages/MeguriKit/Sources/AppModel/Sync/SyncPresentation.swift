import CoreBridge
import Foundation
import Observation

/// The single sync table is shared by app chrome and feature modules.
public enum SyncText {
    public static var bundle: Bundle { .module }
    public static func value(_ key: String) -> String {
        String(localized: String.LocalizationValue(key), table: "Sync", bundle: .module)
    }
    public static func format(_ key: String, _ arguments: CVarArg...) -> String {
        String(format: value(key), locale: .autoupdatingCurrent, arguments: arguments)
    }
    public static func instant(_ milliseconds: Int64, now: Date) -> String {
        let date = Date(timeIntervalSince1970: Double(milliseconds) / 1_000)
        let formatter = DateFormatter()
        formatter.doesRelativeDateFormatting = abs(date.timeIntervalSince(now)) < 86_400
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        formatter.formattingContext = .middleOfSentence
        return formatter.string(from: date)
    }
}

nonisolated public enum SettingsPage: String, Hashable, Codable, CaseIterable, Sendable {
    case accountSync, collection, ankiServer
}
nonisolated public enum CloudSyncEntry: String, Hashable, Sendable {
    case connect, publicationChoice, replaceLocal, replaceCloud
}

public enum CloudConnectionConfirmation: Sendable, Equatable { case publication, adoption, resume }

/// Presentation only; the retained coordinator owns all work and authority.
@Observable public final class SyncPresentation {
    public var statusPresented = false {
        didSet {
            if statusPresented && !oldValue {
                statusPresentationID = UUID()
                pendingStatusEntry = nil
            }
        }
    }
    public private(set) var statusPresentationID = UUID()
    fileprivate enum StatusDestination {
        case cloudSync(CloudSyncEntry, AppNavigation.SheetHost)
        case accountSync
    }
    fileprivate struct StatusEntry {
        let presentationID: UUID
        let destination: StatusDestination
        let generation: UInt64
        let navigationIntent: UInt64
        let reviewID: ObjectIdentifier?
    }
    fileprivate var pendingStatusEntry: StatusEntry?
    public var statusOwnsInput: Bool { statusPresented || pendingStatusEntry != nil }
    public var statusHandoffPending: Bool { pendingStatusEntry != nil }
    public func cancelStatusHandoff() { pendingStatusEntry = nil }
    public var settingsRequest: UUID?
    public var settingsSearch = ""
    public var meguriAffectedFilesPresented = false
    /// A status action asked for N15.3's sheet; the Sync section presents it.
    public var keepCollectionRequested = false
    public var serviceChanging = false
    public var serviceChangeFailed = false
    public var showStatusForCommands = true
    public var connectionPath: [CloudSyncEntry] = []
    public init() {}
}

public struct SyncStatusPresentation {
    public static func providerNeedsAttention(_ state: SyncProviderState?) -> Bool {
        state?.phase == .blocked || state?.phase == .handoffRecovery
    }
    /// Whether iCloud sync waits on the person: a decision, an account or a
    /// problem that will not clear by itself. Waiting, transfers and a pause
    /// the person chose do not count (N16.2).
    public static func needsUser(_ state: CloudPolicyStatusState?, setup: SyncCoordinator.SetupState) -> Bool {
        // The installation cannot reach iCloud until it is set up: a setup step.
        if setup == .configurationUnavailable { return true }
        return switch state {
        case .needsAttention, .paused(.decisionRequired): true
        case .iCloudUnavailable(let reason): reason != .temporary
        default: false
        }
    }
    /// N16.2 with N15.4's kinds: the review shows the status button for a
    /// needs you row or an issues row, never for waiting rows alone.
    public static func needsUser(_ status: SyncMainStatus) -> Bool { status.needsYou || status.hasIssues }
    /// Sync Now may ask for the status. During review it shows only where the
    /// status button itself shows (N16.2): presenting it pauses the review.
    public static func showsStatusAfterSyncNow(requested: Bool, reviewing: Bool, needsUser: Bool) -> Bool {
        requested && (!reviewing || needsUser)
    }
    public static func showsRecoveryResume(_ recovery: CloudRecoveryStatus) -> Bool {
        if recovery.adoptionRecoveredCancelOnly || recovery.adoptionAfterReview { return false }
        switch recovery.step {
        case .staging, .staged, .ready: return recovery.adoptionTransportWait
        default: return true
        }
    }
    public static func recoveryStatusKey(_ recovery: CloudRecoveryStatus) -> String {
        if recovery.adoptionRecoveredCancelOnly { return "sync.attention.adoptUnproven" }
        if recovery.adoptionAfterReview { return "sync.attention.adoptAfterReview" }
        return switch recovery.step {
        case .confirmed, .bound, .ready: "sync.publish.preparing"
        case .zones: "sync.publish.uploading"
        case .sent, .complete: "sync.publish.finishing"
        case .cancelling: "sync.publish.cancelling"
        case .staging: "sync.adopt.staging"
        case .staged: "sync.adopt.validating"
        }
    }
    public static func cancellationMessageKey(_ recovery: CloudRecoveryStatus) -> String {
        if recovery.adoptionRecoveredCancelOnly { return "sync.cancelOp.adoptUnproven.message" }
        return switch recovery.step {
        case .staging, .staged, .ready: "sync.cancelOp.adopt.message"
        default: recovery.switchSent ? "sync.cancelOp.publish.message.switchSent" : "sync.cancelOp.publish.message"
        }
    }
    public let symbol: String
    public let titleKey: String
    public let detailKey: String?
    public init(state: CloudPolicyStatusState?, setup: SyncCoordinator.SetupState) {
        let values: (String, String, String?)
        switch state {
        case .localOnly:
            values = ("icloud.slash", "sync.status.off.title", "sync.status.localOnly.detail")
        case .iCloudUnavailable(let reason):
            values = ("person.icloud", "sync.status.unavailable.title", Self.unavailableKey(reason))
        case .paused(.decisionRequired):
            // The fence's reason is not exposed by this projection.
            values = ("pause.circle", "sync.status.paused.title", "sync.needsYou.fence.none")
        case .paused(.automaticOff), .paused(.userPaused):
            // P25.2: `userPaused` is never reported; the update hold reads as auto off.
            values = ("pause.circle", "sync.status.automaticOff.title", "sync.status.automaticOff.detail")
        case .needsAttention:
            values = ("exclamationmark.icloud", "sync.status.needsAttention.title", nil)
        case .offline:
            values = ("wifi.slash", "sync.status.offline.title", "sync.status.offline.detail")
        case .syncingCollection:
            values = ("arrow.triangle.2.circlepath.icloud", "sync.status.syncing.title", "sync.status.syncing.detail")
        case .syncingMedia:
            values = ("photo.badge.arrow.down", "sync.status.syncingMedia.title", nil)
        case .changesPending:
            values = ("icloud.and.arrow.up", "sync.status.pending.title", "sync.status.pending.detail")
        case .upToDate:
            values = ("checkmark.icloud", "sync.status.upToDate.title", nil)
        case .awaitingEvidence, nil:
            if setup == .configurationUnavailable {
                values = ("person.icloud", "sync.status.unavailable.title", "sync.status.unavailable.setupNeeded")
            } else {
                values = ("icloud", "sync.button.label", "sync.counts.unknown")
            }
        }
        symbol = values.0; titleKey = values.1; detailKey = values.2
    }
    public static func unavailableKey(_ reason: CloudPolicyUnavailableReason) -> String {
        switch reason {
        case .noAccount: "sync.status.unavailable.noAccount"
        case .restricted: "sync.status.unavailable.restricted"
        case .temporary: "sync.status.unavailable.temporary"
        case .setupNeeded: "sync.status.unavailable.setupNeeded"
        }
    }
    public static func permitsPublication(_ decision: CloudConnectionDecision) -> Bool {
        decision == .confirmUpload || decision == .chooseCollection
    }
    public static func permitsPublication(_ decision: CloudConnectionDecision, on page: CloudSyncEntry) -> Bool {
        switch page {
        case .connect: decision == .confirmUpload
        case .replaceCloud: permitsPublication(decision)
        case .publicationChoice, .replaceLocal: false
        }
    }
    /// UI readiness only; confirmation still consumes the coordinator's retained authority.
    public static func publicationReady(_ decision: CloudConnectionDecision?, accountAvailable: Bool, busy: Bool) -> Bool {
        guard let decision else { return false }
        return permitsPublication(decision) && accountAvailable && !busy
    }
    public static func permitsResume(_ decision: CloudConnectionDecision) -> Bool {
        decision == .confirmResume
    }
    public static func permits(_ intent: CloudConnectionConfirmation, decision: CloudConnectionDecision) -> Bool {
        switch intent {
        case .publication: permitsPublication(decision)
        case .adoption: permitsAdoption(decision)
        case .resume: permitsResume(decision)
        }
    }
    public static func connectionErrorKey(_ failure: CloudConnectionFailure) -> String {
        switch failure {
        case .accountMismatch: "sync.recovery.error.accountMismatch"
        case .accountUnavailable: "sync.recovery.error.accountUnavailable"
        case .authorityChanged: "sync.recovery.error.epochChanged"
        case .epochIncomplete: "sync.recovery.error.epochIncomplete"
        case .closed, .wrongOwner, .consumed, .notOffered: "sync.recovery.error.notOffered"
        case .busy: "sync.recovery.error.other"
        case .keyUnavailable, .unreadableControl, .incomingStagingUnavailable, .mediaApplyUnavailable,
             .resetRecoveryUnavailable, .tracking, .incomplete, .fileIdentityUnavailable: "sync.recovery.error.other"
        }
    }
    public static func permitsAdoption(_ decision: CloudConnectionDecision) -> Bool {
        decision == .confirmAdoption || decision == .chooseCollection
    }
    public static func availability(_ projected: CloudPolicySyncNowAvailability?,
                                    setup: SyncCoordinator.SetupState) -> CloudPolicySyncNowAvailability {
        if let projected { return projected }
        return .unavailable(setup == .configurationUnavailable ? .iCloudUnavailable : .collectionUnavailable)
    }
    public static func reviewAvailabilityKey(_ controls: CloudPolicyControls?) -> String {
        controls?.automaticEligible == true ? "sync.now.unavailable.reviewActive" : "sync.now.unavailable.reviewActive.manual"
    }
    public static func errorKey(_ error: CloudPolicyOperationError) -> String {
        switch error {
        case .noAccount: "sync.error.noAccount"
        case .configuration: "sync.status.unavailable.setupNeeded"
        case .offline: "sync.error.offline"
        case .throttled: "sync.error.throttled"
        case .quotaFull: "sync.error.quotaFull"
        case .localStorageFull: "sync.error.localStorageFull"
        case .trackingError: "sync.error.trackingError"
        case .conflictRecovery: "sync.error.conflictRecovery"
        case .unsupportedSchema: "sync.error.unsupportedSchema"
        case .other: "sync.error.cloudError"
        }
    }
}

extension ReviewCollection {
    public var syncPresentation: SyncPresentation {
        navigation.featureState { SyncPresentation() }
    }
    public func cloudPreviewEntryUnavailableReason(_ entry: CloudSyncEntry) -> String? {
        // P25.2: the update hold disables nothing; the entry clears it.
        if sync.setupState == .configurationUnavailable { return "sync.status.unavailable.setupNeeded" }
        if sync.connectionService == nil,
           !(entry == .connect && lifecycle == .noCollection && sync.setupState == .prepared) {
            return "sync.now.unavailable.collectionUnavailable"
        }
        if sync.setupState == .failed { return "sync.recovery.error.other" }
        if sync.connectionOperationActive { return "sync.recovery.error.busy" }
        if sync.connectionPreviewActive { return "sync.connect.reading" }
        return nil
    }
    /// Every control uses Rust's projection, or the actual installation fact
    /// while that projection is absent. No binding or account is inferred.
    public func syncNowAvailability() -> CloudPolicySyncNowAvailability {
        if sync.providerState?.selection == .meguri {
            switch sync.meguriStatus.route {
            case .round, .account: return .available
            case .disabled: return .unavailable(.recoveryRequired)
            }
        }
        return SyncStatusPresentation.availability(sync.status?.syncNow, setup: sync.setupState)
    }
    @discardableResult public func requestSyncNow(source: CloudPolicySource = .command, showStatus: Bool = false) -> Bool {
        guard navigation.sheet == nil, syncNowAvailability() == .available else { return false }
        if sync.providerState?.selection == .meguri, case .account(let key) = sync.meguriStatus.route {
            if key == "sync.meguri.media.showAffected" { syncPresentation.meguriAffectedFilesPresented = true }
            #if os(macOS)
            syncPresentation.settingsRequest = UUID()
            #else
            _ = navigation.openAccountSync()
            #endif
            return true
        }
        sync.syncNow(source: source)
        if SyncStatusPresentation.showsStatusAfterSyncNow(requested: showStatus, reviewing: review != nil,
                                                          needsUser: sync.needsUser) {
            syncPresentation.statusPresented = true
        }
        return true
    }
    /// A preview cannot enable sync. Preparing the existing entry location also
    /// preserves the collection's subscription and platform-configuration order.
    public func prepareCloudConnectionPreview(allowDuringReview: Bool = false) async throws {
        guard review == nil || allowDuringReview else { throw BridgeError.sessionActive }
        // A recovery call the person starts clears the update hold first (P25.2).
        guard await sync.clearUpdateHold() else { throw BridgeError.unavailable(reason: "paused") }
        try await prepareCloudConnectionHost()
        try Task.checkCancellation()
        guard sync.setupState != .configurationUnavailable else {
            throw BridgeError.unavailable(reason: "configuration")
        }
        guard sync.connectionService != nil else { throw BridgeError.unavailable(reason: "collectionUnavailable") }
        guard !sync.settings.paused else { throw BridgeError.unavailable(reason: "paused") }
        sync.beginConnectionPreview()
    }
    public func presentCloudSync(_ entry: CloudSyncEntry, host: AppNavigation.SheetHost = .main) {
        syncPresentation.cancelStatusHandoff()
        guard navigation.sheet == nil || navigation.sheet == .accountSync else { return }
        syncPresentation.statusPresented = false
        syncPresentation.connectionPath = []
        navigation.present(.cloudSync(entry))
        navigation.sheetHost = host
    }
    /// Compact status and its destination use separate native presentations. Only the
    /// status sheet's actual onDismiss may consume this presentation request.
    public func queueCloudSyncAfterStatusDismissal(_ entry: CloudSyncEntry, host: AppNavigation.SheetHost = .main) {
        queueSyncStatusDestination(.cloudSync(entry, host))
    }
    public func queueAccountSyncAfterStatusDismissal() {
        queueSyncStatusDestination(.accountSync)
    }
    private func queueSyncStatusDestination(_ destination: SyncPresentation.StatusDestination) {
        let presentation = syncPresentation
        guard presentation.statusPresented, !presentation.statusHandoffPending,
              navigation.sheet == nil, canCompleteSyncStatusHandoff else { return }
        presentation.pendingStatusEntry = .init(presentationID: presentation.statusPresentationID,
            destination: destination, generation: generation,
            navigationIntent: navigation.intentRevision, reviewID: navigation.review?.id)
        presentation.statusPresented = false
    }
    public func compactSyncStatusDidDismiss(_ presentationID: UUID) {
        let presentation = syncPresentation
        guard let request = presentation.pendingStatusEntry, request.presentationID == presentationID else { return }
        presentation.cancelStatusHandoff()
        guard !presentation.statusPresented, canCompleteSyncStatusHandoff,
              request.generation == generation, request.navigationIntent == navigation.intentRevision,
              request.reviewID == navigation.review?.id, navigation.sheet == nil else { return }
        switch request.destination {
        case .cloudSync(let entry, let host): presentCloudSync(entry, host: host)
        case .accountSync: navigation.openAccountSync()
        }
    }
}

extension AppNavigation {
    /// A settings sheet over review never changes its section, deck or session.
    @discardableResult public func openAccountSync() -> Bool {
        guard sheet == nil else { return false }
        if review != nil { present(.accountSync) }
        else {
            // N15.1: the Sync section is on the flat Settings page itself; beside
            // a section list, Sync becomes the selected section.
            featureState { SyncPresentation() }.settingsRequest = UUID()
            show(.settings)
            setPath([], for: .settings)
        }
        return true
    }
}

// Display values carry no command or binding authority.
public enum MeguriSetupStage: Sendable, Equatable {
    case required, preparing, preview, collection, media, mediaBlocked
    case acceptanceUnknown, acceptedLocalIncomplete, failed
}
public enum MeguriRoutineState: Sendable, Equatable {
    case unreported, exchanging, idle, pending, waiting, mediaBlocked, authentication
    case acceptanceUnknown, fullSyncRequired, network, server, cancelled, failed
}
public enum MeguriSyncNowRoute: Equatable, Sendable {
    case disabled(String?), account(String), round
}
/// What Sync Now does for Meguri Sync, and whether the service is busy or
/// waits on the person. The status text itself is `SyncKinds.meguri` (T19.5).
public struct MeguriSyncStatusPresentation {
    public let route: MeguriSyncNowRoute
    public let isBusy: Bool
    /// Whether Meguri Sync waits on the person (N16.2): a sign-in, a
    /// collection choice, a setup step, or a round or setup that stopped and
    /// will not retry. Transfers, pending changes, failures that retry and
    /// the state before the first report do not count.
    public let needsUser: Bool
    /// Read-only values for the replacement sheet; no preview or command authority.
    public static func replacementCountRows(cards: UInt64, notes: UInt64, reviews: UInt64) -> [(label: String, value: String)] {
        [(SyncText.value("sync.counts.cards"), cards.formatted(.number)),
         (SyncText.value("sync.counts.notes"), notes.formatted(.number)),
         (SyncText.value("sync.counts.reviews"), reviews.formatted(.number))]
    }
    public init(stopped: Bool, connection: MeguriProviderConnectionPhase, stage: MeguriSetupStage?,
                routine: MeguriRoutineState?, settling: Bool = false) {
        // T19.7's reasons.
        if stopped || connection == .blocked {
            route = .disabled("sync.meguri.now.unavailable.stopped.sync")
        } else if let routine {
            route = switch routine {
            case .acceptanceUnknown: .disabled("sync.meguri.now.unavailable.acceptanceUnknown")
            case .fullSyncRequired: .account("sync.decision.action")
            case .mediaBlocked: .account("sync.meguri.media.showAffected")
            case .authentication: .disabled("sync.meguri.now.unavailable.signIn.sync")
            case .exchanging, .failed: .disabled(nil)
            case .unreported, .idle, .pending, .waiting, .network, .server, .cancelled: settling ? .disabled(nil) : .round
            }
        } else { route = .account("sync.meguri.setup.continue") }
        if stopped || connection == .blocked { isBusy = false }
        else if connection == .authenticating { isBusy = true }
        else if connection != .setupRequired { isBusy = false }
        else if let stage { isBusy = stage == .preparing || stage == .collection || stage == .media }
        else { isBusy = routine == .exchanging }
        if stopped || connection == .blocked { needsUser = true }
        else if connection == .unconfigured || connection == .authenticating { needsUser = false }
        else if connection == .signedOut || connection == .cancelled { needsUser = true }
        else if let stage {
            needsUser = switch stage {
            case .required, .preview, .mediaBlocked, .acceptanceUnknown, .acceptedLocalIncomplete, .failed: true
            case .preparing, .collection, .media: false
            }
        } else if let routine {
            needsUser = switch routine {
            case .failed, .mediaBlocked, .authentication, .acceptanceUnknown, .fullSyncRequired: true
            case .unreported, .exchanging, .idle, .pending, .waiting, .network, .server, .cancelled: false
            }
        } else { needsUser = true } // Signed in, setup not continued.
    }
    public static func canDiscard(_ receipt: MeguriBootstrapReceipt) -> Bool {
        receipt.acceptance == .notSent && receipt.phase != .needsDecision
            && receipt.boundary != .activationInterrupted
    }
    public static func canRecover(_ receipt: MeguriBootstrapReceipt) -> Bool {
        receipt.direction == .export && receipt.acceptance == .exactOk
            && (receipt.phase == .remoteAccepted || receipt.phase == .activating
                || receipt.boundary == .activationInterrupted)
    }
    /// A classification only: T19.8 withdraws its text, and no view shows it.
    public static let cancelledFailureKey = "meguri.setup.cancelled"
    public static func failureKey(_ failure: MeguriBootstrapFailure) -> String? {
        switch failure {
        case .accountChanged, .connectionChanged, .username: "sync.meguri.setup.error.identityChanged"
        case .publicationChanged: "sync.meguri.setup.error.remoteChanged"
        case .documentChanged: "sync.meguri.setup.error.localChanged"
        case .storage, .diskFull: "sync.meguri.setup.error.storage"
        case .cancelled: cancelledFailureKey
        case .transfer(let value): transferFailureKey(value)
        default: nil
        }
    }
    public static func transferFailureKey(_ value: ManualTransferFailure) -> String? {
        if value.boundary == .activationInterrupted { return "sync.meguri.setup.error.activationInterrupted" }
        switch value.issue {
        case .safetyBackup: return "sync.meguri.setup.error.backupFailed"
        case .invalidDownload: return "sync.meguri.setup.error.invalidDownload"
        case .storage, .diskFull: return "sync.meguri.setup.error.storage"
        case .cancelled, .interrupted: return cancelledFailureKey
        case .server:
            switch value.serverKind {
            case "auth_failed": return "sync.meguri.setup.error.authentication"
            case "offline", "network": return "sync.meguri.setup.error.remoteUnavailable"
            case "server_error", "unexpected_status", "server_message": return "sync.meguri.setup.error.server"
            default: return nil
            }
        default:
            return switch value.boundary {
            case .localChanged: "sync.meguri.setup.error.localChanged"
            case .remoteChanged: "sync.meguri.setup.error.remoteChanged"
            case .identityChanged: "sync.meguri.setup.error.identityChanged"
            default: nil
            }
        }
    }
}
