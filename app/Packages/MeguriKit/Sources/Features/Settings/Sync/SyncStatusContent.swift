import AppModel
import CoreBridge
import Foundation
import SwiftUI

struct SyncNowButton: View {
    let collection: ReviewCollection
    var identifier = "sync.status.syncNow"
    var showsUnavailableReason = true
    /// N23.3: the panel's Sync Now is disabled while the status moves, even
    /// where a press would join the running exchange (P12.1).
    var disabledWhileMoving = false
    private var meguri: Bool { collection.sync.providerState?.selection == .meguri }
    private var reason: String? { SyncStatusCopy.unavailable(collection) }
    private var available: Bool {
        // Meguri Sync runs a round only once its setup has completed (MP5).
        if meguri { return collection.sync.meguriStatus.route == .round }
        return collection.syncNowAvailability() == .available
    }
    var body: some View {
        Button(SyncText.value("sync.now.command")) { collection.requestSyncNow() }
            .disabled(!SyncPanel.syncNowEnabled(available: available && collection.navigation.sheet == nil,
                moving: disabledWhileMoving && collection.sync.panelProgress != nil))
            // N23.3: in the panel only a reason makes a help tag.
            .help(reason ?? (disabledWhileMoving ? "" : SyncText.value("sync.now.command")))
            .accessibilityIdentifier(identifier)
            #if DEBUG
            // Observe the actual coordinator; never query a transport from a view.
            .accessibilityValue(meguri
                ? "meguri-round-success-sequence:\(collection.sync.debugMeguriRoundSuccessSequence)"
                : DebugCloudDiagnostics.nativeFetchSequenceValue)
            #endif
        if showsUnavailableReason, let reason {
            Text(reason).font(.footnote).foregroundStyle(.secondary)
                .accessibilityIdentifier("\(identifier)Reason")
        }
    }
}

