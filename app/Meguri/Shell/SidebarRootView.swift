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
                            .toolbar {
                                if navigation.section == .home {
                                    ToolbarItem(placement: .primaryAction) { features.syncStatus() }
                                }
                            }
                            #endif
                            .navigationDestination(for: Destination.self) { destination in
                                SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
                            }
                    }
                    // Each sidebar row has its own stack: a new selection
                    // starts from its root rather than animating from the
                    // last one's.
                    .id(navigation.sidebarSelection)
                }
            }
            .detailColumnChrome()
        }
        #if os(macOS)
        .toolbar {
            if navigation.review == nil {
                ToolbarItem(placement: .primaryAction) { features.syncStatus() }
            }
        }
        #endif
    }

    /// The deck page Home shows in the Home section, else the section's root.
    private var root: SectionDestinationView.Screen {
        if navigation.section == .home, let deck = navigation.homeDeckPage {
            return .deckPage(deck)
        }
        return .root(navigation.section)
    }
}
