import AppModel
import CoreBridge
import SwiftUI

public struct AccountSyncView: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost
    public init(collection: ReviewCollection, host: AppNavigation.SheetHost = .main) {
        self.collection = collection; self.host = host
    }
    private var sync: SyncCoordinator { collection.sync }
    private var controls: CloudPolicyControls { sync.status?.controls ?? .unknown }
    private var decisionRequired: Bool { controls.decisionRequired == true }
    public var body: some View {
        Form {
            Section {
                SyncStatusRows(collection: collection)
                Text(SyncText.value(accountKey))
                    .accessibilityIdentifier("settings.accountSync.accountState")
                if decisionRequired {
                    Button(SyncText.value("sync.decision.action")) { open(.publicationChoice) }
                        .accessibilityLabel(SyncText.value("sync.decision.action.label"))
                        .accessibilityIdentifier("settings.accountSync.decide")
                }
                switch controls.enable {
                case .resumePause:
                    Button(SyncText.value("sync.settings.resumePause")) { update(paused: false) }
                        .accessibilityIdentifier("settings.accountSync.resume")
                case .pause:
                    Button(SyncText.value("sync.settings.pause")) { update(paused: true) }
                        .accessibilityIdentifier("settings.accountSync.pause")
                case .connect:
                    CloudPreviewEntryButton(collection: collection, entry: .connect, host: host)
                case .decisionRequired: EmptyView()
                case .awaitingResumeOffer:
                    CloudPreviewEntryButton(collection: collection, entry: .connect, host: host)
                case nil where sync.settings.paused:
                    // The persisted device pause is available before a Host.
                    Button(SyncText.value("sync.settings.resumePause")) { update(paused: false) }
                        .accessibilityIdentifier("settings.accountSync.resume")
                case nil:
                    // Before a Host exists, only the installation's actual setup
                    // fact can admit preparation. Never infer binding from P14.
                    if sync.setupState == .prepared, collection.lifecycle == .noCollection {
                        CloudPreviewEntryButton(collection: collection, entry: .connect, host: host)
                    }
                }
                SyncNowButton(collection: collection, identifier: "settings.accountSync.syncNow",
                    showsUnavailableReason: !(sync.status == nil && sync.setupState == .configurationUnavailable))
            } header: {
                Text(SyncText.value("sync.settings.iCloud"))
                    .accessibilityIdentifier("settings.accountSync.iCloudHeading")
            } footer: {
                Text(SyncText.value("sync.settings.privacy"))
                    .accessibilityIdentifier("settings.accountSync.privacyFooter")
            }
            Section {
                Toggle(SyncText.value("sync.settings.automatic"), isOn: Binding(
                    get: { sync.settings.automatic }, set: { update(automatic: $0) }))
                    .disabled(controls.bindingUsable != true)
                    .accessibilityIdentifier("settings.accountSync.automatic")
            } footer: {
                Text(SyncText.value("sync.settings.automatic.footer"))
                    .accessibilityIdentifier("settings.accountSync.automaticFooter")
                if let controlReason {
                    Text(SyncText.value(controlReason))
                        .accessibilityIdentifier("settings.accountSync.automaticUnavailableReason")
                }
            }
            Section {
                Toggle(SyncText.value("sync.settings.media"), isOn: Binding(
                    get: { sync.settings.media }, set: { update(media: $0) }))
                    .disabled(controls.bindingUsable != true)
                    .accessibilityIdentifier("settings.accountSync.media")
            } footer: {
                Text(SyncText.value("sync.settings.media.footer"))
                    .accessibilityIdentifier("settings.accountSync.mediaFooter")
                if let controlReason {
                    Text(SyncText.value(controlReason))
                        .accessibilityIdentifier("settings.accountSync.mediaUnavailableReason")
                }
            }
            CloudAttentionSection(collection: collection, host: host)
            Section {
                LabeledContent {
                    if let count = sync.status?.attention[.conflicts] { Text(count, format: .number) }
                    else { Text(SyncText.value("sync.counts.unknown")) }
                } label: {
                    Button(SyncText.value("sync.settings.conflicts")) {}.disabled(true)
                        .accessibilityIdentifier("settings.accountSync.reviewConflicts")
                }
            } footer: {
                Text(SyncText.value("sync.recovery.error.notOffered"))
                    .accessibilityIdentifier("settings.accountSync.conflictsUnavailableReason")
            }
            if sync.status?.accountStatus == .available {
                Section {
                    CloudPreviewEntryButton(collection: collection, entry: .replaceLocal, host: host)
                    CloudPreviewEntryButton(collection: collection, entry: .replaceCloud, host: host)
                } header: {
                    Text(SyncText.value("sync.settings.advanced"))
                        .accessibilityIdentifier("settings.accountSync.advancedHeading")
                } footer: {
                    if sync.settings.paused {
                        Text(SyncText.value("sync.status.paused.user"))
                            .accessibilityIdentifier("settings.accountSync.advancedPauseReason")
                    }
                }
            }
            if controls.bindingActive == true {
                Section {
                    Button(SyncText.value("sync.settings.stop"), role: .destructive) {}.disabled(true)
                        .accessibilityIdentifier("settings.accountSync.stop")
                } footer: {
                    Text(SyncText.value("sync.recovery.error.notOffered"))
                        .accessibilityIdentifier("settings.accountSync.stopUnavailableReason")
                }
            }
            Section {
                DisclosureGroup {
                    // Available metadata and typed failures live directly here.
                    // No raw SDK diagnostics, account identity or connection values.
                    if let status = sync.status {
                        Text(SyncText.value(accountKey))
                            .accessibilityIdentifier("settings.accountSync.detailsAccountState")
                        if let error = status.error {
                            Text(SyncText.value(SyncStatusPresentation.errorKey(error.error)))
                                .accessibilityIdentifier("settings.accountSync.detailsError")
                        }
                        if let last = status.lastAttempt {
                            Text(SyncStatusCopy.resultTitle(last.outcome))
                                .accessibilityIdentifier("settings.accountSync.detailsLastAttempt")
                        }
                    } else {
                        Text(SyncText.value(accountKey))
                            .accessibilityIdentifier("settings.accountSync.detailsAccountState")
                    }
                } label: { Text("Connection details", bundle: .module) }
                .accessibilityIdentifier("settings.accountSync.details")
            }
        }
        .formStyle(.grouped)
        .navigationTitle(SyncText.value("sync.settings.section"))
        .accessibilityIdentifier("settings.accountSync.content")
    }
    private var controlReason: String? {
        controls.bindingUsable == false ? "sync.now.unavailable.notConnected" : nil
    }
    private var accountKey: String {
        guard let status = sync.status else {
            return "sync.settings.account.unknown"
        }
        switch status.accountStatus {
        case .available:
            return !status.accountMatchesBinding && status.attention[.isolated] != nil
                ? "sync.settings.account.other" : "sync.settings.account.available"
        case .noAccount: return "sync.settings.account.none"
        case .restricted: return "sync.settings.account.restricted"
        case .temporary: return "sync.settings.account.temporary"
        case .configuration: return "sync.settings.account.setupNeeded"
        case .undetermined: return "sync.settings.account.unknown"
        }
    }
    private func open(_ entry: CloudSyncEntry) { collection.presentCloudSync(entry, host: host) }
    private func update(paused: Bool? = nil, automatic: Bool? = nil, media: Bool? = nil) {
        let settings = sync.settings
        guard paused != nil || automatic != nil || media != nil else { return }
        sync.setDeviceSettings(paused: paused ?? settings.paused, automatic: automatic ?? settings.automatic, media: media ?? settings.media)
    }
}