enum SyncStatusCopy {
    /// The main status's detail as the Sync section's status row and the panel show it.
    @MainActor static func detail(_ sync: SyncCoordinator, now: Date) -> String? {
        let status = sync.mainStatus
        if let key = status.detailKey {
            // N23.1: a detail that repeats the title is not shown ("Setting Up…").
            guard status.shownDetailKey != nil else { return nil }
            if let count = status.detailCount { return SyncText.format(key, Int64(clamping: count)) }
            return SyncText.value(key)
        }
        // Both services use "Last synced" for a completed exchange (MP6.6).
        guard status.titleKey == "sync.status.upToDate.title" else { return nil }
        let instant: Int64?
        if sync.providerState?.selection == .meguri {
            instant = sync.meguriLastExchange
        } else if case .upToDate(let at) = sync.status?.state {
            instant = at
        } else if case .upToDate(let at) = sync.status?.stateWithoutAttention {
            // T19.4: with only issues present the row shows the state underneath, detail included.
            instant = at
        } else { instant = nil }
        return instant.map { SyncText.format("sync.status.upToDate.detail", SyncText.instant($0, now: now)) }
    }
    static func keepsLocally(_ status: CloudPolicyStatus) -> Bool {
        guard status.attention[.collectionCorrupt] == nil, status.attention[.localStorageFull] == nil,
              status.attention[.trackingError] == nil else { return false }
        switch status.state {
        case .iCloudUnavailable, .paused, .offline, .needsAttention: return true
        default: return false
        }
    }
    static func unavailable(_ collection: ReviewCollection) -> String? {
        if collection.sync.providerState?.selection == .meguri {
            if case .disabled(let key) = collection.sync.meguriStatus.route {
                return key.map { SyncText.value($0) }
            }
            return nil
        }
        guard case .unavailable(let reason) = collection.syncNowAvailability() else { return nil }
        let key: String
        switch reason {
        case .collectionUnavailable: key = "sync.now.unavailable.collectionUnavailable"
        case .reviewActive:
            key = SyncStatusPresentation.reviewAvailabilityKey(collection.sync.status?.controls)
        case .notConnected: key = SyncKinds.syncNowOffKey
        // Never reported: P12.1 row 4 is removed (P25.2).
        case .paused: return nil
        case .iCloudUnavailable:
            if collection.sync.status == nil, collection.sync.setupState == .configurationUnavailable {
                key = "sync.status.unavailable.setupNeeded"
            } else if case .iCloudUnavailable(let reason) = collection.sync.status?.state, reason != .noAccount {
                key = SyncStatusPresentation.unavailableKey(reason)
            } else { key = "sync.now.unavailable.iCloudUnavailable" }
        case .decisionRequired: key = "sync.now.unavailable.decisionRequired"
        case .recoveryRequired: key = "sync.now.unavailable.recoveryRequired"
        }
        return SyncText.value(key)
    }
    static func lines(_ status: CloudPolicyStatus, now: Date) -> [String] {
        var lines: [String] = []
        if status.pending > 0 { lines.append(SyncText.format("sync.details.toSend", Int64(clamping: status.pending))) }
        if status.heldBack > 0 { lines.append(SyncText.value("sync.details.heldBack")) }
        if status.incomingWaiting > 0 { lines.append(SyncText.format("sync.details.incoming", Int64(clamping: status.incomingWaiting))) }
        if case .upToDate = status.state { /* The status detail already carries its qualified instant. */ }
        else if let completed = status.lastCompleted {
            lines.append(SyncText.format("sync.details.lastSync", SyncText.instant(completed.endedMilliseconds, now: now)))
        }
        if let attempt = status.lastAttempt,
           status.lastCompleted.map({ attempt.endedMilliseconds > $0.endedMilliseconds }) ?? true {
            lines.append(SyncText.format("sync.details.lastAttempt", resultTitle(attempt.outcome),
                SyncText.instant(attempt.endedMilliseconds, now: now)))
            lines += resultLines(attempt.outcome, status: status)
        }
        if let error = status.error { lines.append(SyncText.value(SyncStatusPresentation.errorKey(error.error))) }
        lines += media(status.media)
        return lines
    }
    static func sum(_ first: UInt64?, _ second: UInt64?) -> UInt64? {
        guard let first, let second else { return nil }
        let result = first.addingReportingOverflow(second)
        return result.overflow ? nil : result.partialValue
    }
    static func media(_ facts: CloudPolicyMediaFacts) -> [String] {
        var lines: [String] = []
        let running = (facts.uploadsRunning ?? 0) > 0 || (facts.downloadsRunning ?? 0) > 0
        if let files = sum(facts.uploadFiles, facts.downloadFiles), files > 0 {
            lines.append(SyncText.format(running ? "sync.media.filesLeft" : "sync.media.waiting", Int64(clamping: files)))
        }
        if running, let bytes = sum(facts.uploadBytes, facts.downloadBytes) {
            lines.append(SyncText.format("sync.media.sizeLeft", ByteCountFormatter.string(fromByteCount: Int64(clamping: bytes), countStyle: .file)))
        }
        // No download estimate or optional count is filled in by the UI.
        if (facts.downloadsRunning ?? 0) > 0, let milliseconds = facts.remainingDownloadMs {
            let formatter = DateComponentsFormatter()
            formatter.unitsStyle = .full; formatter.maximumUnitCount = 1
            if let duration = formatter.string(from: Double(milliseconds) / 1_000) {
                lines.append(SyncText.format("sync.media.timeLeft", duration))
            }
        }
        if facts.noSpace == true { lines.append(SyncText.value("sync.media.noSpace")) }
        return lines
    }
    static func resultTitle(_ outcome: CloudPolicyExchangeOutcome) -> String {
        let key: String = switch outcome {
        case .succeeded: "sync.result.succeeded"
        case .incomplete: "sync.result.incomplete"
        case .waiting: "sync.result.waiting"
        case .failed: "sync.result.failed"
        case .cancelled: "sync.result.cancelled"
        }
        return SyncText.value(key)
    }
    private static func resultLines(_ outcome: CloudPolicyExchangeOutcome, status: CloudPolicyStatus) -> [String] {
        switch outcome {
        case .succeeded(_, let held, let newer, let incoming, let facts):
            var lines: [String] = []
            if newer > 0 { lines.append(SyncText.format("sync.result.newerPending", Int64(clamping: newer))) }
            if held > 0 { lines.append(SyncText.value("sync.details.heldBack")) }
            if incoming > 0 { lines.append(SyncText.format("sync.result.incomingWaiting", Int64(clamping: incoming))) }
            lines += media(facts)
            return lines
        case .incomplete(let outstanding, let blocked):
            var lines: [String] = []
            if outstanding > 0 { lines.append(SyncText.format("sync.result.outstanding", Int64(clamping: outstanding))) }
            if blocked > 0 { lines.append(SyncText.format("sync.result.blocked", Int64(clamping: blocked))) }
            return lines
        case .waiting(let error, _): return [SyncText.value(error.map(SyncStatusPresentation.errorKey) ?? "sync.result.notReady")]
        case .failed(let error): return [SyncText.value(SyncStatusPresentation.errorKey(error))]
        case .cancelled:
            let savingFailure = [CloudPolicyAttention.collectionCorrupt, .localStorageFull, .trackingError].contains { status.attention[$0] != nil }
            return savingFailure ? [] : [SyncText.value("sync.status.keptLocally")]
        }
    }
}

