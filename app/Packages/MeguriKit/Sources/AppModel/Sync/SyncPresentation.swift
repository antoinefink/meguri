import CoreBridge
import Foundation
import Observation

/// The single sync table is shared by app chrome and feature modules.
public enum SyncText {
    public static var bundle: Bundle { .module }
    public static func value(_ key: String) -> String {
        String(localized: String.LocalizationValue(key), table: "Sync", bundle: .module)
    }
    /// Counts take the locale's own digits and grouping from the system's
    /// formatting ("4,431", "4.431"), never from string building.
    public static func format(_ key: String, _ arguments: CVarArg...) -> String {
        format(key, locale: .autoupdatingCurrent, arguments)
    }
    static func format(_ key: String, locale: Locale, _ arguments: [CVarArg]) -> String {
        String(format: value(key), locale: locale, arguments: arguments)
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
    case accountSync, collection, backups, ankiServer, integrations
}
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
        case accountSync(keepCollection: Bool)
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
    /// N23.6: the sync the person started from the toolbar button, until it ends.
    public var buttonFeedback = SyncButton.Feedback()
    /// N23.1: the panel's measured height in the sheet and in the popover's sheet
    /// (iPhone), so a sheet opens at its one detent. Measured by the shell, out of
    /// the toolbar, while no sheet shows (`SyncPanelMeasure`).
    public var panelSheetHeight: Double?
    public var panelPopoverHeight: Double?
    /// N23.6: the one sync button that presents the panel: the last one that
    /// appeared (the screen on top) or was tapped. A stack keeps the buttons of
    /// the screens under it, so only this one presents.
    public var statusPresenter: UUID?
    public init() {}
}

public struct SyncStatusPresentation {
    public static func providerNeedsAttention(_ state: SyncProviderState?) -> Bool {
        state?.phase == .blocked || state?.phase == .handoffRecovery
    }
    /// N16.2 with N15.4's kinds: the review shows the status button for a
    /// needs you row or an issues row, never for waiting rows alone.
    public static func needsUser(_ status: SyncMainStatus) -> Bool { status.needsYou || status.hasIssues }
    /// Sync Now may ask for the status. During review it shows only where the
    /// status button itself shows (N16.2): presenting it pauses the review.
    public static func showsStatusAfterSyncNow(requested: Bool, reviewing: Bool, needsUser: Bool) -> Bool {
        requested && (!reviewing || needsUser)
    }
}

