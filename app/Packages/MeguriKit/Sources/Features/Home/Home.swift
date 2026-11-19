import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#endif

public struct HomeView: View {
    public let collection: ReviewCollection
    public let deckID: Int64?
    #if os(macOS)
    @Environment(\.openSettings) private var openSettings
    @State private var isVisible = false
    @FocusState private var homeFocused: Bool
    #else
    @Environment(\.shellLayout) private var shellLayout
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.todayBesideDeckList) private var todayBesideDeckList
    #endif
    @Environment(\.locale) private var locale
    public init(collection: ReviewCollection, deckID: Int64? = nil) { self.collection = collection; self.deckID = deckID }
    public var body: some View {
        Group {
            if showsRootDeckList, showsToday {
                TodayView(collection: collection)
            } else if showsRootDeckList, let query = activeSearch {
                HomeDeckSearchResults(collection: collection, query: query) { id in
                    collection.navigation.select(.deck(DeckID(id)))
                }
            } else if showsRootDeckList {
                HomeDeckTree(nodes: collection.decks, navigation: collection.navigation, collection: collection,
                    previousAnswerCount: showsPreviousSession ? collection.previousRecord?.answers.count : nil, error: collection.error,
                    isExpanded: { collection.studyExpanded(deckID: $0) },
                    isExpansionPending: { collection.isStudyExpansionPending(deckID: $0) },
                    setExpanded: { id, expanded, descendants in
                        collection.setStudyExpanded(deckID: id, expanded: expanded, includingDescendants: descendants)
                    }) { id in
                    collection.navigation.select(.deck(DeckID(id)))
                }
            } else {
                if let deck = deckPageDetail {
                    // Keeps the screen's and the page's identifiers on separate elements.
                    GeometryReader { _ in deckPage(deck) }
                } else {
                    GeometryReader { geometry in
                        scrollContent(availableHeight: geometry.size.height)
                    }
                }
            }
        }
        .task(id: collection.deckPageRefreshRequest) {
            guard let request = collection.deckPageRefreshRequest, request.deckID == deckID else { return }
            await collection.refreshDeckPage(request)
        }
        .navigationTitle(pageTitle)
        #if os(macOS)
        // N14.4: the Mac names the page in the toolbar, with what is waiting under it.
        .navigationSubtitle(pageSubtitle)
        #else
        .modifier(HomeDeckSearch(enabled: showsDeckSearch, search: search))
        // The deck page carries its own title in the content: no empty large-title band above it.
        // The tab layout's deck list shares its bar's row with the title (N14.8).
        .toolbarTitleDisplayMode(deckID != nil ? .inline : shellLayout == .compact ? .inlineLarge : .automatic)
        #endif
        .toolbar {
            #if !os(macOS)
            ToolbarItem(placement: .primaryAction) {
                if showsDeckSearch {
                    // N14.2: the list's Go to Deck; the system field appears when asked for.
                    Button { search.presented = true } label: {
                        Label { Text("Search", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                    }
                    .accessibilityIdentifier("home.collection.search")
                }
            }
            #endif
            ToolbarItem(placement: .primaryAction) {
                #if os(macOS)
                addNoteButton
                #else
                if deckID == nil, shellLayout == .compact { addMenu } else { addNoteButton }
                #endif
            }
            #if os(macOS)
            ToolbarItemGroup(placement: .primaryAction) {
                if collection.review == nil, collection.navigation.review == nil {
                    Button { collection.presentGoToDeck() } label: {
                        Label { Text("Go to Deck…", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                    }
                    .help(Text("Go to Deck…", bundle: .module))
                    .disabled(!collection.canPresentGoToDeck())
                    .accessibilityIdentifier("home.collection.goToDeck")
                }
                Button { collection.deckActions.present(.create) } label: {
                    Label { Text("Create Deck", bundle: .module) } icon: { Image(systemName: "folder.badge.plus") }
                }
                .help(Text("Create Deck", bundle: .module))
                .disabled(!collection.deckActions.canPresent(.create))
                .accessibilityIdentifier("home.collection.createDeck")
            }
            #else
            ToolbarItem(placement: .primaryAction) {
                if collection.review == nil, collection.navigation.review == nil, showsCollectionActions {
                    Button { collection.presentGoToDeck() } label: {
                        Label { Text("Go to Deck…", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                    }
                    .disabled(!collection.canPresentGoToDeck())
                    .accessibilityIdentifier("home.collection.goToDeck")
                }
            }
            ToolbarItem(placement: .primaryAction) {
                if showsCollectionActions {
                    Button { collection.deckActions.present(.create) } label: {
                        Label { Text("Create Deck", bundle: .module) } icon: { Image(systemName: "folder.badge.plus") }
                    }
                    .disabled(!collection.deckActions.canPresent(.create))
                    .accessibilityIdentifier("home.collection.createDeck")
                }
            }
            #endif
        }
        #if os(macOS)
        .focusable(homeKeyboardActive)
        // Home owns unhandled keys without drawing a focus ring around the page.
        .focusEffectDisabled()
        .focused($homeFocused)
        .onAppear { isVisible = true }
        .onDisappear { isVisible = false }
        .onChange(of: homeKeyboardActive, initial: true) { _, active in
            // The deck page can change while the picker still owns focus.
            // Hand it to the visible Home only after the sheet is dismissed.
            homeFocused = active
        }
        #endif
        .onKeyPress(keys: [KeyEquivalent("/")], phases: .down) { press in
            guard press.modifiers.isEmpty, collection.navigation.section == .home,
                  collection.canPresentGoToDeck() else { return .ignored }
            #if os(macOS)
            // Native editing, sheet and menu tracking own their input, even if
            // an unhandled event bubbles to Home's key path.
            guard homeKeyboardActive, NSApp.isActive, let window = NSApp.keyWindow, window.attachedSheet == nil,
                  !(window.firstResponder is NSTextInputClient),
                  RunLoop.current.currentMode != .eventTracking,
                  let event = NSApp.currentEvent, event.type == .keyDown, !event.isARepeat else { return .ignored }
            #endif
            collection.presentGoToDeck()
            return .handled
        }
        .accessibilityIdentifier("home.collection.screen")
    }
    @ViewBuilder private var addNoteButton: some View {
        if deckID.flatMap({ collection.deck($0) })?.filtered != true {
            Button { collection.navigation.present(.add(deck: deckID.map { DeckID($0) })) } label: {
                Label { Text("Add Note", bundle: .module) } icon: { Image(systemName: "plus") }
            }
            .help(Text("Add Note", bundle: .module))
            .disabled(!collection.addNotePresentation.canPresent(presetDeck: deckID))
            .accessibilityIdentifier(deckID == nil ? "home.collection.addNote" : "home.deck.addNote")
        }
    }
    #if !os(macOS)
    /// N14.2: the tab layout's deck list has one add button; Go to Deck is its search field.
    private var addMenu: some View {
        Menu {
            Button { collection.navigation.present(.add(deck: nil)) } label: {
                Label { Text("New Note", bundle: .module) } icon: { Image(systemName: "square.and.pencil") }
            }
            .disabled(!collection.addNotePresentation.canPresent(presetDeck: nil))
            .accessibilityIdentifier("home.collection.addNote")
            Button { collection.deckActions.present(.create) } label: {
                Label { Text("New Deck…", bundle: .module) } icon: { Image(systemName: "folder.badge.plus") }
            }
            .disabled(!collection.deckActions.canPresent(.create))
            .accessibilityIdentifier("home.collection.createDeck")
        } label: {
            Label { Text("Add", bundle: .module) } icon: { Image(systemName: "plus") }
        }
        .accessibilityIdentifier("home.collection.add")
    }
    /// Go to Deck and Create Deck are toolbar buttons only in the sidebar layout (N14.2, N14.3).
    private var showsCollectionActions: Bool { shellLayout == .sidebar }
    private var showsDeckSearch: Bool { deckID == nil && shellLayout == .compact && showsRootDeckList }
    /// Held by the navigation, not the view: a fold, an unfold or a layout switch keeps the query.
    private var search: DeckListSearch {
        collection.navigation.featureState(DeckListSearch.self) { DeckListSearch() }
    }
    private var activeSearch: String? { showsDeckSearch ? search.query : nil }
    /// Today beside the list (N14.1) already shows the previous session.
    private var showsPreviousSession: Bool {
        !(todayBesideDeckList && collection.navigation.homeDeckPage == nil)
    }
    /// N14.1: the sidebar layout's root is Today; the tab layout's is the deck list.
    private var showsToday: Bool { shellLayout == .sidebar }
    #else
    private var activeSearch: String? { nil }
    private var showsToday: Bool { true }
    private var showsPreviousSession: Bool { true }
    #endif
    private var pageTitle: String {
        // The sidebar's first row says Today in every collection state, so its page does too.
        guard let deckID else { return HomeHeading.rootTitle(showsToday: showsToday) }
        #if os(macOS)
        return HomeHeading.deckTitle(deckPageDetail?.name ?? collection.deck(deckID)?.name ?? "")
        #else
        return ""
        #endif
    }
    #if os(macOS)
    private var pageSubtitle: String {
        if let deck = deckPageDetail, let waiting = collection.deckWaiting(deck.id) {
            return HomeHeading.deckSubtitle(waiting, locale: locale)
        }
        guard deckID == nil, showsRootDeckList else { return "" }
        return HomeHeading.todaySubtitle(TodayDecks(nodes: collection.decks).totals, locale: locale)
    }
    #endif
    /// N14.3: a regular deck with nothing waiting says so and offers Add Note in Study's place.
    private func nothingDue(_ deck: ReviewCollection.DeckPageDetail) -> Bool {
        !collection.offersStudy(deckID: deck.id)
    }
    #if os(macOS)
    private var homeKeyboardActive: Bool {
        isVisible && collection.navigation.section == .home &&
            collection.navigation.homeDeckPage?.rawValue == deckID &&
            collection.navigation.path(.home).isEmpty &&
            collection.review == nil && collection.navigation.review == nil &&
            collection.navigation.sheet == nil
    }
    #endif
    private var showsRootDeckList: Bool {
        guard case .ready = collection.state, !collection.decks.isEmpty else { return false }
        return deckID.flatMap { collection.deckPage($0) } == nil
    }
    /// N2: every state view is centred in the pane; only the deck list starts at the top.
    private var centersContent: Bool {
        if case .ready = collection.state { return collection.decks.isEmpty }
        return true
    }
    private func scrollContent(availableHeight: CGFloat) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Spacing.large) {
                if let record = collection.previousRecord, !record.answers.isEmpty {
                    VStack(alignment: .leading, spacing: Spacing.xSmall) {
                        Text("Previous Session", bundle: .module).font(.headline)
                        Text("\(record.answers.count) answers recorded", bundle: .module)
                            .accessibilityIdentifier("home.session.recorded")
                    }
                }
                if let error = collection.error, !hasFailed {
                    // A symbol with the colour, so colour is not the only cue.
                    Label {
                        Text(verbatim: error)
                    } icon: {
                        Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.token(.statusError))
                            .accessibilityHidden(true)
                    }
                    .accessibilityIdentifier("home.collection.error")
                }
                content
            }
            .padding(Spacing.medium)
            .frame(maxWidth: ContentWidth.readable, alignment: .leading)
            .frame(maxWidth: .infinity)
            .frame(minHeight: centersContent ? availableHeight : 0, alignment: centersContent ? .center : .top)
            .scrollTargetLayout()
        }
        .navigationScrollPosition(collection.navigation, .section(.home), idType: Int64.self)
    }
    private var hasFailed: Bool {
        if case .failed = collection.state { return true }
        return false
    }
    @ViewBuilder private var content: some View {
        switch collection.state {
        case .unopened, .loading: LoadingStateView(Text("Opening collection…", bundle: .module))
        case .noFile:
            HomeCollectionEmptyContent(canPresent: collection.entry.canPresent,
                importAction: { collection.entry.present() },
                connectTitleKey: collection.sync.status?.controls.resumeBindingOffered == true
                    ? "sync.action.resume" : "sync.connect.action",
                connectUnavailableReason: collection.cloudPreviewEntryUnavailableReason(.connect),
                canPresentConnect: collection.navigation.sheet == nil || collection.navigation.sheet == .accountSync,
                connectAction: { collection.presentCloudSync(.connect) },
                canPresentAccountSync: collection.navigation.sheet == nil,
                accountSyncAction: {
                    guard collection.navigation.sheet == nil else { return }
                    #if os(macOS)
                    collection.syncPresentation.settingsRequest = UUID()
                    openSettings()
                    #else
                    collection.navigation.present(.accountSync)
                    #endif
                }, canStartEmpty: collection.canStartEmpty,
                startEmptyAction: { Task { await collection.startEmpty() } })
        case .unavailable:
            LoadingStateView(Text("Collection is busy…", bundle: .module))
        case .creationRecovery:
            EmptyStateView(Text("Couldn’t Start Empty", bundle: .module), systemImage: "exclamationmark.triangle",
                description: Text("Quit and reopen Meguri before trying again. Any collection file already created will be kept.", bundle: .module))
        case .failed(let message):
            ErrorStateView(Text("Couldn’t Open Collection", bundle: .module), message: Text(verbatim: message),
                retryIdentifier: "home.collection.retry", retry: { Task { await collection.start() } })
                #if DEBUG
                .debugRetryAccessibilityValue(collection.openFailureDiagnosticValue ?? "")
                #endif
        case .ready:
            if deckPageDetail == nil, collection.decks.isEmpty {
                EmptyStateView(Text("No Decks", bundle: .module), systemImage: "rectangle.stack",
                    description: Text("This collection has no decks to study.", bundle: .module))
            }
        }
    }

    private var deckPageDetail: ReviewCollection.DeckPageDetail? {
        guard case .ready = collection.state else { return nil }
        return deckID.flatMap { collection.deckPage($0) }
    }
    private func children(of id: Int64, in nodes: [CountedDeckNode]) -> [CountedDeckNode] {
        for node in nodes {
            if node.id == id { return node.children }
            let found = children(of: id, in: node.children)
            if !found.isEmpty { return found }
        }
        return []
    }
    /// The album page (Music, Podcasts): the deck, what is due and Study, then its subdecks.
    private func deckPage(_ deck: ReviewCollection.DeckPageDetail) -> some View {
        let path = deck.name.components(separatedBy: "::")
        let subdecks = children(of: deck.id, in: collection.decks)
        let empty = nothingDue(deck)
        return List {
            deckSummary(deck, empty: empty)
            if !subdecks.isEmpty {
                Section {
                    ForEach(subdecks, id: \.id) { node in
                        HomeDeckBranch(node: node, collection: collection, ancestors: path, depth: 0,
                            select: openSubdeck,
                            isExpanded: { collection.studyExpanded(deckID: $0) },
                            isExpansionPending: { collection.isStudyExpansionPending(deckID: $0) },
                            setExpanded: { id, expanded, descendants in
                                collection.setStudyExpanded(deckID: id, expanded: expanded, includingDescendants: descendants)
                            }, frameChanged: { _, _ in })
                    }
                } header: {
                    Text("Subdecks", bundle: .module)
                        .accessibilityIdentifier("home.deck.subdecksHeader")
                }
            }
            if let error = collection.error {
                Text(verbatim: error).foregroundStyle(.token(.statusError))
                    .accessibilityIdentifier("home.collection.error")
            }
            if let record = collection.previousRecord, record.deckID == deck.id, !record.answers.isEmpty {
                Section {
                    Text("\(record.answers.count) answers recorded", bundle: .module)
                        .accessibilityIdentifier("home.session.recorded")
                } header: {
                    Text("Previous Session", bundle: .module)
                        .accessibilityIdentifier("home.deck.previousSessionHeader")
                }
            }
        }
        #if !os(macOS)
        // N14.3: system grouped sections on the grouped background.
        .listStyle(.insetGrouped)
        #endif
    }
    @ViewBuilder private func deckAction(_ deck: ReviewCollection.DeckPageDetail, empty: Bool) -> some View {
        if empty {
            Button { collection.navigation.present(.add(deck: DeckID(deck.id))) } label: {
                Label { Text("Add Note", bundle: .module) } icon: { Image(systemName: "plus") }
            }
            .buttonStyle(.bordered)
            .frame(minHeight: ControlMetrics.minimumTarget)
            .disabled(!collection.addNotePresentation.canPresent(presetDeck: deck.id))
            .accessibilityIdentifier("home.deck.empty.addNote")
        } else {
            Button { Task { await collection.startStudy(deckID: deck.id) } } label: {
                #if os(macOS)
                Label { Text("Study", bundle: .module) } icon: { Image(systemName: "play.fill") }
                #else
                Text("Study", bundle: .module).font(.headline)
                #endif
            }
            .buttonStyle(.borderedProminent)
            .frame(minHeight: ControlMetrics.minimumTarget)
            .contentShape(Rectangle())
            .disabled(!collection.canStudy || collection.navigation.sheet != nil || collection.addNotePresentation.opening)
            .accessibilityIdentifier("home.deck.study")
        }
    }
    #if os(macOS)
    /// The Mac shows the name and counts in the toolbar (N14.4): the content starts with the action.
    private func deckSummary(_ deck: ReviewCollection.DeckPageDetail, empty: Bool) -> some View {
        VStack(alignment: .leading) { deckAction(deck, empty: empty) }
            .listRowSeparator(.hidden)
            // A List takes the screen's identifier, so the page's goes here.
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("home.deck.page.\(deck.id)")
    }
    #else
    /// N14.3: the deck's name over one row with what is waiting and its action.
    private func deckSummary(_ deck: ReviewCollection.DeckPageDetail, empty: Bool) -> some View {
        Section {
            // Side by side; at the accessibility text sizes the action goes under the text.
            // One copy of each element: under ViewThatFits the Dynamic Type audit failed them.
            let stacked = dynamicTypeSize.isAccessibilitySize
            let layout = stacked ? AnyLayout(VStackLayout(alignment: .leading)) : AnyLayout(HStackLayout())
            layout {
                deckWaitingText(deck, empty: empty)
                if !stacked { Spacer() }
                deckAction(deck, empty: empty)
            }
            // A List takes the screen's identifier, so the page's goes here.
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("home.deck.page.\(deck.id)")
        } header: {
            Text(verbatim: HomeHeading.deckTitle(deck.name))
                .font(.largeTitle.bold())
                .foregroundStyle(.token(.textPrimary))
                .textCase(nil)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("home.deck.title")
        }
    }
    private func deckWaitingText(_ deck: ReviewCollection.DeckPageDetail, empty: Bool) -> some View {
        VStack(alignment: .leading) {
            if empty {
                Text("Nothing to study right now", bundle: .module)
                    .accessibilityIdentifier("home.deck.nothingDue")
            } else {
                Text("\(deck.counts.waiting) cards waiting", bundle: .module)
                    .accessibilityIdentifier("home.deck.waiting")
                // One line while it fits; one count per line before any of them wraps.
                ViewThatFits(in: .horizontal) {
                    HStack(spacing: Spacing.xxSmall) { deckCounts(deck, separated: true) }
                    VStack(alignment: .leading) { deckCounts(deck, separated: false) }
                }
                .font(.footnote)
                .foregroundStyle(.token(.textSecondary))
                .accessibilityElement(children: .contain)
                .accessibilityIdentifier("home.deck.counts")
            }
        }
    }
    @ViewBuilder private func deckCounts(_ deck: ReviewCollection.DeckPageDetail, separated: Bool) -> some View {
        countText("\(deck.counts.new) new", label: "New", value: deck.counts.new, id: "new")
        if separated { countSeparator }
        countText("\(deck.counts.learning) learning", label: "Learning", value: deck.counts.learning, id: "learning")
        if separated { countSeparator }
        countText("\(deck.counts.review) to review", label: "To Review", value: deck.counts.review, id: "due")
    }
    #endif
    #if !os(macOS)
    private var countSeparator: some View {
        // A drawn dot: the contrast and text-size audits read a text one as content.
        Circle().frame(width: Spacing.xxSmall, height: Spacing.xxSmall).accessibilityHidden(true)
    }
    #endif
    /// A stack pushes the subdeck, so Back returns to its parent; the sidebar selects it.
    private func openSubdeck(_ id: Int64) {
        #if !os(macOS)
        if shellLayout == .compact {
            let navigation = collection.navigation
            navigation.setPath(navigation.path(.home) + [.deckPage(DeckID(id))], for: .home)
            return
        }
        #endif
        collection.openDeckPage(deckID: id)
    }
    #if !os(macOS)
    private func countText(_ text: LocalizedStringKey, label: LocalizedStringKey, value: UInt32, id: String) -> some View {
        Text(text, bundle: .module)
            .lineLimit(1)
            .accessibilityLabel(Text(label, bundle: .module))
            .accessibilityValue(Text(value, format: .number))
            .accessibilityIdentifier("home.deck.counts.\(id)")
    }
    #endif
}

#if !os(macOS)
/// The deck list's search field (N14.2), only where the list is the root.
private struct HomeDeckSearch: ViewModifier {
    let enabled: Bool
    @Bindable var search: DeckListSearch
    func body(content: Content) -> some View {
        if enabled {
            // Out of the way until the toolbar's search button presents it, so the large title stays.
            content.searchable(text: $search.text, isPresented: $search.presented, placement: .navigationBarDrawer(displayMode: .automatic),
                               prompt: Text("Search or Create a Deck", bundle: .module))
        } else {
            content
        }
    }
}
#endif

/// Go to Deck's matches in place of the tree (N14.2): full names, then the offer to create the typed deck.
private struct HomeDeckSearchResults: View {
    let collection: ReviewCollection
    let query: String
    let select: (Int64) -> Void

    var body: some View {
        let search = DeckSearch(nodes: collection.decks, query: query)
        List {
            ForEach(search.matches) { match in
                Button { select(match.id) } label: {
                    VStack(alignment: .leading) {
                        Text(verbatim: match.name)
                        if !match.ancestors.isEmpty {
                            // As Today: the parents as text under the name.
                            Text(verbatim: match.ancestors.joined(separator: " › "))
                                .font(.caption)
                                .quietText()
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text(verbatim: match.fullName))
                .accessibilityIdentifier("home.deckSearch.deck.\(match.id)")
            }
            if search.offersCreate {
                // The name sheet checks and saves the name, as Create Deck does.
                Button {
                    collection.deckActions.present(.create)
                    collection.deckActions.name = query
                } label: {
                    Label { Text("Create deck named \(query)", bundle: .module) } icon: { Image(systemName: "plus") }
                }
                .disabled(!collection.deckActions.canPresent(.create))
                .accessibilityIdentifier("home.deckSearch.create")
            }
        }
        .accessibilityIdentifier("home.deckSearch.list")
    }
}

struct HomeCollectionEmptyContent: View {
    var canPresent: Bool = true
    var importAction: (() -> Void)? = nil
    var connectTitleKey: String = "sync.connect.action"
    var connectUnavailableReason: String? = nil
    var canPresentConnect: Bool = true
    var connectAction: () -> Void = {}
    var canPresentAccountSync: Bool = true
    var accountSyncAction: (() -> Void)? = nil
    var canStartEmpty: Bool = false
    var startEmptyAction: (() -> Void)? = nil

    var body: some View {
        EmptyStateView(Text("No Collection", bundle: .module), systemImage: "rectangle.stack",
            description: Text("Import, sync or start an empty collection.", bundle: .module)) {
            Button(SyncText.value(connectTitleKey), action: connectAction)
                .disabled(connectUnavailableReason != nil || !canPresentConnect)
                .help(SyncText.value(connectUnavailableReason ?? connectTitleKey))
                .accessibilityIdentifier("home.collection.connect")
            if let connectUnavailableReason {
                Text(SyncText.value(connectUnavailableReason)).foregroundStyle(.secondary)
            }
            if let importAction {
                HomeCollectionImportButton(canPresent: canPresent, action: importAction)
            }
            if let accountSyncAction {
                Button(SyncText.value("sync.settings.section"), action: accountSyncAction)
                    .disabled(!canPresentAccountSync)
                    .accessibilityIdentifier("home.collection.accountSync")
            }
            if let startEmptyAction {
                Button(action: startEmptyAction) { Text("Start Empty", bundle: .module) }
                    .disabled(!canStartEmpty)
                    .accessibilityIdentifier("home.collection.startEmpty")
            }
        }
    }
}

/// Styled by EmptyStateView's state actions, like the Connect button beside it.
struct HomeCollectionImportButton: View {
    let canPresent: Bool
    let action: () -> Void
    var body: some View {
        Button(action: action) {
            Text("Import Collection")
        }
        #if os(macOS)
        // As ErrorStateView's retry: the Mac target audit reads this frame.
        .frame(minWidth: ControlMetrics.minimumTarget, minHeight: ControlMetrics.minimumTarget)
        .contentShape(Rectangle())
        #endif
        .disabled(!canPresent)
        .accessibilityIdentifier("home.collection.import")
    }
}

/// Owns the root's native scroll container; never embed this list in a ScrollView.
public struct HomeDeckTree: View {
    public let nodes: [CountedDeckNode]
    public let select: (Int64) -> Void
    private let navigation: AppNavigation?
    private let collection: ReviewCollection?
    private let previousAnswerCount: Int?
    private let error: String?
    private let isExpanded: ((Int64) -> Bool)?
    private let isExpansionPending: ((Int64) -> Bool)?
    private let setExpanded: ((Int64, Bool, Bool) -> Void)?
    @State private var rowFrames: [Int64: CGRect] = [:]
    @State private var restoring: Int64?
    @State private var tracking = false

    public init(nodes: [CountedDeckNode], navigation: AppNavigation? = nil, collection: ReviewCollection? = nil,
                previousAnswerCount: Int? = nil, error: String? = nil,
                isExpanded: ((Int64) -> Bool)? = nil,
                isExpansionPending: ((Int64) -> Bool)? = nil,
                setExpanded: ((Int64, Bool, Bool) -> Void)? = nil,
                select: @escaping (Int64) -> Void) {
        self.nodes = nodes; self.navigation = navigation; self.collection = collection
        self.previousAnswerCount = previousAnswerCount; self.error = error; self.select = select
        self.isExpanded = isExpanded; self.setExpanded = setExpanded
        self.isExpansionPending = isExpansionPending
    }

    public var body: some View {
        GeometryReader { viewport in
            ScrollViewReader { proxy in
                List {
                    ForEach(nodes, id: \.id) { node in
                        HomeDeckBranch(node: node, collection: collection, ancestors: [], depth: 0, select: select,
                            isExpanded: isExpanded, isExpansionPending: isExpansionPending,
                            setExpanded: setExpanded, frameChanged: recordFrame)
                    }
                    if let error {
                        Section {
                            Text(verbatim: error)
                                .foregroundStyle(.token(.statusError))
                                .accessibilityIdentifier("home.collection.error")
                                .id(InfoAnchor.error)
                        }
                    }
                    if let previousAnswerCount, previousAnswerCount > 0 {
                        Section {
                            Text("\(previousAnswerCount) answers recorded", bundle: .module)
                                .foregroundStyle(.secondary)
                                .accessibilityIdentifier("home.session.recorded")
                        } header: {
                            Text("Previous Session", bundle: .module)
                                .accessibilityIdentifier("home.collection.previousSessionHeader")
                        }
                        .id(InfoAnchor.previousSession)
                    }
                }
                .coordinateSpace(name: "home.deck.list")
                // List does not use the ScrollView target-layout modifier.
                // Track actual row geometry, not lazy row creation, so cached
                // off-screen rows cannot overwrite the saved navigation anchor.
                .onChange(of: rowFrames) { _, frames in
                    let visible = frames.filter { $0.value.maxY > 0 && $0.value.minY < viewport.size.height }
                    if let restoring {
                        // Saved anchors can become hidden by a persisted parent disclosure.
                        guard visible[restoring] != nil || !contains(restoring, in: nodes) else { return }
                        self.restoring = nil
                        tracking = true
                    }
                    guard tracking, let top = visible.min(by: { $0.value.minY < $1.value.minY })?.key else { return }
                    navigation?.setScrollPosition(ScrollPosition(id: top, anchor: .top), for: .section(.home))
                }
                .task {
                    tracking = false
                    await Task.yield()
                    let saved = navigation?.scrollPosition(for: .section(.home))
                    if let id = saved?.viewID as? Int64, contains(id, in: nodes) {
                        restoring = id
                        proxy.scrollTo(id, anchor: .top)
                        if let frame = rowFrames[id], frame.maxY > 0, frame.minY < viewport.size.height {
                            restoring = nil
                            tracking = true
                        }
                    } else {
                        restoring = nil
                        if saved?.edge == .top { scrollToTop(proxy) }
                        tracking = true
                    }
                }
                .onDisappear { tracking = false }
                .onChange(of: navigation?.scrollPosition(for: .section(.home)).edge) { _, edge in
                    if edge == .top {
                        restoring = nil
                        tracking = true
                        scrollToTop(proxy)
                    }
                }
            }
        }
    }

    private enum InfoAnchor: Hashable { case previousSession, error }
    private func recordFrame(_ id: Int64, _ frame: CGRect?) { rowFrames[id] = frame }
    private func contains(_ id: Int64, in nodes: [CountedDeckNode]) -> Bool {
        nodes.contains { node in
            node.id == id || ((isExpanded?(node.id) ?? !node.studyCollapsed) && contains(id, in: node.children))
        }
    }
    private func scrollToTop(_ proxy: ScrollViewProxy) {
        if let first = nodes.first {
            proxy.scrollTo(first.id, anchor: .top)
        } else if error != nil {
            proxy.scrollTo(InfoAnchor.error, anchor: .top)
        } else if let previousAnswerCount, previousAnswerCount > 0 {
            proxy.scrollTo(InfoAnchor.previousSession, anchor: .top)
        }
    }
}

private struct HomeDeckBranch: View {
    let node: CountedDeckNode
    let collection: ReviewCollection?
    let ancestors: [String]
    let depth: Int
    let select: (Int64) -> Void
    let isExpanded: ((Int64) -> Bool)?
    let isExpansionPending: ((Int64) -> Bool)?
    let setExpanded: ((Int64, Bool, Bool) -> Void)?
    let frameChanged: (Int64, CGRect?) -> Void
    #if os(macOS)
    @Environment(\.locale) private var locale
    #else
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    #endif

    private var path: String {
        node.name.contains("::") ? node.name : (ancestors + [node.name]).joined(separator: "::")
    }

    private var expanded: Binding<Bool> {
        Binding(get: { isExpanded?(node.id) ?? !node.studyCollapsed },
                set: { setExpanded?(node.id, $0, studyDisclosureIncludesDescendants()) })
    }

    var body: some View {
        Group {
            #if os(macOS)
            if node.children.isEmpty {
                row
            } else {
                DisclosureGroup(isExpanded: expanded) {
                    ForEach(node.children, id: \.id) { child in
                        childBranch(child)
                    }
                } label: { row }
            }
            #else
            // N14.7: the triangle leads, so every count ends at the row's trailing edge.
            HStack {
                if node.children.isEmpty {
                    // The triangle's own width, unseen: names line up whether or not a deck has subdecks.
                    triangle.hidden()
                } else {
                    // Above the row, so the whole target toggles even where it reaches over the name's start.
                    triangle.overlay { disclosure }.zIndex(1)
                }
                row
            }
            .padding(.leading, CGFloat(depth) * Spacing.medium)
            if expanded.wrappedValue {
                ForEach(node.children, id: \.id) { child in
                    childBranch(child)
                }
            }
            #endif
        }
        .id(node.id)
    }

    private func childBranch(_ child: CountedDeckNode) -> some View {
        AnyView(HomeDeckBranch(node: child, collection: collection, ancestors: path.components(separatedBy: "::"), depth: depth + 1,
            select: select, isExpanded: isExpanded, isExpansionPending: isExpansionPending,
            setExpanded: setExpanded, frameChanged: frameChanged))
    }

    #if !os(macOS)
    /// The system's disclosure indicator at its own size, drawn only.
    private var triangle: some View {
        DisclosureGroup(isExpanded: expanded) { EmptyView() } label: { EmptyView() }
            .fixedSize()
            .allowsHitTesting(false)
            // One element first: hidden alone, the group still exports its own
            // small indicator button, which fails the touch-target audit (R1980).
            .accessibilityRepresentation { EmptyView() }
    }

    /// N14.7: the touch target over the triangle. It reserves no layout: it reaches into the
    /// row's leading inset and a little over the name's start.
    private var disclosure: some View {
        // Only this control owns disclosure semantics; selection and child rows are siblings.
        Button { expanded.wrappedValue.toggle() } label: { Color.clear }
            .buttonStyle(.plain)
            .frame(width: ControlMetrics.minimumTarget, height: ControlMetrics.minimumTarget)
            .contentShape(Rectangle())
            // The name as a label: as text it would count as clipped in this control's frame.
            .accessibilityLabel(Text(verbatim: path))
            .accessibilityValue(disclosureValue)
            .accessibilityIdentifier("home.deck.disclosure.\(node.id)")
    }

    private var disclosureValue: Text {
        if isExpansionPending?(node.id) == true {
            return expanded.wrappedValue
                ? Text("Expanded, saving", bundle: .module) : Text("Collapsed, saving", bundle: .module)
        }
        return expanded.wrappedValue
            ? Text("Expanded", bundle: .module) : Text("Collapsed", bundle: .module)
    }
    #endif

    private var row: some View {
        Button { select(node.id) } label: {
            rowContent
                #if os(macOS)
                .frame(minHeight: ControlMetrics.minimumTarget)
                #else
                // A childless row's button fills its cell; one beside a disclosure doesn't.
                .frame(minHeight: node.children.isEmpty ? nil : ControlMetrics.minimumTarget)
                #endif
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
        }
        // Keep the row action distinct from its native disclosure control.
        .buttonStyle(.plain)
        .accessibilityLabel(Text(verbatim: path))
        .accessibilityValue(Text("\(Text(node.counts.new, format: .number)) new · \(Text(node.counts.learning, format: .number)) learning · \(Text(node.counts.review, format: .number)) due", bundle: .module))
        .accessibilityIdentifier("home.deck.row.\(node.id)")
        #if os(macOS)
        .help(Text(verbatim: path))
        #endif
        .onGeometryChange(for: CGRect.self) { $0.frame(in: .named("home.deck.list")) } action: {
            frameChanged(node.id, $0)
        }
        .onDisappear { frameChanged(node.id, nil) }
        .deckMenu(collection: collection, deck: node)
    }

    @ViewBuilder private var filteredSymbol: some View {
        if node.filtered {
            Image(systemName: "line.3.horizontal.decrease.circle")
                .accessibilityHidden(true)
        }
    }

    #if os(macOS)
    private var rowContent: some View {
        HStack {
            filteredSymbol
            Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                .accessibilityIdentifier("home.deck.name.\(node.id)")
            Spacer()
            if let plainCounts {
                Text(verbatim: plainCounts)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("home.deck.rowCounts.\(node.id)")
            }
        }
    }

    /// N11.5: non-zero counts as trailing secondary text, without colours.
    private var plainCounts: String? {
        let new = node.counts.new.formatted(.number.locale(locale))
        let learning = node.counts.learning.formatted(.number.locale(locale))
        let review = node.counts.review.formatted(.number.locale(locale))
        let parts = [node.counts.new == 0 ? nil : String(localized: "\(new) new", bundle: .module),
                     node.counts.learning == 0 ? nil : String(localized: "\(learning) learning", bundle: .module),
                     node.counts.review == 0 ? nil : String(localized: "\(review) to review", bundle: .module)].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
    #else
    private var rowContent: some View {
        // At the accessibility sizes the count goes under the name, which then keeps the row's width.
        let stacks = dynamicTypeSize.isAccessibilitySize
        return (stacks ? AnyLayout(VStackLayout(alignment: .leading)) : AnyLayout(HStackLayout())) {
            HStack {
                filteredSymbol
                Text(verbatim: node.name.components(separatedBy: "::").last ?? node.name)
                    .accessibilityIdentifier("home.deck.name.\(node.id)")
            }
            if !stacks { Spacer() }
            if node.counts.waiting > 0 {
                Text(node.counts.waiting, format: .number)
                    .foregroundStyle(.token(.textSecondary))
                    .monospacedDigit()
                    // A number never splits across lines.
                    .lineLimit(1)
                    .fixedSize()
                    .accessibilityIdentifier("home.deck.rowCounts.\(node.id)")
            }
        }
    }
    #endif
}

public struct HomeSidebarDecks: View {
    public let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        if collection.decks.isEmpty {
            emptyGuidance
                #if !os(macOS)
                .font(.subheadline)
                .foregroundStyle(.token(.textSecondary))
                .lineLimit(nil)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                #endif
                .accessibilityIdentifier("home.sidebar.guidance")
        } else {
            ForEach(collection.decks, id: \.id) { node in
                HomeSidebarBranch(collection: collection, node: node, ancestors: [])
                    // The list reads the selection tag here: a top-level deck without
                    // subdecks otherwise never showed as selected.
                    .tag(SidebarItem.deck(DeckID(node.id)))
            }
        }
    }

    private var emptyGuidance: Text { Text("No decks", bundle: .module) }
}

private struct HomeSidebarBranch: View {
    let collection: ReviewCollection
    let node: CountedDeckNode
    let ancestors: [String]
    private var name: String { node.name.components(separatedBy: "::").last ?? node.name }
    private var path: String { (ancestors + [name]).joined(separator: "::") }
    private var quiet: Bool {
        DeckWaiting(counts: node.counts, filtered: node.filtered)
            .quietInSidebar(selected: collection.navigation.sidebarSelection == .deck(DeckID(node.id)))
    }
    private var expanded: Binding<Bool> {
        Binding(get: { collection.studyExpanded(deckID: node.id) }, set: {
            collection.setStudyExpanded(deckID: node.id, expanded: $0,
                includingDescendants: studyDisclosureIncludesDescendants())
        })
    }

    var body: some View {
        Group {
            if node.children.isEmpty {
                row
            } else {
                DisclosureGroup(isExpanded: expanded) {
                    ForEach(node.children, id: \.id) { child in
                        // AnyView hides the row's own tag, so the group's would reach it and select it too.
                        AnyView(HomeSidebarBranch(collection: collection, node: child, ancestors: ancestors + [name]))
                            .tag(SidebarItem.deck(DeckID(child.id)))
                    }
                } label: { row }
                .tag(SidebarItem.deck(DeckID(node.id)))
                #if !os(macOS)
                .accessibilityElement(children: .contain)
                .accessibilityLabel(Text(verbatim: path))
                .accessibilityValue(expanded.wrappedValue
                    ? Text("Expanded", bundle: .module) : Text("Collapsed", bundle: .module))
                .accessibilityIdentifier("home.sidebar.disclosure.\(node.id)")
                #endif
            }
        }
        .id(node.id)
    }

    private var row: some View {
        Group {
            #if os(macOS)
            Button { collection.openDeckPage(deckID: node.id) } label: {
                Text(verbatim: name)
                    // N14.6: nothing waiting reads quieter, except on the selection highlight.
                    .foregroundStyle(quiet ? .secondary : .primary)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(Text(verbatim: path))
            .accessibilityLabel(Text(verbatim: path))
            .accessibilityValue(Text("\(Text(node.counts.new, format: .number)) new · \(Text(node.counts.learning, format: .number)) learning · \(Text(node.counts.review, format: .number)) due", bundle: .module))
            .accessibilityIdentifier("home.sidebar.deck.\(node.id)")
            .tag(SidebarItem.deck(DeckID(node.id)))
            #else
            HStack {
                Text(verbatim: name)
                Spacer()
                if node.counts.waiting > 0 {
                    // Primary text: secondary falls short of 4.5:1 on the sidebar.
                    Text(node.counts.waiting, format: .number)
                        .monospacedDigit()
                }
            }
            // N14.6: nothing waiting reads quieter, in the token that keeps 4.5:1 on the sidebar.
            .foregroundStyle(.token(quiet ? .textSecondary : .textPrimary))
            .lineLimit(nil)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityElement(children: .combine)
            .accessibilityLabel(Text(verbatim: path))
            .accessibilityValue(Text("\(Text(node.counts.new, format: .number)) new · \(Text(node.counts.learning, format: .number)) learning · \(Text(node.counts.review, format: .number)) due", bundle: .module))
            #endif
        }
        #if !os(macOS)
        .accessibilityIdentifier("home.sidebar.deck.\(node.id)")
        #endif
        .deckMenu(collection: collection, deck: node)
        .tag(SidebarItem.deck(DeckID(node.id)))
        #if os(macOS)
        // An integer badge, which the system hides at zero (N11.5).
        .badge(Int(node.counts.review))
        #endif
    }
}

private func studyDisclosureIncludesDescendants() -> Bool {
    #if os(macOS)
    NSEvent.modifierFlags.contains(.option)
    #else
    false
    #endif
}

public struct FsrsAdmissionView: View {
    public let collection: ReviewCollection
    public init(collection: ReviewCollection) { self.collection = collection }
    public var body: some View {
        VStack(alignment: .leading, spacing: Spacing.large) {
            Label { Text("Study with FSRS", bundle: .module) } icon: { Image(systemName: "brain") }
                .font(.title2.bold())
            Text(collection.policy == .needsReEnable
                 ? String(localized: "Another device turned FSRS off. Meguri will turn it on again for this collection.", bundle: .module)
                 : String(localized: "Meguri schedules with FSRS. Enabling it computes memory states from your review history without changing due dates or intervals. The change reaches your other devices when you sync.", bundle: .module))
            if let error = collection.error {
                Text(verbatim: error).foregroundStyle(.token(.statusError))
                    .accessibilityIdentifier("home.policy.error")
            }
            if collection.enabling {
                if let progress = collection.progress, let total = progress.total {
                    ProgressView(value: Double(progress.completed), total: Double(max(1, total)))
                    Text("\(progress.completed) of \(total) cards", bundle: .module)
                } else { ProgressView() }
            }
            Button { Task { await collection.enableFsrs() } } label: {
                Text("Enable FSRS", bundle: .module).frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.borderedProminent)
            .disabled(collection.enabling)
            .accessibilityIdentifier("home.policy.enable")
        }
        .padding(Spacing.large)
        .frame(maxWidth: ContentWidth.readable)
        .interactiveDismissDisabled()
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("home.policy.sheet")
    }
}
