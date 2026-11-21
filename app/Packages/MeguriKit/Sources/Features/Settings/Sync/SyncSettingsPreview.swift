import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Synthetic Sync section examples for routes and previews: the production
/// views over an inert coordinator. No collection, account or server exists.
public enum SyncSettingsPreview {
    public enum Page: Equatable, Sendable {
        case settings, troubleshooting, issues, keepCollection, statusPopover
        /// N23's panel drawn in the window, as the popover shows it: a window
        /// capture on Mac does not include the popover's own window.
        case statusPanel
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
                .sheet(isPresented: .constant(true)) {
                    MeguriKeepCollectionSheet(collection: collection, facts: Self.keepCollectionFacts(collection))
                }
        case .statusPanel:
            NavigationStack {
                SyncStatusPanel(collection: collection, openEntry: { _, _ in }, showSettings: { _ in })
                    .padding()
                    .frame(width: ContentWidth.primaryAction)
                    .accessibilityElement(children: .contain)
                    .accessibilityIdentifier("sync.status.popover")
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                    .toolbar { ToolbarItem(placement: .primaryAction) { CloudSyncStatusButton(collection: collection) } }
            }
        case .statusPopover:
            NavigationStack {
                Form {}
                    .formStyle(.grouped)
                    .toolbar { ToolbarItem(placement: .primaryAction) { CloudSyncStatusButton(collection: collection) } }
                    .task { collection.syncPresentation.statusPresented = true }
            }
        }
    }
    /// Invented counts for both sides (MP6.3 row 4); no core decided them.
    private static func keepCollectionFacts(_ collection: ReviewCollection) -> MeguriSetupFacts {
        let now = Int64(collection.configuration.clock.now().timeIntervalSince1970 * 1_000)
        return .init(row: .ask,
            device: .init(notes: 1_204, cards: 2_318, reviews: 18_452, mediaFiles: 312, lastChangeMs: now - 240_000),
            server: .init(notes: 1_198, cards: 2_306, reviews: 18_120, mediaFiles: nil, lastChangeMs: now - 86_400_000),
            unsent: true)
    }
}

#Preview("Meguri Sync connected") { SyncSettingsPreview.screen(.meguriConnected) }
#Preview("iCloud needs you") { SyncSettingsPreview.screen(.iCloudNeedsYou) }
#Preview("Troubleshooting") { SyncSettingsPreview.screen(.iCloudIssues, page: .troubleshooting) }
