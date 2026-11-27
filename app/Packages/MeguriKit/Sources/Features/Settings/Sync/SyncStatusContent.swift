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
    /// T19.7 (resume fix): Mac only, a help tag says why Sync Now waits while
    /// setup has not finished; touch shows no reason line for it.
    private var setupHelp: String? {
        #if os(macOS)
        guard meguri, collection.sync.meguriStage != nil,
              collection.sync.meguriStatus.route == .account("sync.meguri.setup.continue") else { return nil }
        return SyncText.value("sync.meguri.now.unavailable.setup")
        #else
        nil
        #endif
    }
    private var available: Bool { Self.available(collection) }
    /// Whether Sync Now can run, the sheet and a moving status aside.
    @MainActor static func available(_ collection: ReviewCollection) -> Bool {
        // Meguri Sync runs a round only once its setup has completed (MP5).
        if collection.sync.providerState?.selection == .meguri { return collection.sync.meguriStatus.route == .round }
        return collection.syncNowAvailability() == .available
    }
    var body: some View {
        Button(SyncText.value("sync.now.command")) { collection.requestSyncNow() }
            .disabled(!SyncPanel.syncNowEnabled(available: available && collection.navigation.sheet == nil,
                moving: disabledWhileMoving && collection.sync.panelProgress != nil))
            // N23.3: in the panel only a reason makes a help tag.
            .help(reason ?? setupHelp ?? (disabledWhileMoving ? "" : SyncText.value("sync.now.command")))
            .accessibilityIdentifier(identifier)
            #if DEBUG
            // Observe the actual coordinator; never query a transport from a view.
            .accessibilityValue("meguri-round-success-sequence:\(collection.sync.debugMeguriRoundSuccessSequence)")
            #endif
        if showsUnavailableReason, let reason {
            Text(reason).font(.footnote).foregroundStyle(.secondary)
                .accessibilityIdentifier("\(identifier)Reason")
        }
    }
}

enum SyncStatusCopy {
    /// The issues row and the panel's issues line: an issue beside the state.
    /// Meguri Sync's media-blocked row shows only with the files it lists.
    @MainActor static func showsIssues(_ sync: SyncCoordinator) -> Bool {
        let status = sync.mainStatus
        guard status.hasIssues else { return false }
        guard status.issuesRowKey != nil else { return true }
        return (MeguriAffectedCount.value(sync) ?? 0) > 0
    }
    /// The main status's detail as the Sync section's status row and the panel show it.
    @MainActor static func detail(_ sync: SyncCoordinator, now: Date) -> String? {
        let status = sync.mainStatus
        if let key = status.detailKey {
            // N23.1: a detail that repeats the title is not shown ("Setting Up…").
            guard status.shownDetailKey != nil else { return nil }
            if let count = status.detailCount { return SyncText.format(key, Int64(clamping: count)) }
            return SyncText.value(key)
        }
        guard status.titleKey == "sync.status.upToDate.title" else { return nil }
        return sync.meguriLastExchange.map { SyncText.format("sync.status.upToDate.detail", SyncText.instant($0, now: now)) }
    }

    static func unavailable(_ collection: ReviewCollection) -> String? {
        if collection.sync.providerState?.selection == .meguri {
            if case .disabled(let key) = collection.sync.meguriStatus.route {
                return key.map { SyncText.value($0) }
            }
            return nil
        }
        return SyncText.value(SyncKinds.syncNowOffKey)
    }
}
