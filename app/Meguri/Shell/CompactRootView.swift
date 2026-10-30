#if os(iOS)
import AppModel
import SwiftUI

/// The tab layout (R3, R18, R19, R23), at compact width: Decks, Browse,
/// Stats and Settings, each with its own `NavigationStack`. A review covers
/// the whole screen, tab bar included, for the whole session. Every piece of
/// state lives in `AppNavigation`, so the sidebar layout can take over at
/// any moment (R5).
struct CompactRootView: View {
    let navigation: AppNavigation
    let features: ShellFeatures
    let configuration: LaunchConfiguration

    var body: some View {
        // The setter also runs when the selected tab is tapped again: that
        // pops its stack, then scrolls its root to the top (R19).
        TabView(selection: Binding(get: { navigation.compactTab }, set: { navigation.selectTab($0) })) {
            ForEach(CompactTab.allCases.filter { features.availableSections.contains($0.section) }, id: \.self) { tab in
                Tab(value: tab) {
                    stack(tab)
                } label: {
                    Label { Text(tab.title) } icon: { Image(systemName: tab.systemImage) }
                }
                // The tab bar button's own (R1964): without it the button
                // takes its symbol's name, which its image has too. SwiftUI
                // sets it a moment after launch.
                .accessibilityIdentifier("shell.root.tab.\(tab.rawValue)")
            }
        }
        .fullScreenCover(item: Binding(get: { navigation.review }, set: { if $0 == nil { navigation.endReview() } })) { review in
            features.review(review, navigation)
                .modifier(LaunchEnvironment(configuration: configuration))
                // Sheets over the review (Add, R90) present from the cover.
                .modifier(PresentationHost(navigation: navigation, features: features, configuration: configuration, isActive: true))
        }
    }

    private func stack(_ tab: CompactTab) -> some View {
        NavigationStack(path: Binding(get: { navigation.compactPath(tab) }, set: { navigation.setCompactPath($0, for: tab) })) {
            SectionDestinationView(screen: .root(tab.section), navigation: navigation, features: features)
                .toolbar {
                    if tab == .decks { ToolbarItem(placement: .primaryAction) { features.syncStatus() } }
                }
                .navigationDestination(for: Destination.self) { destination in
                    SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
                }
        }
    }
}
#endif
