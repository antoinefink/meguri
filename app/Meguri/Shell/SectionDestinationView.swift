import AppModel
import DesignSystem
import Editor
import Home
import Review
import Settings
import SwiftUI

/// The review with the sync status button only while sync waits on the
/// person, or while its popover is still open (N16.2).
private struct ReviewDestination: View {
    let model: ReviewCoordinator
    let collection: ReviewCollection
    var body: some View {
        let showsStatus = collection.sync.needsUser || collection.syncPresentation.statusOwnsInput
        ReviewView(model: model, status: showsStatus ? AnyView(CloudSyncStatusButton(collection: collection)) : nil,
            editNote: ReviewEditNote(flow: collection.editNoteFlow, open: { Task { await collection.openEditNote() } },
                form: AnyView(EditNoteView(collection: collection))),
            visibilityChanged: collection.setReviewVisible) { Task { await collection.finishReview() } }
    }
}

/// What the shell hosts in each of its slots (KTD10): section roots, the
/// deck page, the sidebar's deck rows, the review, sheets, and the deck
/// lookup deep links ask (R320). `standard` is the app's; the units that
/// build each feature put it in place of its placeholder (Home U152, deck
/// page U155, review U157/U175, Browse U210, Stats U225, Settings U160,
/// Note Types U200, Add U195, the collection's decks U151/U152). Snapshot
/// fixtures and test-hook routes pass their own.
struct ShellFeatures {
    var nativeMacWorkspace = false
    /// N14.1: the sidebar's first row and the detail with no deck selected are Today.
    var todayRoot = false
    /// What an unfolded iPhone shows beside the deck list before a deck is picked (N14.1).
    var today: (AppNavigation) -> AnyView = { _ in
        AnyView(ContentUnavailableView("Select a Deck", systemImage: "rectangle.stack"))
    }
    var availableSections: [AppSection] = AppSection.sidebar
    var syncStatus: () -> AnyView = { AnyView(EmptyView()) }
    var settingsPage: (SettingsPage) -> AnyView = { _ in AnyView(EmptyView()) }
    var addPanel: () -> AnyView = { AnyView(EmptyView()) }
    /// N17.2: Import opens the file picker on the presenting window, before any sheet.
    var collectionEntry: CollectionEntryCoordinator?
    var sectionRoot: (AppSection, AppNavigation) -> AnyView
    var deckPage: (DeckID, AppNavigation) -> AnyView
    /// The rows of the sidebar's "Decks" group, each tagged
    /// `SidebarItem.deck(id)` so that selecting it opens the deck's page.
    var sidebarDecks: (AppNavigation) -> AnyView
    var review: (ReviewPresentation, AppNavigation) -> AnyView
    var sheet: (SheetRoute, AppNavigation) -> AnyView
    var deckExists: (DeckID) async -> Bool

    /// Real features share the app-owned collection; unbound standard remains
    /// available to isolated shell fixtures that replace every review slot.
    static func standard(collection: ReviewCollection) -> ShellFeatures {
        collection.configureAddNotePresentation()
        var features = standard
        features.addPanel = { AnyView(AddNoteView(collection: collection)) }
        features.collectionEntry = collection.entry
        features.todayRoot = true
        features.today = { _ in AnyView(TodayView(collection: collection, besideDeckList: true)) }
        #if os(macOS)
        features.nativeMacWorkspace = true
        features.availableSections = [.home]
        #else
        features.availableSections = [.home, .settings]
        #endif
        features.sectionRoot = { section, _ in
            if section == .settings { AnyView(SettingsView(collection: collection)) }
            else { AnyView(HomeView(collection: collection)) }
        }
        features.syncStatus = { AnyView(CloudSyncStatusButton(collection: collection)) }
        features.settingsPage = { AnyView(SettingsPageView(collection: collection, page: $0)) }
        features.deckPage = { deck, _ in AnyView(HomeView(collection: collection, deckID: deck.rawValue)) }
        features.sidebarDecks = { _ in AnyView(HomeSidebarDecks(collection: collection)) }
        features.review = { presentation, _ in
            if let model = presentation.session as? ReviewCoordinator {
                AnyView(ReviewDestination(model: model, collection: collection))
            } else { AnyView(EmptyView()) }
        }
        features.sheet = { route, navigation in
            switch route {
            case .goToDeck: AnyView(GoToDeckView(collection: collection))
            case .collectionEntry: AnyView(CollectionEntryView(coordinator: collection.entry))
            case .deckName: AnyView(DeckNameSheet(model: collection.deckActions))
            case .collectionExport: AnyView(CollectionExportDestination(coordinator: collection.collectionExport))
            case .manualTransfer:
                AnyView(ManualTransferWorkflowView(model: collection.manualTransfers, close: navigation.dismissSheet)
                    #if os(macOS)
                    .frame(idealWidth: ContentWidth.readable, minHeight: WindowMetrics.minimum.height)
                    #endif
                )
            case .cloudSync(let entry): AnyView(CloudConnectionView(collection: collection, entry: entry))
            case .accountSync: AnyView(AccountSyncSheet(collection: collection))
            case .add: AnyView(AddNoteView(collection: collection))
            }
        }
        features.deckExists = { collection.deck($0.rawValue) != nil }
        return features
    }

