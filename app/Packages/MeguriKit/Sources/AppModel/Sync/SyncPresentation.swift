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

/// Presentation only; the retained coordinator owns all work and authority.
@Observable public final class SyncPresentation {
    public var statusPresented = false
    public var settingsRequest: UUID?
    public var settingsSearch = ""
    public var showStatusForCommands = true
    public var connectionPath: [CloudSyncEntry] = []
    public init() {}
}

public struct SyncStatusPresentation {
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
        guard navigation.sheet == nil || navigation.sheet == .accountSync else { return }
        syncPresentation.statusPresented = false
        syncPresentation.connectionPath = []
        navigation.present(.cloudSync(entry))
        navigation.sheetHost = host
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