extension ReviewCollection {
    public var syncPresentation: SyncPresentation {
        navigation.featureState { SyncPresentation() }
    }
    /// Every control uses Rust's projection, or the actual installation fact
    /// while that projection is absent. No binding or account is inferred.
    public func syncNowAvailability() -> SyncNowAvailability {
        if sync.providerState?.selection == .meguri {
            switch sync.meguriStatus.route {
            case .round, .account: return .available
            case .disabled: return .unavailable(.recoveryRequired)
            }
        }
        return .unavailable(.notConnected)
    }
    @discardableResult public func requestSyncNow(source: SyncSource = .command, showStatus: Bool = false) -> Bool {
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
    public enum SyncServiceChangeOutcome: Sendable, Equatable {
        case changed
        /// The selection was no longer the one the person saw: nothing changed, and they choose again.
        case superseded
        case failed
    }
    /// The Service control's change (N15.2 row 1), for a destination the person
    /// chose while `source` was the selection shown. A failure records its step
    /// and reason for Copy Debug Info; the control stays usable, so choosing
    /// again is the retry.
    public func changeSyncService(to target: SyncProviderSelection, from source: SyncProviderSelection?) async -> SyncServiceChangeOutcome {
        let destination = switch target { case .meguri: "meguri"; case .localOnly: "off" }
        func failed(_ step: SyncServiceChangeFailure.Step, _ reason: SyncServiceChangeFailure.Reason) -> SyncServiceChangeOutcome {
            // The coordinator that owns the selection now, also after a handoff replaced it.
            sync.serviceChangeFailed(.init(destination: destination, step: step, reason: reason))
            return .failed
        }
        sync.serviceChangeBegan()
        do { try await prepareSyncServiceSelection() }
        catch { return failed(.preparation, .init(ProviderFailureReason(error))) }
        let current = sync
        // A choice in the Service control clears the update hold, by writing
        // `paused: false` before the change itself is asked for (P25.2).
        guard await current.clearUpdateHold() else { return failed(.updateHold, .refused) }
        // The selection is not reserved across those waits: a change of it
        // ends this switch, and the person confirms again.
        // No state yet shows as Off, the same as `.localOnly`.
        guard (current.providerState?.selection ?? .localOnly) == (source ?? .localOnly) else { return .superseded }
        if await current.changeProvider(to: target, removeMeguriCredentials: source == .meguri) { return .changed }
        return failed(.handoff, .init(current.providerChangeFailureReason))
    }
    /// Compact status and its destination use separate native presentations. Only the
    /// status sheet's actual onDismiss may consume this presentation request.
    public func queueAccountSyncAfterStatusDismissal(keepCollection: Bool = false) {
        queueSyncStatusDestination(.accountSync(keepCollection: keepCollection))
    }
    /// N15.3: only the Sync section presents the keep-collection sheet, so a status
    /// action outside Settings opens that section first. The request is recorded
    /// only once the section is opening, so a refused route leaves nothing behind.
    @discardableResult public func openAccountSyncFromStatus(keepCollection: Bool) -> Bool {
        guard navigation.openAccountSync() else { return false }
        if keepCollection { syncPresentation.keepCollectionRequested = true }
        return true
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
        case .accountSync(let keepCollection): openAccountSyncFromStatus(keepCollection: keepCollection)
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
    /// The media step stopped before it finished (a quit, a lost connection, a
    /// server error) and continues by itself (MP6.5, resume fix).
    case mediaPaused
    /// The media step failed again after its automatic attempts: the person retries it.
    case mediaStopped
    case acceptanceUnknown, acceptedLocalIncomplete, failed
}
public enum MeguriRoutineState: Sendable, Equatable {
    case unreported, exchanging, idle, pending, waiting, mediaBlocked, authentication
    case acceptanceUnknown, fullSyncRequired, network, server, cancelled, failed
}
/// One progress indicator's value: the view keeps one identity for both cases.
public enum MeguriProgressValue: Equatable, Sendable {
    case indeterminate, fraction(Double)
    public var fraction: Double? { if case .fraction(let value) = self { value } else { nil } }
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
                routine: MeguriRoutineState?, settling: Bool = false,
                structureRunning: Bool = false, structureUploadPending: Bool = false) {
        // T19.7's reasons.
        if stopped || connection == .blocked {
            route = .disabled("sync.meguri.now.unavailable.stopped.sync")
        } else if structureRunning || (structureUploadPending && (stage == nil || stage == .required) && routine != .authentication) {
            // MP7: the job holds the provider; a waiting upload comes before any round (its Upload… is the status action).
            route = .disabled(nil)
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
        else if structureRunning { isBusy = true }
        else if connection == .authenticating { isBusy = true }
        else if connection != .setupRequired { isBusy = false }
        else if let stage { isBusy = stage == .preparing || stage == .collection || stage == .media }
        else { isBusy = routine == .exchanging }
        if stopped || connection == .blocked { needsUser = true }
        else if structureRunning { needsUser = false }
        else if structureUploadPending, stage == nil || stage == .required, connection == .setupRequired { needsUser = true }
        else if connection == .unconfigured || connection == .authenticating { needsUser = false }
        else if connection == .signedOut || connection == .cancelled { needsUser = true }
        else if let stage {
            needsUser = switch stage {
            case .required, .preview, .mediaBlocked, .mediaStopped, .acceptanceUnknown, .acceptedLocalIncomplete, .failed: true
            case .preparing, .collection, .media, .mediaPaused: false
            }
        } else if let routine {
            needsUser = switch routine {
            case .failed, .mediaBlocked, .authentication, .acceptanceUnknown, .fullSyncRequired: true
            case .unreported, .exchanging, .idle, .pending, .waiting, .network, .server, .cancelled: false
            }
        } else { needsUser = true } // Signed in, setup not continued.
    }
    /// The Account section's progress while Meguri work runs: none while idle
    /// or stopped. The media step's value is the whole step's files done over
    /// files done and left, never one request's bytes, so batches cannot switch
    /// it between a bar and a spinner. Other steps use their job's own total.
    public static func progress(busy: Bool, stopped: Bool, stage: MeguriSetupStage?,
                                completed: UInt64?, total: UInt64?,
                                mediaDone: UInt64?, mediaAwaiting: UInt64?) -> MeguriProgressValue? {
        guard busy, !stopped else { return nil }
        if stage == .media {
            guard let done = mediaDone, let awaiting = mediaAwaiting else { return .indeterminate }
            let all = Double(done) + Double(awaiting)
            return all > 0 ? .fraction(Double(done) / all) : .indeterminate
        }
        guard let completed, let total, total > 0 else { return .indeterminate }
        return .fraction(min(1, Double(completed) / Double(total)))
    }
    /// MP6.5: a provider phase that turns blocked stops the Meguri network work
    /// already running: the setup's media step or a round. A collection step is
    /// left to Core, whose replacement keeps its own exclusive phase.
    public static func stopsWorkOnBlock(was: SyncProviderPhase?, now: SyncProviderPhase?,
                                        stage: MeguriSetupStage?, roundRunning: Bool) -> Bool {
        guard now == .blocked, was != .blocked else { return false }
        if let stage { return stage == .media }
        return roundRunning
    }
    public static func canDiscard(_ receipt: MeguriBootstrapReceipt) -> Bool {
        receipt.acceptance == .notSent && receipt.phase != .needsDecision
            && receipt.boundary != .activationInterrupted
    }
    public static func canRecover(_ receipt: MeguriBootstrapReceipt) -> Bool {
        // An accepted upload in the preview phase is its recovery preview:
        // a first preview has sent nothing.
        receipt.direction == .export && receipt.acceptance == .exactOk
            && (receipt.phase == .remoteAccepted || receipt.phase == .activating || receipt.phase == .preview
                || receipt.boundary == .activationInterrupted)
    }
    /// A classification only: T19.8 withdraws its text, and no view shows it.
    public static let cancelledFailureKey = "meguri.setup.cancelled"
    public static func failureKey(_ failure: MeguriBootstrapFailure) -> String? {
        switch failure {
        case .accountChanged, .connectionChanged, .username: "sync.meguri.setup.error.identityChanged"
        case .publicationChanged, .setupRemoteChanged: "sync.meguri.setup.error.remoteChanged"
        case .documentChanged, .setupLocalChanged: "sync.meguri.setup.error.localChanged"
        case .setupLocalUnusable: MeguriSetupSequence.localUnusableKey
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
