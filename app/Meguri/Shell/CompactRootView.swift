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
    let hasDecks: Bool
    /// Whether Decks shows the list beside the deck at regular width. N18.3: with no
    /// collection the welcome takes the whole width instead. Nil while the collection is
    /// still opening: the layout last shown stays, and a launch starts on the stack, so
    /// no split shows for a moment before a welcome.
    var besideDetail: Bool? = true
    @State private var lastBesideDetail = false
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var isUpright = false

    var body: some View {
        // The setter also runs when the selected tab is tapped again: that
        // pops its stack, then scrolls its root to the top (R19).
        TabView(selection: Binding(get: { navigation.compactTab }, set: { navigation.selectTab($0) })) {
            // Statistics' tab waits for its compact page (U304), whatever the sidebar shows.
            ForEach(CompactTab.allCases.filter { features.availableSections.contains($0.section) && ($0 != .stats || features.statsTab) },
                    id: \.self) { tab in
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
        .onGeometryChange(for: Bool.self) { $0.size.height > $0.size.width } action: { isUpright = $0 }
        // Statistics has no tab yet (U304): a window that narrows on it shows Decks.
        .onChange(of: navigation.section == .stats && !features.statsTab, initial: true) { _, orphaned in
            if orphaned { navigation.select(.section(.home)) }
        }
        // N23.1: the status panel's sheet opens at its measured height; the panel is
        // measured here, out of sight, not in the toolbar item (N23.6).
        .background { features.syncPanelMeasure?() }
        .onChange(of: besideDetail, initial: true) { _, known in
            if let known { lastBesideDetail = known }
        }
        // N20.3: beside the list the naming form takes the detail half, not a sheet.
        .preference(key: DeckNameBesideListKey.self, value: listBesideDetail)
        .fullScreenCover(item: Binding(get: { navigation.review }, set: { if $0 == nil { navigation.endReview() } })) { review in
            features.review(review, navigation)
                .modifier(LaunchEnvironment(configuration: configuration))
                // Sheets over the review (Add, R90) present from the cover.
                .modifier(PresentationHost(navigation: navigation, features: features, configuration: configuration, isActive: true))
        }
    }

    /// Unfolded and held wide, Decks shows the list beside the deck. Held tall the
    /// system would hide the list behind a toggle, so it keeps its stack (N13).
    /// The list beside the detail shows (N13.1, N14.10).
    private var listBesideDetail: Bool {
        navigation.compactTab == .decks && sizeClass == .regular && !isUpright && (besideDetail ?? lastBesideDetail)
    }

    @ViewBuilder private func stack(_ tab: CompactTab) -> some View {
        if tab == .decks, listBesideDetail {
            NavigationSplitView(columnVisibility: .constant(.doubleColumn)) {
                decksList
                    .navigationSplitViewColumnWidth(min: 280, ideal: 475, max: 475)
                    .toolbar(removing: .sidebarToggle)
            } detail: {
                deckDetail
            }
            .navigationSplitViewStyle(.balanced)
        } else {
            singleStack(tab)
        }
    }

    private var decksList: some View {
        NavigationStack {
            SectionDestinationView(screen: .root(.home), navigation: navigation, features: features)
        }
        // N23.6: the list's own trailing group carries the sync button first (Home).
        .environment(\.syncStatusItem, features.syncStatus?())
        // Today shows beside this list: the list does not repeat it (N14.1).
        .environment(\.todayBesideDeckList, true)
    }

    private var deckDetail: some View {
        NavigationStack(path: Binding(get: { navigation.path(.home) }, set: { navigation.setPath($0, for: .home) })) {
            Group {
                if showsDeckNameForm {
                    features.deckNameBesideList()
                } else if let deck = navigation.homeDeckPage {
                    SectionDestinationView(screen: .deckPage(deck), navigation: navigation, features: features)
                } else if hasDecks {
                    features.today(navigation)
                }
            }
            .navigationDestination(for: Destination.self) { destination in
                SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
            }
        }
        // A new deck starts from its own top, as a new sidebar selection does.
        .id(navigation.homeDeckPage)
        // Choosing another row in the list closes the form as Cancel does.
        .onChange(of: navigation.homeDeckPage) { _, _ in
            if showsDeckNameForm { features.cancelDeckName() }
        }
    }

    private var showsDeckNameForm: Bool {
        guard case .deckName = navigation.sheet, navigation.sheetHost == .main else { return false }
        return true
    }

    private func singleStack(_ tab: CompactTab) -> some View {
        NavigationStack(path: Binding(get: { navigation.compactPath(tab) }, set: { navigation.setCompactPath($0, for: tab) })) {
            SectionDestinationView(screen: .root(tab.section), navigation: navigation, features: features)
                .navigationDestination(for: Destination.self) { destination in
                    SectionDestinationView(screen: .init(destination), navigation: navigation, features: features)
                }
        }
        // N23.6: every screen of the Decks stack (the list, a deck page, a subdeck's)
        // puts the sync button first in its own trailing group with search and Add, so
        // the system keeps it in that group: never a separate item, which it moved
        // to the bottom when the large title scrolled away.
        .environment(\.syncStatusItem, tab == .decks ? features.syncStatus?() : nil)
    }
}

/// Whether the tab layout shows the deck list beside the detail, where the
/// naming form takes the detail half instead of a sheet (N20.3).
nonisolated struct DeckNameBesideListKey: PreferenceKey {
    static let defaultValue = false
    static func reduce(value: inout Bool, nextValue: () -> Bool) { value = value || nextValue() }
}
#endif
