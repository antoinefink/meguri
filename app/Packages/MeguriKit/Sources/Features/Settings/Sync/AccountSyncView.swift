import AppModel
import CoreBridge
import SwiftUI

/// The Sync section alone: the sheet over a review (N15.4) and the Mac pane.
public struct AccountSyncView: View {
    let collection: ReviewCollection
    var host: AppNavigation.SheetHost
    /// False in the Mac Settings pane, whose title bar already says Sync.
    var showsHeading: Bool
    @State private var draft = MeguriSignInDraft()
    public init(collection: ReviewCollection, host: AppNavigation.SheetHost = .main, showsHeading: Bool = true) {
        self.collection = collection; self.host = host; self.showsHeading = showsHeading
    }
    public var body: some View {
        Form { SyncSettingsSections(collection: collection, host: host, draft: draft, showsHeading: showsHeading) }
            .formStyle(.grouped)
            .onDisappear { draft.password = "" }
            .navigationTitle(SyncText.value("sync.settings.sync.section"))
            .accessibilityIdentifier("settings.sync.content")
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
