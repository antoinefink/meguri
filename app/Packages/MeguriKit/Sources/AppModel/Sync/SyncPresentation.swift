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
    public var serviceChanging = false
    public var serviceChangeFailed = false
    public var meguriSigningOut = false
    public var meguriSignOutFailed = false
    public var showStatusForCommands = true
    public var connectionPath: [CloudSyncEntry] = []
    public init() {}
}

public struct SyncStatusPresentation {
    public static func providerNeedsAttention(_ state: SyncProviderState?) -> Bool {
        state?.phase == .blocked || state?.phase == .handoffRecovery
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
            values = ("icloud.slash", "sync.status.localOnly.title", "sync.status.localOnly.detail")
        case .iCloudUnavailable(let reason):
            values = ("person.icloud", "sync.status.unavailable.title", Self.unavailableKey(reason))
        case .paused(let reason):
            let title = reason == .automaticOff ? "sync.status.automaticOff.title" : "sync.status.paused.title"
            let detail: String = switch reason {
            case .decisionRequired: "sync.decision.title.none" // reason is not exposed by this projection
            case .userPaused: "sync.status.paused.user"
            case .automaticOff: "sync.status.automaticOff.detail"
            }
            values = ("pause.circle", title, detail)
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
    public static func publicationReady(_ decision: CloudConnectionDecision?, accountAvailable: Bool,
                                        paused: Bool, busy: Bool) -> Bool {
        guard let decision else { return false }
        return permitsPublication(decision) && accountAvailable && !paused && !busy
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
        if sync.settings.paused { return "sync.status.paused.user" }
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
            case .disabled: return .unavailable(sync.settings.paused ? .paused : .recoveryRequired)
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
        if showStatus { syncPresentation.statusPresented = true }
        return true
    }
    /// A preview cannot enable sync. Preparing the existing entry location also
    /// preserves the collection's subscription and platform-configuration order.
    public func prepareCloudConnectionPreview(allowDuringReview: Bool = false) async throws {
        guard review == nil || allowDuringReview else { throw BridgeError.sessionActive }
        guard !sync.settings.paused else { throw BridgeError.unavailable(reason: "paused") }
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
            show(.settings)
            setPath([.settingsPage(.accountSync)], for: .settings)
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
    case unreported, exchanging, idle, pending, mediaBlocked, authentication
    case acceptanceUnknown, fullSyncRequired, network, server, cancelled, failed
}
public enum MeguriSyncNowRoute: Equatable, Sendable {
    case disabled(String?), account(String), round
}
public struct MeguriSyncStatusPresentation {
    public let titleKey: String
    public let detailKey: String?
    public let route: MeguriSyncNowRoute
    public var isBusy: Bool {
        switch titleKey {
        case "sync.meguri.status.signingIn.title", "sync.meguri.status.preparing.title",
             "sync.meguri.status.transferringCollection.title", "sync.meguri.status.transferringMedia.title",
             "sync.meguri.status.syncingCollection.title", "sync.meguri.status.syncingMedia.title": true
        default: false
        }
    }
    public var symbol: String {
        if isBusy { return "arrow.triangle.2.circlepath" }
        return switch titleKey {
        case "sync.meguri.status.notSetUp.title", "sync.meguri.status.signedOut.title",
             "sync.meguri.status.signedIn.title", "sync.meguri.status.setup.title",
             "sync.meguri.status.chooseCollection.title", "sync.meguri.status.signInRequired.title":
            "person.crop.circle.badge.exclamationmark"
        case "sync.meguri.status.paused.title": "pause.circle"
        case "sync.meguri.status.pending.title": "arrow.up.circle"
        case "sync.meguri.status.on.title": "checkmark.circle"
        default: "exclamationmark.arrow.triangle.2.circlepath"
        }
    }
    /// Read-only values for the replacement sheet; no preview or command authority.
    public static func replacementCountRows(cards: UInt64, notes: UInt64, reviews: UInt64) -> [(label: String, value: String)] {
        [(SyncText.value("sync.counts.cards"), cards.formatted(.number)),
         (SyncText.value("sync.counts.notes"), notes.formatted(.number)),
         (SyncText.value("sync.counts.reviews"), reviews.formatted(.number))]
    }
    public init(stopped: Bool, connection: MeguriProviderConnectionPhase, stage: MeguriSetupStage?,
                localCollection: Bool, paused: Bool, routine: MeguriRoutineState?, failureKey: String? = nil) {
        if stopped || connection == .blocked {
            route = .disabled("sync.meguri.now.unavailable.stopped")
        } else if paused {
            route = .disabled("sync.meguri.now.unavailable.paused")
        } else if let routine {
            route = switch routine {
            case .acceptanceUnknown: .disabled("sync.meguri.now.unavailable.acceptanceUnknown")
            case .fullSyncRequired: .account("sync.meguri.now.chooseCollection")
            case .mediaBlocked: .account("sync.meguri.media.showAffected")
            case .authentication: .disabled("sync.meguri.now.unavailable.signIn")
            case .exchanging, .failed: .disabled(nil)
            case .unreported, .idle, .pending, .network, .server, .cancelled: .round
            }
        } else { route = .account("sync.meguri.setup.continue") }
        let pair: (String, String?)
        if stopped { pair = ("stopped", nil) }
        else if connection == .blocked { pair = ("connectionBlocked", "sync.meguri.status.connectionBlocked.detail") }
        else if connection == .unconfigured { pair = ("notSetUp", "sync.meguri.status.notSetUp.detail") }
        else if connection == .signedOut { pair = ("signedOut", "sync.meguri.status.signedOut.detail") }
        else if connection == .cancelled { pair = ("signedOut", "sync.meguri.status.cancelled.detail") }
        else if connection == .authenticating { pair = ("signingIn", nil) }
        else if let stage {
            pair = switch stage {
            case .required: ("setup", localCollection ? "sync.meguri.setup.choose" : "sync.meguri.setup.download.detail")
            case .preparing: ("preparing", "sync.meguri.setup.preparing")
            case .preview: ("setup", "sync.meguri.setup.review")
            case .collection: ("transferringCollection", nil)
            case .media: ("transferringMedia", "sync.meguri.setup.collectionDone")
            case .mediaBlocked: ("mediaBlocked", "sync.meguri.media.blocked")
            case .acceptanceUnknown: ("acceptanceUnknown", "sync.meguri.setup.acceptanceUnknown")
            case .acceptedLocalIncomplete: ("acceptedLocalIncomplete", "sync.meguri.setup.acceptedLocalIncomplete")
            case .failed: ("setupFailed", failureKey)
            }
        } else if paused { pair = ("paused", "sync.meguri.status.paused.detail") }
        else if let routine {
            pair = switch routine {
            case .unreported, .failed: ("stopped", nil)
            case .exchanging: ("syncingCollection", nil)
            case .idle: ("on", nil)
            case .pending: ("pending", "sync.meguri.status.pending.detail")
            case .mediaBlocked: ("mediaBlocked", "sync.meguri.media.blocked")
            case .authentication: ("signInRequired", "sync.meguri.status.signInRequired.detail")
            case .acceptanceUnknown: ("acceptanceUnknown", "sync.meguri.round.acceptanceUnknown")
            case .fullSyncRequired: ("chooseCollection", "sync.meguri.round.fullSyncRequired")
            case .network: ("failed", "sync.meguri.round.network")
            case .server: ("failed", "sync.meguri.round.server")
            case .cancelled: ("failed", "sync.meguri.round.cancelled")
            }
        } else { pair = ("signedIn", "sync.meguri.status.setupUnavailable") }
        titleKey = "sync.meguri.status.\(pair.0).title"; detailKey = pair.1
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
    public static func failureKey(_ failure: MeguriBootstrapFailure) -> String? {
        switch failure {
        case .accountChanged, .connectionChanged, .username: "sync.meguri.setup.error.identityChanged"
        case .publicationChanged: "sync.meguri.setup.error.remoteChanged"
        case .documentChanged: "sync.meguri.setup.error.localChanged"
        case .storage, .diskFull: "sync.meguri.setup.error.storage"
        case .cancelled: "sync.meguri.setup.error.cancelled"
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
        case .cancelled, .interrupted: return "sync.meguri.setup.error.cancelled"
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
