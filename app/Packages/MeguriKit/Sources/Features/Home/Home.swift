import AppModel
import CoreBridge
import DesignSystem
import SwiftUI
#if os(macOS)
import AppKit
#else
import UIKit
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
                HomeDeckSearchResults(collection: collection, search: DeckSearchFocus.search(collection.navigation), query: query)
            } else if showsRootDeckList {
                HomeDeckTree(nodes: collection.decks, navigation: collection.navigation, collection: collection,
                    previousAnswerCount: showsPreviousSession ? collection.previousRecord?.answers.count : nil, error: collection.error,
                    isExpanded: { collection.studyExpanded(deckID: $0) },
                    isExpansionPending: { collection.isStudyExpansionPending(deckID: $0) },
                    setExpanded: { id, expanded, descendants in
                        collection.setStudyExpanded(deckID: id, expanded: expanded, includingDescendants: descendants)
                    }, today: todayRow,
                    // N14.11: beside Today the list leaves the collection's activity to Today.
                    activity: todayRow == nil ? collection.activity : nil,
                    activityActive: collection.heatmapRefreshes) { id in
                    collection.navigation.select(.deck(DeckID(id)))
                }
            } else if collection.state == .noFile {
                welcome
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
        .modifier(HomeDeckSearch(enabled: showsDeckSearch, alwaysShown: todayBesideDeckList, collection: collection, search: search))
        // The deck page carries its own title in the content: no empty large-title band above it.
        // The tab layout's deck list shares its bar's row with the title (N14.8).
        .toolbarTitleDisplayMode(deckID != nil ? .inline : shellLayout == .compact ? .inlineLarge : .automatic)
        #endif
        .toolbar {
            #if !os(macOS)
            ToolbarItem(placement: .primaryAction) {
                if showsSearchButton {
                    // N14.2: the list's deck search; the system field appears when asked for.
                    Button { search.presented = true } label: {
                        Label { Text("Search", bundle: .module) } icon: { Image(systemName: "magnifyingglass") }
                    }
                    .accessibilityIdentifier("home.collection.search")
                }
            }
            #endif
            ToolbarItem(placement: .primaryAction) {
                // N20.1: one Add menu beside sync; the tab layout's deck page keeps Add Note (N14.3).
                #if os(macOS)
                addMenu
                #else
                if deckID != nil, shellLayout == .compact { addNoteButton } else { addMenu }
                #endif
            }
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
        // N20.1, H9.2: "/" from the deck page puts the focus in the deck search
        // (⇧⌘O is Find…'s key equivalent, the shell's).
        .onKeyPress(keys: [KeyEquivalent("/")], phases: .down) { press in
            guard press.modifiers.isEmpty, DeckSearchFocus.available(collection, layout: searchLayout) else { return .ignored }
            #if os(macOS)
            // Native editing, sheet and menu tracking own their input, even if
            // an unhandled event bubbles to Home's key path.
            guard homeKeyboardActive, DeckSearchFocus.slashAdmitted() else { return .ignored }
            #endif
            DeckSearchFocus.request(collection, layout: searchLayout)
            return .handled
        }
        #if os(macOS)
        // H12: Return and S on Today's page start Study, the walk of N24.3. The Mac only:
        // Today's page takes keyboard focus there; on the iPad the sidebar's Today row has the keys.
        // Never while a text input has focus (R197).
        .onKeyPress(keys: [.return, KeyEquivalent("s")], phases: .down) { press in
            guard press.modifiers.isEmpty, deckID == nil, showsRootDeckList, showsToday, homeKeyboardActive,
                  !(NSApp.keyWindow?.firstResponder is NSText) else { return .ignored }
            DeckListStudy.startAll(collection: collection)
            return .handled
        }
        #endif
        // H9.2: O on a normal deck's page opens its Deck Options (N20.9).
        .onKeyPress(keys: [KeyEquivalent("o")], phases: .down) { press in
            guard press.modifiers.isEmpty, !press.phase.contains(.repeat), let deckID,
                  collection.deckOptions.canOpen(deckID: deckID) else { return .ignored }
            #if os(macOS)
            guard homeKeyboardActive else { return .ignored }
            #endif
            collection.openDeckOptions(deckID: deckID)
            return .handled
        }
        .accessibilityIdentifier("home.collection.screen")
    }
    #if !os(macOS)
    /// The tab layout's deck page: Add Note only (N14.3).
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
    #endif
    /// N14.2, N20.1: one Add menu. On a deck page New Note presets that deck.
    private var addMenu: some View {
        let preset = deckID.flatMap { collection.deck($0)?.filtered == true ? nil : $0 }
        return Menu {
            Group {
                Button { collection.navigation.present(.add(deck: preset.map { DeckID($0) })) } label: {
                    Label { Text("New Note", bundle: .module) } icon: { Image(systemName: "square.and.pencil") }
                }
                .disabled(!collection.addNotePresentation.canPresent(presetDeck: preset))
                .accessibilityIdentifier(deckID == nil ? "home.collection.addNote" : "home.deck.addNote")
                Button { collection.deckActions.present(.create) } label: {
                    Label { Text("New Deck…", bundle: .module) } icon: { Image(systemName: "folder.badge.plus") }
                }
                .disabled(!collection.deckActions.canPresent(.create))
                .accessibilityIdentifier("home.collection.createDeck")
            }
            // N20.1: each item with its system symbol, on the Mac too.
            .labelStyle(.titleAndIcon)
        } label: {
            Label { Text("Add", bundle: .module) } icon: { Image(systemName: "plus") }
        }
        .help(Text("Add", bundle: .module))
        .accessibilityIdentifier("home.collection.add")
    }
    private var searchLayout: ShellLayout {
        #if os(macOS)
        .sidebar
        #else
        shellLayout
        #endif
    }
    #if !os(macOS)
    private var showsDeckSearch: Bool { deckID == nil && shellLayout == .compact && showsRootDeckList }
    /// N26.4: beside the detail the column shows the search field itself, so no button repeats it.
    private var showsSearchButton: Bool { showsDeckSearch && !todayBesideDeckList }
    /// Held by the navigation, not the view: a fold, an unfold or a layout switch keeps the query.
    private var search: DeckListSearch { DeckSearchFocus.search(collection.navigation) }
    private var activeSearch: String? { showsDeckSearch ? search.query : nil }
    /// N14.10: beside Today on an unfolded iPhone held wide, the list starts with the way back to it.
    private var todayRow: HomeTodayRow? {
        guard todayBesideDeckList else { return nil }
        let navigation = collection.navigation
        return HomeTodayRow(waiting: TodayDecks(nodes: collection.decks).totals.waiting,
            selection: DeckListSelection(homeDeckPage: navigation.homeDeckPage)) { navigation.openHome() }
    }
    /// Today beside the list (N14.1) already shows the previous session.
    private var showsPreviousSession: Bool {
        !(todayBesideDeckList && collection.navigation.homeDeckPage == nil)
    }
    /// N14.1: the sidebar layout's root is Today; the tab layout's is the deck list.
    private var showsToday: Bool { shellLayout == .sidebar }
    #else
    private var activeSearch: String? { nil }
    private var todayRow: HomeTodayRow? { nil }
    private var showsToday: Bool { true }
    private var showsPreviousSession: Bool { true }
    #endif
    private var pageTitle: String {
        // The sidebar's first row says Today in every collection state, so its page does too.
        guard let deckID else {
            #if !os(macOS)
            // N18.3: the welcome names itself in the content; the tab layout's bar repeats nothing over it.
            if collection.state == .noFile, shellLayout == .compact { return "" }
            // N26.3: beside Today the column is the app's library, as Music's and Podcasts'
            // sidebars name theirs; its decks sit under their own "Decks" header.
            if todayBesideDeckList { return "Meguri" }
            #endif
            return HomeHeading.rootTitle(showsToday: showsToday)
        }
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
        case .noFile: EmptyView() // The welcome owns the page (N18.3).
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

    /// N18.3: one row for sync, the Account & Sync entry; then the import and the empty collection.
    private var welcome: some View {
        HomeWelcome(
            sync: .init(enabled: collection.navigation.sheet == nil) {
                guard collection.navigation.sheet == nil else { return }
                #if os(macOS)
                collection.syncPresentation.settingsRequest = UUID()
                openSettings()
                #else
                collection.navigation.present(.accountSync)
                #endif
            },
            importFile: .init(enabled: collection.entry.canPresent) { collection.entry.present() },
            // Availability, not the window's focus: the three rows look the same in an
            // inactive window. A wait for sync says so in the row (N18.3).
            startEmpty: .init(enabled: collection.startEmptyAvailable,
                unavailableReason: collection.startEmptyAvailable || !collection.startEmptyWaitsForSync
                    ? nil : WelcomeChoice.waitsForSyncReason) { Task { await collection.startEmptyFromPress() } },
            error: collection.error,
            previousAnswerCount: collection.previousRecord?.answers.count)
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
            // N14.11: this deck's and its subdecks' activity, after the subdecks.
            ActivityHeatmapSection(store: collection.activity, scope: .deck(deck.id),
                                   widths: collection.navigation.activityRowWidths, page: .deck)
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
        .activityLoading(collection.activity, scope: .deck(deck.id), active: collection.heatmapRefreshes,
                         widths: collection.navigation.activityRowWidths, page: .deck)
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
            // While Study starts, the button keeps its look until the card shows (N16.8).
            .disabled(!(collection.canStudy || collection.studyStarting) || collection.navigation.sheet != nil
                || collection.addNotePresentation.opening)
            .accessibilityIdentifier("home.deck.study")
            .modifier(StudyActionTarget(collection: collection, deckID: deck.id))
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
    /// N26.4: beside Today the field is the column's only way into search, so it always shows.
    let alwaysShown: Bool
    let collection: ReviewCollection
    @Bindable var search: DeckListSearch
    func body(content: Content) -> some View {
        if enabled {
            // Out of the way until the toolbar's search button presents it, so the large title stays.
            content.searchable(text: $search.text, isPresented: $search.presented, placement: .navigationBarDrawer(displayMode: alwaysShown ? .always : .automatic))
                // N20.5: Return opens the first row, deck or create row, as in the sidebar layouts.
                .onSubmit(of: .search) {
                    guard let query = search.query,
                          let target = DeckSearchRows.results(collection, search, query: query)?
                              .targets(offeringCreate: search.offersCreate(generation: collection.generation)).first else { return }
                    DeckSearchActivation.activate(target, collection: collection, search: search, layout: .compact)
                }
        } else {
            content
        }
    }
}
#endif

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
    private let today: HomeTodayRow?
    /// N14.11: the collection's activity after the deck rows; nil beside Today, which shows it.
    private let activity: ActivityHeatmapStore?
    private let activityActive: Bool
    @State private var rowFrames: [Int64: CGRect] = [:]
    @State private var restoring: Int64?
    @State private var tracking = false
    /// A row the list selected itself (keys) while its page opens: shown until the detail follows.
    @State private var pendingRow: HomeListRow?

    public init(nodes: [CountedDeckNode], navigation: AppNavigation? = nil, collection: ReviewCollection? = nil,
                previousAnswerCount: Int? = nil, error: String? = nil,
                isExpanded: ((Int64) -> Bool)? = nil,
                isExpansionPending: ((Int64) -> Bool)? = nil,
                setExpanded: ((Int64, Bool, Bool) -> Void)? = nil,
                today: HomeTodayRow? = nil,
                activity: ActivityHeatmapStore? = nil, activityActive: Bool = true,
                select: @escaping (Int64) -> Void) {
        self.nodes = nodes; self.navigation = navigation; self.collection = collection
        self.previousAnswerCount = previousAnswerCount; self.error = error; self.select = select
        self.isExpanded = isExpanded; self.setExpanded = setExpanded
        self.isExpansionPending = isExpansionPending
        self.today = today
        self.activity = activity; self.activityActive = activityActive
    }

    public var body: some View {
        // Read here, not only in the binding: expanding or collapsing a deck moves the mark.
        let marked = today.flatMap { markedRow($0.selection) }
        GeometryReader { viewport in
            ScrollViewReader { proxy in
                // N26.1: beside Today the list draws its selection itself, as the iPad sidebar
                // does. A tap runs the row's button; a selection the list makes itself (keys)
                // takes the same path, and shows at once while the page opens. Otherwise the
                // binding reads where the detail is.
                List(selection: today.map { today in
                    Binding(get: { pendingRow ?? marked }, set: { row in
                        guard let row, row != (pendingRow ?? marked) else { return }
                        pendingRow = row
                        switch row {
                        case .today: today.open()
                        case .deck(let id): select(id)
                        }
                    })
                }) {
                    if let today {
                        // N14.10, N26.3: Today in its own section, then the decks under "Decks".
                        Section { HomeTodayRowView(row: today).id(InfoAnchor.today) }
                        Section {
                            branches
                        } header: {
                            // The system's header grey falls short of 4.5:1 here, as on the iPad sidebar.
                            Text("Decks", bundle: .module)
                                .foregroundStyle(.token(.textSecondary))
                                .accessibilityIdentifier("home.collection.decksHeader")
                        }
                    } else {
                        branches
                    }
                    if let activity, let navigation {
                        ActivityHeatmapSection(store: activity, scope: .collection, widths: navigation.activityRowWidths,
                                               page: .deckList)
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
                // The detail followed (or went elsewhere): the navigation's mark is the selection again.
                .onChange(of: marked) { _, _ in pendingRow = nil }
                .activityLoading(activity, scope: .collection, active: activityActive,
                                 widths: navigation?.activityRowWidths, page: .deckList)
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
                    if today != nil, let id = saved?.viewID as? Int64, id == nodes.first?.id {
                        // Saved at the first deck, the list was at its top: the Today row is part of it.
                        restoring = nil
                        scrollToTop(proxy)
                        tracking = true
                    } else if let id = saved?.viewID as? Int64, contains(id, in: nodes) {
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

    private var branches: some View {
        ForEach(nodes, id: \.id) { node in
            HomeDeckBranch(node: node, collection: collection, ancestors: [], depth: 0, select: select,
                isExpanded: isExpanded, isExpansionPending: isExpansionPending,
                setExpanded: setExpanded, selection: today?.selection, frameChanged: recordFrame)
        }
    }

    private enum InfoAnchor: Hashable { case today, previousSession, error }

    /// The row the list beside Today selects (N14.10): Today, the deck whose page shows,
    /// or the outermost collapsed deck that hides it.
    private func markedRow(_ selection: DeckListSelection) -> HomeListRow? {
        guard let deck = selection.deck else { return .today }
        func visible(in nodes: [CountedDeckNode]) -> Int64? {
            for node in nodes {
                if node.id == deck { return node.id }
                if selection.marks(node, expanded: false) {
                    return (isExpanded?(node.id) ?? !node.studyCollapsed) ? visible(in: node.children) : node.id
                }
            }
            return nil
        }
        return visible(in: nodes).map(HomeListRow.deck)
    }
    private func recordFrame(_ id: Int64, _ frame: CGRect?) { rowFrames[id] = frame }
    private func contains(_ id: Int64, in nodes: [CountedDeckNode]) -> Bool {
        nodes.contains { node in
            node.id == id || ((isExpanded?(node.id) ?? !node.studyCollapsed) && contains(id, in: node.children))
        }
    }
    private func scrollToTop(_ proxy: ScrollViewProxy) {
        if today != nil {
            // The Today row is the list's first: the top is above the first deck.
            proxy.scrollTo(InfoAnchor.today, anchor: .top)
        } else if let first = nodes.first {
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
    /// What shows beside this list (N14.10); nil where the list marks no row.
    var selection: DeckListSelection? = nil
    let frameChanged: (Int64, CGRect?) -> Void
    private var selected: Bool { selection?.marks(node, expanded: expanded.wrappedValue) == true }
    #if os(macOS)
    @Environment(\.locale) private var locale
    #else
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    /// The list's vertical padding around a row's 44-point content (N26.5), growing with the text.
    @ScaledMetric(relativeTo: .body) private var rowInset = Spacing.xxSmall
    #endif

    private var path: String {
        node.name.contains("::") ? node.name : (ancestors + [node.name]).joined(separator: "::")
    }
    /// N20.3: shown and spoken paths join their components with " › ".
    private var spokenPath: String { path.components(separatedBy: "::").joined(separator: " › ") }

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
            // N26.5: every deck row is as tall as one without subdecks. The row button's 44-point
            // minimum is the row's content, so the list's vertical padding around it shrinks;
            // the padding alone, added to that minimum, made rows with a disclosure taller.
            .listRowInsets(.vertical, rowInset)
            .tag(HomeListRow.deck(node.id))
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
            setExpanded: setExpanded, selection: selection, frameChanged: frameChanged))
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
            .accessibilityLabel(Text(verbatim: spokenPath))
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
                // The touch target's minimum on every row, with or without a disclosure (N26.5).
                .frame(minHeight: ControlMetrics.minimumTarget)
                #endif
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
        }
        // Keep the row action distinct from its native disclosure control.
        .buttonStyle(.plain)
        .accessibilityLabel(Text(verbatim: spokenPath))
        .accessibilityValue(Text("\(Text(node.counts.new, format: .number)) new · \(Text(node.counts.learning, format: .number)) learning · \(Text(node.counts.review, format: .number)) due", bundle: .module))
        .accessibilityIdentifier("home.deck.row.\(node.id)")
        .accessibilityAddTraits(selected ? .isSelected : [])
        #if os(macOS)
        .help(Text(verbatim: spokenPath))
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
                    // Primary on the selection's highlight, where the secondary token falls short of 4.5:1.
                    .foregroundStyle(.token(selected ? .textPrimary : .textSecondary))
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

/// Which row the deck list beside the detail marks (N14.10): Today while no deck page
/// shows, otherwise that deck's row, or the collapsed parent that hides it.
public nonisolated struct DeckListSelection: Equatable, Sendable {
    /// The deck whose page the detail shows; nil while it shows Today.
    public let deck: Int64?
    public init(deck: Int64?) { self.deck = deck }
    public init(homeDeckPage: DeckID?) { deck = homeDeckPage?.rawValue }
    public var today: Bool { deck == nil }

    /// A collapsed deck stands for the subdeck it hides.
    public func marks(_ node: CountedDeckNode, expanded: Bool) -> Bool {
        guard let deck else { return false }
        return node.id == deck || (!expanded && Self.holds(deck, under: node))
    }
    private static func holds(_ deck: Int64, under node: CountedDeckNode) -> Bool {
        node.children.contains { $0.id == deck || holds(deck, under: $0) }
    }
}

/// The deck list's first row beside Today (N14.10): what it counts, what the list
/// marks, and the way back to Today.
public struct HomeTodayRow {
    public let waiting: Int
    public let selection: DeckListSelection
    public let open: () -> Void
    public var selected: Bool { selection.today }
    public init(waiting: Int, selection: DeckListSelection = DeckListSelection(deck: nil), open: @escaping () -> Void) {
        self.waiting = waiting; self.selection = selection; self.open = open
    }
}

private struct HomeTodayRowView: View {
    let row: HomeTodayRow
    #if !os(macOS)
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @ScaledMetric(relativeTo: .body) private var rowInset = Spacing.xxSmall
    #endif

    var body: some View {
        Button(action: row.open) {
            content
                .frame(maxWidth: .infinity, minHeight: ControlMetrics.minimumTarget, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("Today", bundle: .module))
        .accessibilityValue(Text("\(row.waiting) cards waiting", bundle: .module))
        .accessibilityAddTraits(row.selected ? .isSelected : [])
        .accessibilityIdentifier("home.today.row")
        #if !os(macOS)
        // As a deck row (N26.5): the 44-point target is the content, with a tighter padding around it.
        .listRowInsets(.vertical, rowInset)
        #endif
        .tag(HomeListRow.today)
    }

    private var stacks: Bool {
        #if os(macOS)
        false
        #else
        dynamicTypeSize.isAccessibilitySize
        #endif
    }

    private var content: some View {
        // As a deck row: at the accessibility sizes the count goes under the title.
        (stacks ? AnyLayout(VStackLayout(alignment: .leading)) : AnyLayout(HStackLayout())) {
            // Not a Label: the Dynamic Type audit read its title as text that may be cut.
            HStack(spacing: Spacing.small) {
                // The sidebar layouts' symbol for Today (N14.1, N21.1).
                Image(systemName: "calendar")
                    .foregroundStyle(.tint)
                    .accessibilityHidden(true)
                Text("Today", bundle: .module)
                    .foregroundStyle(.token(.textPrimary))
                    // Rows grow with the text size rather than cut it.
                    .fixedSize(horizontal: false, vertical: true)
            }
            if !stacks { Spacer() }
            if row.waiting > 0 {
                Text(row.waiting, format: .number)
                    .foregroundStyle(.token(row.selected ? .textPrimary : .textSecondary))
                    .monospacedDigit()
                    .lineLimit(1)
                    .fixedSize()
            }
        }
    }
}

/// A row of the deck list, as its selection names it (N14.10).
enum HomeListRow: Hashable {
    case today
    case deck(Int64)
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
    /// Nil in the DTO preview (`HomeSidebarPreview`): the rows draw and do nothing.
    let collection: ReviewCollection?
    let node: CountedDeckNode
    let ancestors: [String]
    /// The preview's selection; the live rows read the navigation's.
    var previewSelection: SidebarItem? = nil
    @Environment(\.locale) private var locale
    private var name: String { node.name.components(separatedBy: "::").last ?? node.name }
    /// N24.4: the full path, then the three counts, zeros included.
    private var helpTag: Text { Text(verbatim: path + "\n" + node.counts.summary(locale: locale)) }
    /// N24.4: the same three counts, "to review" in place of "due".
    private var countsValue: Text { Text(verbatim: node.counts.summary(locale: locale)) }
    /// N20.3: shown and spoken paths join their components with " › ".
    private var path: String { (ancestors + [name]).joined(separator: " › ") }
    private var quiet: Bool {
        DeckWaiting(counts: node.counts, filtered: node.filtered)
            .quietInSidebar(selected: (collection?.navigation.sidebarSelection ?? previewSelection) == .deck(DeckID(node.id)))
    }
    private var expanded: Binding<Bool> {
        Binding(get: { collection?.studyExpanded(deckID: node.id) ?? !node.studyCollapsed }, set: {
            collection?.setStudyExpanded(deckID: node.id, expanded: $0,
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
                        AnyView(HomeSidebarBranch(collection: collection, node: child, ancestors: ancestors + [name],
                                                  previewSelection: previewSelection))
                            .tag(SidebarItem.deck(DeckID(child.id)))
                    }
                } label: { row }
                .tag(SidebarItem.deck(DeckID(node.id)))
                // No accessibility of the group's own on iOS: the list flattens its rows, so
                // an identifier here reached every row under it (the parent's and each
                // subdeck's cell). The system's disclosure control reports its own state.
            }
        }
        .id(node.id)
    }

    private var row: some View {
        Group {
            #if os(macOS)
            Button { collection?.openDeckPage(deckID: node.id) } label: {
                Text(verbatim: name)
                    // N14.6: nothing waiting reads quieter, except on the selection highlight.
                    .foregroundStyle(quiet ? .secondary : .primary)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            // H12: the first click opens the page, as a click does; the second starts the review.
            .simultaneousGesture(TapGesture(count: 2).onEnded {
                if let collection { DeckListStudy.start(deckID: node.id, collection: collection) }
            })
            .help(helpTag)
            .accessibilityLabel(Text(verbatim: path))
            .accessibilityValue(countsValue)
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
            .accessibilityValue(countsValue)
            // N24.4: the help tag with a pointer.
            .help(helpTag)
            #endif
        }
        #if !os(macOS)
        .accessibilityIdentifier("home.sidebar.deck.\(node.id)")
        #endif
        #if os(iOS)
        // H12: a double-click with the iPad's pointer starts the review; touch has no double tap.
        .gesture(PointerDoubleClick { if let collection { DeckListStudy.start(deckID: node.id, collection: collection) } })
        #endif
        .deckMenu(collection: collection, deck: node, place: "sidebar")
        .tag(SidebarItem.deck(DeckID(node.id)))
        #if os(macOS)
        // N24.4: every card waiting, as an integer badge the system hides at zero.
        .badge(node.counts.waiting)
        #endif
    }
}

/// The production sidebar's deck rows over sample decks (route `home/sidebar/typical`):
/// one with only new cards, one with nothing waiting, nested decks and a filtered deck, so
/// the badge (cards waiting) and the quieter nothing-waiting name can be captured.
public enum HomeSidebarPreview {
    public static var nodes: [CountedDeckNode] {
        func deck(_ id: Int64, _ name: String, _ counts: (UInt32, UInt32, UInt32), filtered: Bool = false,
                  collapsed: Bool = false, _ children: [CountedDeckNode] = []) -> CountedDeckNode {
            CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: collapsed, browserCollapsed: false,
                counts: ReviewCounts(new: counts.0, learning: counts.1, review: counts.2), children: children)
        }
        return [
            deck(1, "Languages", (6, 23, 382), [
                deck(2, "Languages::Español", (6, 23, 382), [deck(3, "Languages::Español::Vocabulary", (6, 3, 120))]),
                deck(4, "Languages::日本語", (0, 0, 0)),
            ]),
            deck(5, "Music Theory", (12, 0, 0)),
            deck(6, "Quick Review", (0, 0, 30), filtered: true),
            deck(7, "Reading", (0, 0, 0)),
            deck(8, "Science", (10, 2, 85), collapsed: true, [deck(9, "Science::Chemistry", (10, 2, 85))]),
        ]
    }

    /// The rows, each tagged as the live list tags them; `selection` marks one.
    public static func decks(selection: SidebarItem?) -> some View {
        ForEach(nodes, id: \.id) { node in
            HomeSidebarBranch(collection: nil, node: node, ancestors: [], previewSelection: selection)
                .tag(SidebarItem.deck(DeckID(node.id)))
        }
    }
}

/// Starting a deck from the deck list (H3, H12): a double-click, or Return or S on its
/// selected row. A filtered deck shows R53's explanation and a deck with nothing to study
/// its page's state, on its page; any other goes through the deck page's Study path.
public enum DeckListStudy {
    /// After the press's own actions: the double-click's second click and the selection's
    /// open run first, and the page they open never ends the review this starts.
    /// A later selection (another row, Today) cancels the request: see
    /// `ReviewCollection.startFromDeckList`.
    public static func start(deckID: Int64, collection: ReviewCollection) {
        let request = collection.deckListStudyRequest(deckID: deckID)
        Task { await collection.startFromDeckList(request) }
    }

    /// Today's Study (N24.3) from its keys: nothing while nothing waits or Study is unavailable,
    /// when the key arrives and again when the start runs.
    public static func startAll(collection: ReviewCollection) {
        guard collection.studyAllAvailable else { return }
        Task { await collection.startStudyAll() }
    }
}

#if os(iOS)
/// A double-click with a pointer (trackpad or mouse) only: touches never reach it, and it
/// recognizes alongside the list's own selection, so a click (a tap) still selects the row.
private struct PointerDoubleClick: UIGestureRecognizerRepresentable {
    let action: () -> Void

    func makeUIGestureRecognizer(context: Context) -> UITapGestureRecognizer {
        let recognizer = UITapGestureRecognizer()
        recognizer.numberOfTapsRequired = 2
        recognizer.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.indirectPointer.rawValue)]
        recognizer.cancelsTouchesInView = false
        recognizer.delaysTouchesEnded = false
        recognizer.delegate = context.coordinator
        return recognizer
    }

    func handleUIGestureRecognizerAction(_ recognizer: UITapGestureRecognizer, context: Context) {
        if recognizer.state == .ended { action() }
    }

    func makeCoordinator(converter: CoordinateSpaceConverter) -> Coordinator { Coordinator() }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer,
                               shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool { true }
    }
}
#endif

private func studyDisclosureIncludesDescendants() -> Bool {
    #if os(macOS)
    NSEvent.modifierFlags.contains(.option)
    #else
    false
    #endif
}

/// Study is refused while the collection could not be prepared for it
/// (`sched/app-coordination` F5.3). Shown at the entry point that was used;
/// everything else in the collection stays usable.
public struct StudyRefusalFeedback: ViewModifier {
    let collection: ReviewCollection?
    public init(collection: ReviewCollection?) { self.collection = collection }
    public func body(content: Content) -> some View {
        content.alert(Text("study.prepare.failed", bundle: .module), isPresented: Binding(
            get: { collection?.studyRefusal != nil && collection?.navigation.sheet == nil },
            set: { if !$0 { collection?.dismissStudyRefusal() } }
        )) {
            Button { Task { await collection?.retryRefusedStudy() } } label: { Text("study.prepare.tryAgain", bundle: .module) }
                .keyboardShortcut(.defaultAction)
            Button(role: .cancel) { collection?.dismissStudyRefusal() } label: { Text("Cancel", bundle: .module) }
        }
    }
}