struct CloudRecoveryContent: View {
    let collection: ReviewCollection
    let recovery: CloudRecoveryStatus
    private var cancellable: Bool { recovery.adoptionCancellable || recovery.publicationCancellable }
    var body: some View {
        Text(SyncText.value(SyncStatusPresentation.recoveryStatusKey(recovery)))
            .accessibilityIdentifier("sync.recovery.status")
        if recovery.switchConflict {
            Text(SyncText.value("sync.attention.publishConflict"))
                .accessibilityIdentifier("sync.recovery.switchConflict")
        }
        if SyncStatusPresentation.showsRecoveryResume(recovery) {
            Button(SyncText.value("sync.action.resumeOp")) { Task { await collection.sync.resumeRecoveryOperation() } }
                .disabled(recovery.switchConflict || recovery.step == .cancelling)
                .accessibilityIdentifier("sync.recovery.resume")
        }
        CloudRecoveryCancelButton(collection: collection, recovery: recovery)
        if !cancellable {
            Text(SyncText.value("sync.recovery.error.notOffered")).foregroundStyle(.secondary)
                .accessibilityIdentifier("sync.recovery.cancelUnavailableReason")
        }
    }
}

struct CloudRecoveryCancelButton: View {
    let collection: ReviewCollection
    let recovery: CloudRecoveryStatus
    var label = SyncText.value("sync.action.cancelOp")
    var identifier = "sync.recovery.cancel"
    var onCancelled: (() -> Void)?
    @State private var confirmingCancellation = false
    @State private var cancellation: CloudRecoveryCancellation?
    @State private var cancelling = false
    private var cancellable: Bool { recovery.adoptionCancellable || recovery.publicationCancellable }
    private var adoption: Bool {
        switch cancellation?.recovery.step ?? recovery.step {
        case .staging, .staged, .ready: true
        default: false
        }
    }
    var body: some View {
        Button(label) {
            cancelling = true
            Task {
                cancellation = await collection.sync.prepareConnectionCancellation()
                cancelling = false
                confirmingCancellation = cancellation != nil
            }
        }.disabled(!cancellable || cancelling)
            .accessibilityIdentifier(identifier)
            .confirmationDialog(SyncText.value(adoption ? "sync.cancelOp.adopt.title" : "sync.cancelOp.publish.title"),
                                isPresented: $confirmingCancellation, titleVisibility: .visible) {
                Button(SyncText.value("sync.cancelOp.confirm"), role: .destructive) {
                    guard let cancellation else { return }
                    cancelling = true
                    Task {
                        await collection.sync.cancelConnectionOperation(confirmation: cancellation)
                        cancelling = false
                        if collection.sync.connectionFailure == nil, collection.sync.recovery == nil { onCancelled?() }
                    }
                }.disabled(!cancellable || cancelling)
                Button(SyncText.value("sync.cancelOp.keep"), role: .cancel) { }
            } message: {
                if let cancellation {
                    Text(SyncText.value(SyncStatusPresentation.cancellationMessageKey(cancellation.recovery)))
                }
            }
    }
}
