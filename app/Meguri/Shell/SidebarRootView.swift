import AppModel
import DesignSystem
import SwiftUI

/// The sidebar layout (R4, R17, R20–R22), at regular width and always on the
/// Mac: the sections and the deck tree in the sidebar, the selection's
/// screen in the detail column. Production Mac review replaces the split
/// view entirely; adaptive and fixture shells retain their layout. Every
/// piece of state lives in `AppNavigation`, so the tab layout can take over
/// at any moment (R5).
struct SidebarRootView: View {
    @Bindable var navigation: AppNavigation
    let features: ShellFeatures
    let configuration: LaunchConfiguration

    var body: some View {
        #if os(macOS)
        if features.nativeMacWorkspace, let review = navigation.review {
            features.review(review, navigation)
                .toolbar(removing: .sidebarToggle)
        } else {
            if features.nativeMacWorkspace, case .add = navigation.sheet {
                HSplitView {
                    workspace
                    features.addPanel()
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            } else {
                workspace
            }
        }
        #else
        workspace
        #endif
    }

    private var workspace: some View {
        NavigationSplitView(columnVisibility: $navigation.columnVisibility) {
            SidebarView(navigation: navigation, features: features)
                // The sidebar stays away for the whole review (N11.1), its toggle with it.
                .toolbar(removing: navigation.review == nil ? nil : .sidebarToggle)
                #if os(macOS)
                // The iPad's system width (320 points) already fits a deck tree.
                .navigationSplitViewColumnWidth(
                    min: WindowMetrics.sidebarMinimumWidth, ideal: WindowMetrics.sidebarIdealWidth, max: WindowMetrics.sidebarMaximumWidth
                )
                #endif
        } detail: {
            Group {
                if let review = navigation.review {
                    features.review(review, navigation)
                } else {
                    NavigationStack(path: $navigation.detailPath) {
                        SectionDestinationView(screen: root, navigation: navigation, features: features)
                            #if !os(macOS)
                            // iPadOS draws only a column's own toolbar items: one on the
                            // split view itself never appears (N11.8's status button).
                            // Home's pages put it first in their own trailing group (N23.6);
                            // the other sections get it as their first trailing item.
                            .toolbar {
                                if let syncStatus = features.syncStatus, sectionTakesSyncItem {
                                    ToolbarItem(placement: .primaryAction) { syncStatus() }
                                }
                            }
                            #endif
                            .navigationDestination(for: Destination.self) { destination in
                                SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
                            }
                    }
                    #if !os(macOS)
                    .environment(\.syncStatusItem, navigation.section == .home ? features.syncStatus?() : nil)
                    #endif
                    // Each sidebar row has its own stack: a new selection
                    // starts from its root rather than animating from the
                    // last one's.
                    .id(navigation.sidebarSelection)
                }
            }
            .detailColumnChrome()
        }
        #if os(macOS)
        // N23.6: declared once for every screen of the window (Today, a deck page,
        // Browse, Note Types), in the trailing group. `.primaryAction` is the
        // leading edge on the Mac, which a screen with a principal item (Note
        // Types' Fields | Cards) showed right after the title.
        .toolbar {
            if navigation.review == nil, let syncStatus = features.syncStatus {
                ToolbarItem(placement: .automatic) { syncStatus() }
            }
        }
        #endif
    }

    #if !os(macOS)
    /// iPad: the sections whose own toolbar does not carry the sync button. Home's
    /// pages carry it in their group; Settings holds the Sync section itself.
    private var sectionTakesSyncItem: Bool {
        guard features.sectionShown(navigation.section) else { return false }
        return navigation.section != .home && navigation.section != .settings
    }
    #endif

    /// The deck page Home shows in the Home section, else the section's root.
    private var root: SectionDestinationView.Screen {
        // A section that is not offered now (no collection, N18.3) shows Today.
        guard features.sectionShown(navigation.section) else { return .root(.home) }
        if navigation.section == .home, let deck = navigation.homeDeckPage {
            return .deckPage(deck)
        }
        return .root(navigation.section)
    }
}
