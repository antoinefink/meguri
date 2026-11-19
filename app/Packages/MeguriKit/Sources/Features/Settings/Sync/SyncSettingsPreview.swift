import AppModel
import SwiftUI

/// Synthetic Sync section examples for routes and previews: the production
/// views over an inert coordinator. No collection, account or server exists.
public enum SyncSettingsPreview {
    public enum Page: Equatable, Sendable {
        case settings, troubleshooting, issues, keepCollection, statusPopover
        /// Settings as the tab layout shows it; held wide, the sections are a list beside the section (N15.1).
        case settingsInTabLayout
    }

    @MainActor public static func screen(_ state: SyncCoordinator.RenderState, page: Page = .settings,
                                         configuration: LaunchConfiguration = .init()) -> some View {
        SyncSettingsPreviewScreen(collection: ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: .renderFixture(state, clock: configuration.clock)), page: page)
    }
}

private struct SyncSettingsPreviewScreen: View {
    @State var collection: ReviewCollection
    let page: SyncSettingsPreview.Page
    var body: some View {
        switch page {
        case .settings:
            NavigationStack { SettingsView(collection: collection) }
        case .settingsInTabLayout:
            NavigationStack { SettingsView(collection: collection) }
                .environment(\.shellLayout, .compact)
        case .troubleshooting:
            NavigationStack { SyncTroubleshootingPage(collection: collection) }
        case .issues:
            NavigationStack { SyncIssuesPage(collection: collection) }
        case .keepCollection:
            // Presented as the sheet it is, over the Settings page.
            NavigationStack { SettingsView(collection: collection) }
                .sheet(isPresented: .constant(true)) { MeguriKeepCollectionSheet(collection: collection) {} }
        case .statusPopover:
            NavigationStack {
                Form {}
                    .formStyle(.grouped)
                    .toolbar { ToolbarItem(placement: .primaryAction) { CloudSyncStatusButton(collection: collection) } }
                    .task { collection.syncPresentation.statusPresented = true }
            }
        }
    }
}

#Preview("Meguri Sync connected") { SyncSettingsPreview.screen(.meguriConnected) }
#Preview("iCloud needs you") { SyncSettingsPreview.screen(.iCloudNeedsYou) }
#Preview("Troubleshooting") { SyncSettingsPreview.screen(.iCloudIssues, page: .troubleshooting) }