struct CloudPreviewEntryButton: View {
    let collection: ReviewCollection
    let entry: CloudSyncEntry
    var host: AppNavigation.SheetHost = .main
    private var reason: String? {
        collection.cloudPreviewEntryUnavailableReason(entry)
    }
    var body: some View {
        Button(SyncText.value(titleKey)) { collection.presentCloudSync(entry, host: host) }
            .disabled(reason != nil || (collection.navigation.sheet != nil && collection.navigation.sheet != .accountSync))
            .help(SyncText.value(reason ?? titleKey))
            .accessibilityIdentifier(entry == .connect && collection.sync.status?.controls.resumeBindingOffered == true ? "settings.accountSync.resume"
                : entry == .connect ? "settings.accountSync.connect" : "settings.accountSync.\(entry.rawValue)")
        if let reason { Text(SyncText.value(reason)).foregroundStyle(.secondary) }
    }
    private var titleKey: String {
        switch entry {
        case .connect:
            collection.sync.status?.controls.resumeBindingOffered == true
                ? "sync.action.resume" : "sync.connect.action"
        case .publicationChoice: "sync.decision.action"
        case .replaceLocal: "sync.adopt.action"
        case .replaceCloud: "sync.publish.action"
        }
    }
}

struct CloudAttentionSection: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost = .main
    var body: some View {
        if let items = collection.sync.status?.attention, !items.isEmpty {
            Section {
                ForEach(Array(SyncStatusCopy.attentionOrder.enumerated()).filter { items[$0.element] != nil }, id: \.element) { offset, item in
                    VStack(alignment: .leading) {
                        Text(SyncStatusCopy.attention(item, count: items[item] ?? 0))
                            .accessibilityIdentifier("settings.accountSync.attentionMessage.\(offset)")
                        if item == .chooseCollection || item == .isolated {
                            CloudPreviewEntryButton(collection: collection, entry: .connect, host: host)
                        }
                        if item == .rootUnreadable || item == .crtMismatch || item == .mediaZoneMissing {
                            CloudPreviewEntryButton(collection: collection, entry: .replaceCloud, host: host)
                        }
                        if item == .crtMismatch {
                            CloudPreviewEntryButton(collection: collection, entry: .replaceLocal, host: host)
                        }
                        if [.quotaFull, .localStorageFull, .trackingError, .cloudError, .exchangeIncomplete].contains(item) {
                            SyncNowButton(collection: collection, reasonIdentifierSuffix: ".\(offset)")
                        }
                        if item == .quotaFull {
                            Text(SyncText.value("sync.attention.storageGuidance"))
                                .accessibilityIdentifier("settings.accountSync.attentionStorageGuidance.\(offset)")
                        }
                        if [.refused, .referenceWaiting, .oversize, .unreadable, .unsyncable, .claimStuck,
                            .generationIncomplete, .applyFailed, .rekeyFailed, .mediaMissing].contains(item) {
                            Button(SyncText.value("sync.action.showDetails")) {}.disabled(true)
                                .accessibilityIdentifier("settings.accountSync.attentionShowDetails.\(offset)")
                            Text(SyncText.value("sync.recovery.error.notOffered")).foregroundStyle(.secondary)
                                .accessibilityIdentifier("settings.accountSync.attentionDetailsUnavailableReason.\(offset)")
                        }
                        if item == .conflicts {
                            Button(SyncText.value("sync.action.reviewConflicts")) {}.disabled(true)
                                .accessibilityIdentifier("settings.accountSync.attentionReviewConflicts.\(offset)")
                            Text(SyncText.value("sync.recovery.error.notOffered")).foregroundStyle(.secondary)
                                .accessibilityIdentifier("settings.accountSync.attentionConflictsUnavailableReason.\(offset)")
                        }
                    }
                    .accessibilityElement(children: .contain)
                }
            } header: {
                Text(SyncText.value("sync.status.needsAttention.title"))
                    .accessibilityIdentifier("settings.accountSync.attentionHeading")
            }
        }
    }
}

/// The mobile sheet owns keyboard input and leaves the review/navigation intact.
public struct AccountSyncSheet: View {
    let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        NavigationStack {
            AccountSyncView(collection: collection)
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button { collection.navigation.dismissSheet() } label: { Text("Done", bundle: .module) }
                    }
                }
        }
    }
}
