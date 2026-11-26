import AppModel
import Browse
import CoreBridge
import DeckOptions
import DesignSystem
import Editor
import Home
import NoteTypes
import Review
import Settings
import Stats
import SwiftUI

/// The review with the sync status button only while sync waits on the
/// person, or while its popover is still open (N16.2).
private struct ReviewDestination: View {
    let model: ReviewCoordinator
    var walk: StudyWalk? = nil
    let collection: ReviewCollection
    var body: some View {
        let showsStatus = collection.sync.needsUser || collection.syncPresentation.statusOwnsInput
        ReviewView(model: model, walk: walk, status: showsStatus ? AnyView(CloudSyncStatusButton(collection: collection)) : nil,
            editNote: ReviewEditNote(flow: collection.editNoteFlow, open: { Task { await collection.openEditNote() } },
                form: AnyView(EditNoteView(collection: collection))),
            showInBrowse: collection.reviewShowInBrowse,
            visibilityChanged: collection.setReviewVisible) { Task { await collection.finishReview() } }
    }
}

/// Today's Study (N24.3): one presentation for the whole walk, showing its open deck.
private struct WalkReviewDestination: View {
    let walk: StudyWalk
    let collection: ReviewCollection
    var body: some View {
        if let model = walk.current {
            ReviewDestination(model: model, walk: walk, collection: collection)
        } else {
            ProgressView()
        }
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
    /// W12: the Mac deletion alert is hosted directly on the route's window.
    var noteTypeDeletionModel: NoteTypesModel?
    /// N14.1: the sidebar's first row and the detail with no deck selected are Today.
    var todayRoot = false
    /// What an unfolded iPhone shows beside the deck list before a deck is picked (N14.1).
    var today: (AppNavigation) -> AnyView = { _ in
        AnyView(ContentUnavailableView("Select a Deck", systemImage: "rectangle.stack"))
    }
    /// The sections this device offers: the sidebar's rows, and the tab layout's tabs (Note
    /// Types inside Settings). Statistics' tab and row both follow it (U304).
    var availableSections: [AppSection] = AppSection.sidebar
    /// The Statistics page where `StatsRollout` shows it: View › Statistics and Go to Section
    /// act on it; nil while it cannot be reached.
    var statsPage: StatsPage?
    /// What keeps the page's wiring to the collection alive for the app's run.
    var statsHost: AnyObject?
    /// Whether a section's row shows now: Browse and Note Types read the
    /// collection, so with none on this device (first run, N18.3) they are
    /// not offered, nor their View commands.
    var sectionShown: (AppSection) -> Bool = { _ in true }
    /// N23.6: the toolbar's sync status button; nil (sample shells) declares no item,
    /// since an item with nothing in it would still get an empty glass.
    var syncStatus: (() -> AnyView)?
    /// iPhone: measures the status panel for its sheet (N23.1), behind the shell's root.
    var syncPanelMeasure: (() -> AnyView)?
    var settingsPage: (SettingsPage) -> AnyView = { _ in AnyView(EmptyView()) }
    /// N20.9: the Deck Options page pushed on a deck page.
    var deckOptions: (DeckID, AppNavigation) -> AnyView = { _, _ in AnyView(EmptyView()) }
    /// N22.7: a note type's page pushed over the Note Types list (iPad).
    var noteType: (Int64, AppNavigation) -> AnyView = { _, _ in AnyView(EmptyView()) }
    var tags: () -> AnyView = { AnyView(EmptyView()) }
    var addPanel: () -> AnyView = { AnyView(EmptyView()) }
    /// N17.2: Import opens the file picker on the presenting window, before any sheet.
    var collectionEntry: CollectionEntryCoordinator?
    /// U162: the file intake whose picker the window shows (File › Import…, Settings).
    var importIntake: ImportIntake?
    var sectionRoot: (AppSection, AppNavigation) -> AnyView
    var deckPage: (DeckID, AppNavigation) -> AnyView
    /// The rows of the sidebar's "Decks" group, each tagged
    /// `SidebarItem.deck(id)` so that selecting it opens the deck's page.
    var sidebarDecks: (AppNavigation) -> AnyView
    /// The sidebar with its deck search around it (N20.1); fixtures keep the bare list.
    var sidebarSearch: (AnyView) -> AnyView = { $0 }
    /// N21.1, `browser/workspace` V6: the whole sidebar while Browse is
    /// selected, around the section rows the shell gives (tagged
    /// `BrowseSidebarTag.section`): the filters in place of the decks. Nil
    /// keeps the decks (fixtures without Browse).
    var browseSidebar: ((AppNavigation, AnyView) -> AnyView)?
    /// H12: Return or S while the sidebar has focus starts the selected row's review: a
    /// deck's, or Today's Study. True when the row takes the key, even to do nothing.
    var sidebarStudy: (SidebarItem) -> Bool = { _ in false }
    /// N20.3: the naming form in the detail beside the deck list (iPhone at
    /// regular width), and what choosing another row does to it: Cancel.
    var deckNameBesideList: () -> AnyView = { AnyView(EmptyView()) }
    var cancelDeckName: () -> Void = {}
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
        features.importIntake = collection.intake
        features.todayRoot = true
        features.today = { _ in AnyView(TodayView(collection: collection, besideDeckList: true)) }
        #if os(macOS)
        features.nativeMacWorkspace = true
        features.noteTypeDeletionModel = collection.noteTypes
        #endif
        // N21.1: Today, then Statistics (N25.1) and Browse on the Mac; Note Types under them
        // (N22.1). The iPad's Browse row waits for its own layout; W14: Note Types has a sidebar
        // row at regular width, and the tab layout reaches it from Settings (U286).
        features.availableSections = AppSection.productSidebar(on: .current, statistics: StatsRollout.enabled)
        // X2.5's Show and the leech popover's Show in Browse (N16.11) open Browse where it is shown.
        collection.deckImportBrowseAvailable = features.availableSections.contains(.browse)
        let statsPage: StatsPage?
        if StatsRollout.enabled {
            let host = StatsCollectionHost(collection: collection)
            statsPage = host.page
            features.statsHost = host
        } else {
            statsPage = nil
        }
        features.statsPage = statsPage
        // Statistics, where it is shown, reads a collection as Browse does.
        features.sectionShown = { section in
            (section != .stats || statsPage != nil) && Self.sectionShown(section, hasCollection: collection.hasCollection)
        }
        features.sectionRoot = { section, navigation in
            switch section {
            case .settings: AnyView(SettingsView(collection: collection))
            case .stats:
                statsPage.map { AnyView(StatisticsView(page: $0, navigation: navigation)) }
                    ?? AnyView(HomeView(collection: collection))
            #if os(macOS)
            case .browse:
                AnyView(BrowseView(model: collection.browse, addNote: {
                    // Browse shows no deck: Add opens on its default deck.
                    navigation.present(.add(deck: nil))
                }, editNoteType: { id in
                    collection.noteTypes.selection = id
                    navigation.select(.section(.noteTypes))
                }, addMenu: AnyView(HomeAddMenu(collection: collection))))
            #endif
            case .noteTypes: AnyView(NoteTypesView(model: collection.noteTypes, navigation: navigation))
            default: AnyView(HomeView(collection: collection))
            }
        }
        #if os(macOS)
        features.browseSidebar = { navigation, header in
            AnyView(BrowseSidebarHost(collection: collection, navigation: navigation, header: header))
        }
        #endif
        features.syncStatus = { AnyView(CloudSyncStatusButton(collection: collection)) }
        #if os(iOS)
        features.syncPanelMeasure = { AnyView(CloudSyncPanelMeasure(collection: collection)) }
        #endif
        features.settingsPage = { AnyView(SettingsPageView(collection: collection, page: $0)) }
        features.deckPage = { deck, _ in AnyView(HomeView(collection: collection, deckID: deck.rawValue)) }
        #if os(iOS)
        features.noteType = { id, _ in AnyView(NoteTypePage(model: collection.noteTypes, id: id)) }
        features.tags = { AnyView(CollectionTagsView(collection: collection)) }
        #endif
        features.deckOptions = { deck, navigation in
            AnyView(DeckOptionsPage(model: collection.deckOptions) { navigation.popDeckOptions(deck) })
        }
        features.sidebarDecks = { _ in AnyView(HomeSidebarDecks(collection: collection)) }
        features.sidebarSearch = { list in AnyView(HomeSidebarDeckSearch(collection: collection) { list }) }
        features.sidebarStudy = { item in
            switch item {
            case .section(.home): DeckListStudy.startAll(collection: collection); return true
            case .deck(let deck): DeckListStudy.start(deckID: deck.rawValue, collection: collection); return true
            case .section: return false
            }
        }
        features.deckNameBesideList = { AnyView(DeckNameForm(model: collection.deckActions, embedded: true)) }
        features.cancelDeckName = { if !collection.deckActions.busy { collection.deckActions.cancel() } }
        features.review = { presentation, _ in
            if let model = presentation.session as? ReviewCoordinator {
                AnyView(ReviewDestination(model: model, collection: collection))
            } else if let walk = presentation.session as? StudyWalk {
                AnyView(WalkReviewDestination(walk: walk, collection: collection))
            } else { AnyView(EmptyView()) }
        }
        features.sheet = { route, navigation in
            switch route {
            case .collectionEntry: AnyView(CollectionEntryView(coordinator: collection.entry))
            case .deckImport: AnyView(DeckImportSheet(model: collection.deckImport))
            case .deckName: AnyView(DeckNameForm(model: collection.deckActions))
            case .newDeckOptionsPreset: AnyView(NewDeckOptionsPresetForm(model: collection.deckOptions))
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
            case .noteTypeSave: AnyView(NoteTypeSaveSheet(model: collection.noteTypes))
            }
        }
        features.deckExists = { collection.deck($0.rawValue) != nil }
        return features
    }