    static var standard: ShellFeatures {
        ShellFeatures(
            sectionRoot: { section, navigation in AnyView(SectionPlaceholder(section: section, navigation: navigation)) },
            deckPage: { deck, _ in AnyView(DeckPagePlaceholder(deck: deck)) },
            sidebarDecks: { _ in AnyView(SidebarDecksPlaceholder()) },
            review: { presentation, navigation in AnyView(ReviewPlaceholder(presentation: presentation, navigation: navigation)) },
            sheet: { sheet, navigation in AnyView(SheetPlaceholder(sheet: sheet, navigation: navigation)) },
            // No collection is open before U151: no deck exists.
            deckExists: { _ in false }
        )
    }
}

/// One screen of a section's stack: a section's root or the deck page, from
/// `ShellFeatures`.
struct SectionDestinationView: View {
    enum Screen: Hashable {
        case root(AppSection)
        case deckPage(DeckID)
        case settingsPage(SettingsPage)

        /// The screen a pushed destination shows.
        init(_ destination: Destination) {
            switch destination {
            case let .deckPage(deck): self = .deckPage(deck)
            case .noteTypes: self = .root(.noteTypes)
            case .settingsPage(let page): self = .settingsPage(page)
            }
        }
    }

    let screen: Screen
    let navigation: AppNavigation
    let features: ShellFeatures

    var body: some View {
        switch screen {
        case let .root(section): features.sectionRoot(section, navigation)
        case let .deckPage(deck): features.deckPage(deck, navigation)
        case let .settingsPage(page): features.settingsPage(page)
        }
    }
}

// MARK: Names and symbols

extension AppSection {
    var title: LocalizedStringResource {
        switch self {
        case .home: "Home"
        case .browse: "Browse"
        case .stats: "Stats"
        case .noteTypes: "Note Types"
        case .settings: "Settings"
        }
    }

    var systemImage: String {
        switch self {
        case .home: "house"
        case .browse: "magnifyingglass"
        case .stats: "chart.bar.xaxis"
        case .noteTypes: "list.bullet.rectangle"
        case .settings: "gearshape"
        }
    }
}

extension CompactTab {
    var title: LocalizedStringResource {
        switch self {
        case .decks: "Decks"
        case .browse: "Browse"
        case .stats: "Stats"
        case .settings: "Settings"
        }
    }

    var systemImage: String {
        switch self {
        case .decks: "rectangle.stack"
        case .browse: AppSection.browse.systemImage
        case .stats: AppSection.stats.systemImage
        case .settings: AppSection.settings.systemImage
        }
    }
}

// MARK: Placeholders until each feature lands

/// A section that is not built yet.
struct SectionPlaceholder: View {
    let section: AppSection
    let navigation: AppNavigation

    var body: some View {
        EmptyStateView(
            Text(section.title), systemImage: section.systemImage,
            description: section == .browse && !navigation.browseSearch.isEmpty
                ? Text("Search: \(navigation.browseSearch)") : Text("Not built yet.")
        )
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("shell.placeholder.\(section.rawValue)")
        .navigationTitle(Text(section.title))
    }
}

struct DeckPagePlaceholder: View {
    let deck: DeckID

    var body: some View {
        EmptyStateView(Text("Deck"), systemImage: "rectangle.stack", description: Text("Deck \(String(deck.rawValue))"))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("shell.placeholder.deckPage")
            .navigationTitle(Text("Deck"))
    }
}

/// The "Decks" group's content until the collection's deck tree (U152).
struct SidebarDecksPlaceholder: View {
    var body: some View {
        Text("No decks")
            .foregroundStyle(.token(.textSecondary))
            .accessibilityIdentifier("shell.root.sidebarDecksEmpty")
    }
}

struct ReviewPlaceholder: View {
    let presentation: ReviewPresentation
    let navigation: AppNavigation

    var body: some View {
        EmptyStateView(Text("Review"), systemImage: "rectangle.on.rectangle", description: Text("Not built yet.")) {
            Button("Close") { navigation.endReview() }
                .accessibilityIdentifier("shell.placeholder.reviewClose")
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("shell.placeholder.review")
    }
}

struct SheetPlaceholder: View {
    let sheet: SheetRoute
    let navigation: AppNavigation

    var body: some View {
        NavigationStack {
            EmptyStateView(Text("Add"), systemImage: "plus", description: Text("Not built yet."))
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("shell.placeholder.add")
                .toolbar {
                    ToolbarItem(placement: .cancellationAction) {
                        Button("Cancel") { navigation.dismissSheet() }
                            .accessibilityIdentifier("shell.sheet.cancel")
                    }
                }
        }
    }
}
