import AppModel
import CoreBridge
import SwiftUI

/// The Sync section alone: the sheet over a review (N15.4) and the Mac pane.
public struct AccountSyncView: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost
    @State private var draft = MeguriSignInDraft()
    public init(collection: ReviewCollection, host: AppNavigation.SheetHost = .main) {
        self.collection = collection; self.host = host
    }
    public var body: some View {
        Form { SyncSettingsSections(collection: collection, host: host, draft: draft) }
            .formStyle(.grouped)
            .onDisappear { draft.password = "" }
            .navigationTitle(SyncText.value("sync.settings.sync.section"))
            .accessibilityIdentifier("settings.sync.content")
    }
}

struct CloudPreviewEntryButton: View {
    let collection: ReviewCollection
    let entry: CloudSyncEntry
    var host: AppNavigation.SheetHost = .main
    var entryAction: ((CloudSyncEntry, AppNavigation.SheetHost) -> Void)? = nil
    var title: String? = nil
    var identifier: String? = nil
    private var reason: String? {
        collection.cloudPreviewEntryUnavailableReason(entry)
    }
    var body: some View {
        Button(SyncText.value(title ?? titleKey)) {
            if let entryAction { entryAction(entry, host) }
            else { collection.presentCloudSync(entry, host: host) }
        }
            .disabled(reason != nil || (collection.navigation.sheet != nil && collection.navigation.sheet != .accountSync))
            .help(SyncText.value(reason ?? title ?? titleKey))
            .accessibilityIdentifier(identifier ?? "settings.sync.\(entry.rawValue)")
        if let reason { Text(SyncText.value(reason)).foregroundStyle(.secondary) }
    }
    private var titleKey: String {
        switch entry {
        case .connect:
            collection.sync.status?.controls.resumeBindingOffered == true
                ? "sync.action.resume" : "sync.needsYou.action.setUp"
        case .publicationChoice: "sync.decision.action"
        case .replaceLocal: "sync.trouble.replaceDevice"
        case .replaceCloud: "sync.trouble.replaceService.iCloud"
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
                            .accessibilityIdentifier("settings.sync.done")
                    }
                }
        }
    }
}