    /// Today and Settings need no collection; every other section reads one.
    static func sectionShown(_ section: AppSection, hasCollection: Bool) -> Bool {
        hasCollection || section == .home || section == .settings
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
        case deckOptions(DeckID)
        case noteType(Int64)
        case tags
        case settingsPage(SettingsPage)

        /// The screen a pushed destination shows.
        init(_ destination: Destination) {
            switch destination {
            case let .deckPage(deck): self = .deckPage(deck)
            case let .deckOptions(deck): self = .deckOptions(deck)
            case .noteTypes: self = .root(.noteTypes)
            case .tags: self = .tags
            case .noteType(let id): self = .noteType(id)
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
        case let .deckOptions(deck): features.deckOptions(deck, navigation)
        case let .noteType(id): features.noteType(id, navigation)
        case .tags: features.tags()
        case let .settingsPage(page): features.settingsPage(page)
        }
    }
}

/// Browse's sidebar over the app's collection: Today's deck tree and its
/// expansion (V6), and its reads once the collection is ready.
private struct BrowseSidebarHost: View {
    let collection: ReviewCollection
    let navigation: AppNavigation
    let header: AnyView

    var body: some View {
        BrowseSidebar(model: collection.browse, decks: collection.decks, deckExpanded: { node in
            Binding(get: { collection.studyExpanded(node) },
                    set: { collection.setStudyExpanded(deckID: node.id, expanded: $0, includingDescendants: false) })
        }, navigate: { navigation.select(.section($0)) }) {
            header
        }
        // Rows and counts are read once the collection can answer (a launch, a replacement).
        .onChange(of: collection.browseReadable) { _, readable in
            if readable { collection.browse.appear() }
        }
        // T14: a search handed to Browse while it shows (a deep link) replaces the query now.
        .onChange(of: navigation.browseSearch) { _, search in
            if !search.isEmpty { collection.browse.appear() }
        }
    }
}

// MARK: Names and symbols

extension AppSection {
    var title: LocalizedStringResource {
        switch self {
        case .home: "Home"
        case .browse: "Browse"
        case .stats: "Statistics"
        case .noteTypes: "Note Types"
        case .settings: "Settings"
        }
    }

    var systemImage: String {
        switch self {
        case .home: "house"
        case .browse: "magnifyingglass"
        case .stats: "chart.bar.xaxis"
        // W14: a system symbol, like Today and Browse.
        case .noteTypes: "rectangle.stack"
        case .settings: "gearshape"
        }
    }
}

extension CompactTab {
    var title: LocalizedStringResource {
        switch self {
        case .decks: "Decks"
        case .browse: "Browse"
        case .stats: "Statistics"
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
