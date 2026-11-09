import AppModel
import CoreBridge
import Foundation
import SwiftUI

/// Text is projected from Rust facts, never from native SDK idleness.
struct SyncStatusContent: View {
    let collection: ReviewCollection
    var body: some View {
        Section { SyncStatusRows(collection: collection) }
    }
}

/// Shared native rows can live in Settings' iCloud group or the status popover.
struct SyncStatusRows: View {
    let collection: ReviewCollection
    @Environment(\.appClock) private var clock
    private var sync: SyncCoordinator { collection.sync }
    private var presentation: SyncStatusPresentation { .init(state: sync.status?.state, setup: sync.setupState) }
    var body: some View {
        Group {
            VStack(alignment: .leading) {
                Label(SyncText.value(presentation.titleKey), systemImage: presentation.symbol)
                if let key = presentation.detailKey { Text(SyncText.value(key)) }
                if let status = sync.status {
                    if case .upToDate(let at) = status.state {
                        Text(SyncText.format("sync.status.upToDate.detail", SyncText.instant(at, now: clock.now())))
                    }
                    if status.state == .needsAttention {
                        if status.attention.count == 1, let item = status.attention.first {
                            Text(SyncStatusCopy.attention(item.key, count: item.value))
                        } else if !status.attention.isEmpty {
                            Text(SyncText.format("sync.status.needsAttention.count", status.attention.count))
                        }
                    }
                    if SyncStatusCopy.keepsLocally(status) { Text(SyncText.value("sync.status.keptLocally")) }
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("settings.accountSync.status")
            #if DEBUG
            .accessibilityValue(DebugCloudDiagnostics.latestValue ?? "")
            #endif
            if let status = sync.status {
                ForEach(Array(SyncStatusCopy.lines(status, now: clock.now()).enumerated()), id: \.offset) { _, line in
                    Text(line)
                }
            }
            if let failure = sync.connectionFailure {
                Text(SyncText.value(SyncStatusPresentation.connectionErrorKey(failure)))
            } else if sync.operationFailed { Text(SyncText.value("sync.recovery.error.other")) }
            if let recovery = sync.recovery {
                CloudRecoveryContent(collection: collection, recovery: recovery)
            }
        }
    }
}

struct SyncNowButton: View {
    let collection: ReviewCollection
    var identifier = "sync.status.syncNow"
    var showsUnavailableReason = true
    private var reason: String? { SyncStatusCopy.unavailable(collection) }
    var body: some View {
        Button(SyncText.value("sync.now.command")) { collection.requestSyncNow() }
            .disabled(collection.syncNowAvailability() != .available || collection.navigation.sheet != nil)
            .help(reason ?? SyncText.value("sync.now.command"))
            .accessibilityIdentifier(identifier)
        if showsUnavailableReason, let reason { Text(reason).foregroundStyle(.secondary) }
    }
}

enum SyncStatusCopy {
    static func keepsLocally(_ status: CloudPolicyStatus) -> Bool {
        guard status.attention[.collectionCorrupt] == nil, status.attention[.localStorageFull] == nil,
              status.attention[.trackingError] == nil else { return false }
        switch status.state {
        case .iCloudUnavailable, .paused, .offline, .needsAttention: return true
        default: return false
        }
    }
    static func unavailable(_ collection: ReviewCollection) -> String? {
        guard case .unavailable(let reason) = collection.syncNowAvailability() else { return nil }
        let key: String
        switch reason {
        case .collectionUnavailable: key = "sync.now.unavailable.collectionUnavailable"
        case .reviewActive:
            key = SyncStatusPresentation.reviewAvailabilityKey(collection.sync.status?.controls)
        case .notConnected: key = "sync.now.unavailable.notConnected"
        case .paused: key = "sync.now.unavailable.paused"
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
        lines += media(status.media, paused: status.mediaPaused)
        return lines
    }
    static func sum(_ first: UInt64?, _ second: UInt64?) -> UInt64? {
        guard let first, let second else { return nil }
        let result = first.addingReportingOverflow(second)
        return result.overflow ? nil : result.partialValue
    }
    static func media(_ facts: CloudPolicyMediaFacts, paused: Bool) -> [String] {
        if paused { return [SyncText.value("sync.media.off")] }
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
            lines += media(facts, paused: status.mediaPaused)
            return lines
        case .incomplete(let outstanding, let blocked):
            var lines: [String] = []
            if outstanding > 0 { lines.append(SyncText.format("sync.result.outstanding", Int64(clamping: outstanding))) }
            if blocked > 0 { lines.append(SyncText.format("sync.result.blocked", Int64(clamping: blocked))) }
            return lines
        case .waiting(let error, _), .failed(let error): return [SyncText.value(SyncStatusPresentation.errorKey(error))]
        case .cancelled:
            let savingFailure = [CloudPolicyAttention.collectionCorrupt, .localStorageFull, .trackingError].contains { status.attention[$0] != nil }
            return savingFailure ? [] : [SyncText.value("sync.status.keptLocally")]
        }
    }
    static func attention(_ item: CloudPolicyAttention, count: UInt64) -> String {
        if item == .generationIncomplete || item == .recoveryOpWaiting {
            return SyncText.value("sync.error.conflictRecovery")
        }
        let key: String = switch item {
        case .isolated: "isolated"
        case .rootUnreadable: "rootUnreadable"
        case .chooseCollection: "chooseCollection"
        case .conflicts: "conflicts"
        case .conflictsUnreadable: "conflictsUnreadable"
        case .refused: "refused"
        case .referenceWaiting: "referenceWaiting"
        case .oversize: "oversize"
        case .unreadable: "unreadable"
        case .unsyncable: "unsyncable"
        case .claimStuck: "claimStuck"
        case .generationIncomplete, .recoveryOpWaiting: "cloudError" // required detail is not in this projection
        case .assetUnavailable: "assetUnavailable"
        case .applyFailed: "applyFailed"
        case .rekeyFailed: "rekeyFailed"
        case .crtMismatch: "crtMismatch"
        case .collectionCorrupt: "collectionCorrupt"
        case .mediaZoneMissing: "mediaMissingInICloud"
        case .mediaChanged: "mediaChanged"
        case .mediaMissing: "mediaMissing"
        case .exchangeIncomplete: "exchangeIncomplete"
        case .quotaFull: "quotaFull"
        case .localStorageFull: "localStorageFull"
        case .trackingError: "trackingError"
        case .unsupportedSchema: "unsupportedSchema"
        case .cloudError: "cloudError"
        }
        return SyncText.format("sync.attention." + key, Int64(clamping: count))
    }
    // P14.4's presentation order; a Dictionary's iteration order is not UI order.
    static let attentionOrder: [CloudPolicyAttention] = [.isolated, .rootUnreadable, .chooseCollection,
        .conflicts, .conflictsUnreadable, .refused, .referenceWaiting, .oversize, .unreadable, .unsyncable,
        .claimStuck, .generationIncomplete, .assetUnavailable, .applyFailed, .rekeyFailed, .crtMismatch,
        .collectionCorrupt, .mediaZoneMissing, .mediaChanged, .mediaMissing, .recoveryOpWaiting,
        .exchangeIncomplete, .quotaFull, .localStorageFull, .trackingError, .unsupportedSchema, .cloudError]
}

struct CloudRecoveryContent: View {
    let collection: ReviewCollection
    let recovery: CloudRecoveryStatus
    var body: some View {
        Text(SyncText.value(stepKey))
        if recovery.switchConflict { Text(SyncText.value("sync.attention.publishConflict")) }
        Button(SyncText.value("sync.action.resumeOp")) {
            if recovery.adoptionTransportWait { collection.sync.resumeWaitingAdoption() }
            else { collection.sync.resumeConnection() }
        }
            .disabled(recovery.switchConflict || recovery.step == .cancelling || collection.sync.settings.paused)
            .accessibilityIdentifier("sync.recovery.resume")
        // Only the exact accepted staging wait grants this Cancel offer.
        Button(SyncText.value("sync.action.cancelOp")) {
            if recovery.adoptionTransportWait { Task { await collection.sync.cancelConnectionOperation() } }
        }.disabled(!recovery.adoptionTransportWait)
        if !recovery.adoptionTransportWait {
            Text(SyncText.value("sync.recovery.error.notOffered")).foregroundStyle(.secondary)
        }
    }
    private var stepKey: String {
        switch recovery.step {
        case .confirmed, .bound, .ready: "sync.publish.preparing"
        case .zones: "sync.publish.uploading"
        case .sent, .complete: "sync.publish.finishing"
        case .cancelling: "sync.publish.cancelling"
        case .staging: "sync.adopt.staging"
        case .staged: "sync.adopt.validating"
        }
    }
}
