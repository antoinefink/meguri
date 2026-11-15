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
    public var showStatusForCommands = true
    public var connectionPath: [CloudSyncEntry] = []
    public init() {}
}

public struct SyncStatusPresentation {
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
        SyncStatusPresentation.availability(sync.status?.syncNow, setup: sync.setupState)
    }
    @discardableResult public func requestSyncNow(source: CloudPolicySource = .command, showStatus: Bool = false) -> Bool {
        guard navigation.sheet == nil, syncNowAvailability() == .available else { return false }
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
