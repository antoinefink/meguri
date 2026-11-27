import AppModel
import CoreBridge
import DesignSystem
import SwiftUI

/// Synthetic Sync section examples for routes and previews: the production
/// views over an inert coordinator. No collection, account or server exists.
public enum SyncSettingsPreview {
    public enum Page: Equatable, Sendable {
        case settings, troubleshooting, keepCollection, statusPopover
        /// N23's panel as a person sees it. Mac: drawn at the popover's size under
        /// the toolbar button, since a window capture does not include the
        /// popover's own window. Touch: the real popover (the sheet at compact width).
        case statusPanel
        /// Settings as the tab layout shows it; held wide, the sections are a list beside the section (N15.1).
        case settingsInTabLayout
    }

    @MainActor public static func screen(_ state: SyncCoordinator.RenderState, page: Page = .settings,
                                         serviceSwitch: SyncCoordinator.RenderServiceSwitch? = nil,
                                         configuration: LaunchConfiguration = .init()) -> some View {
        SyncSettingsPreviewScreen(collection: ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: .renderFixture(state, clock: configuration.clock, serviceSwitch: serviceSwitch)), page: page)
    }

    /// The service change confirmation's examples (T21.1): the old service's
    /// state, the new value, and invented facts about unsent work.
    public enum ServiceSwitchExample: String, CaseIterable, Sendable {
        case meguriOffUnknown, meguriOffKnown
        public var state: SyncCoordinator.RenderState {
            .meguriConnected
        }
        public var request: SyncCoordinator.RenderServiceSwitch {
            switch self {
            case .meguriOffUnknown: .init(destination: "localOnly", facts: .init(changes: .unknown))
            // Pending work without a count (Meguri Sync's round gives none) and a known media count.
            case .meguriOffKnown: .init(destination: "localOnly", facts: .init(changes: .some, heldBack: 0, media: 3))
            }
        }
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
        case .keepCollection:
            // Presented as the sheet it is, over the Settings page.
            NavigationStack { SettingsView(collection: collection) }
                .sheet(isPresented: .constant(true)) {
                    MeguriKeepCollectionSheet(collection: collection, facts: Self.keepCollectionFacts(collection))
                }
        case .statusPanel:
            #if os(macOS)
            // As a person sees it: the popover's size, under the toolbar button
            // that opens it, over the window. Only its own window is left out:
            // a Mac window capture does not include a popover's window.
            NavigationStack {
                Color.clear
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .overlay(alignment: .topTrailing) {
                        SyncStatusPanel(collection: collection, showSettings: { _ in })
                            .padding()
                            .frame(width: ContentWidth.primaryAction)
                            .fixedSize(horizontal: false, vertical: true)
                            .floatingChrome(in: Radius.rounded(Radius.large))
                            .accessibilityElement(children: .contain)
                            .accessibilityIdentifier("sync.status.popover")
                            .padding(.trailing, Spacing.small)
                    }
                    .toolbar { ToolbarItem(placement: .primaryAction) { SyncStatusButton(collection: collection) } }
            }
            #else
            // Touch captures include the popover (the sheet at compact width): the real one.
            statusPopover
            #endif
        case .statusPopover:
            statusPopover
        }
    }
    /// The status button in the toolbar, its popover (the sheet at compact width) open.
    private var statusPopover: some View {
        NavigationStack {
            Form {}
                .formStyle(.grouped)
                .toolbar { ToolbarItem(placement: .primaryAction) { SyncStatusButton(collection: collection) } }
                .task { collection.syncPresentation.statusPresented = true }
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
#Preview("Troubleshooting") { SyncSettingsPreview.screen(.meguriRoundFailed, page: .troubleshooting) }
