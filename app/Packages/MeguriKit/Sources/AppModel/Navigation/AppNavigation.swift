import Observation
import SwiftUI

/// Where the user is in the app (KTD10). One instance sits above the switch
/// between the tab layout and the sidebar layout, and both read and write
/// the same state, so changing layout (rotation, resizing, Split View,
/// folding) keeps the section, the selected deck, every stack, the scroll
/// positions and an open review (R5). Neither layout writes the state just by
/// reading it.
///
/// - The current `section` and the `selectedDeck`, whose page the Home
///   section shows (`homeDeckPage`): the sidebar layout as the Home
///   section's root, the tab layout pushed on the Decks tab. Another
///   section keeps the deck selected, and so does a relaunch in the tab
///   layout, which shows Home rather than the page (R13).
/// - One stack per section (R19), `path(_:)`.
/// - The open sheet (R25) and the review (R21–R23).
/// - The sidebar's visibility (R20) and per-screen scroll positions.
/// - Feature-owned state that must outlive a layout switch (Add and edit
///   drafts, Browse's search session), `featureState(_:make:)`.
@Observable
public final class AppNavigation {
    public private(set) var section: AppSection = .home
    /// The deck the user last opened, if any. It stays selected while
    /// another section is shown, and while Home shows its own root after a
    /// relaunch in the tab layout (R13).
    public private(set) var selectedDeck: DeckID?
    /// Whether the Home section shows the selected deck's page rather than
    /// its own root. Only a relaunch in the tab layout hides the page of a
    /// selected deck; leaving the page (Home, a pop) deselects the deck.
    private var showsDeckPage = false
    public enum SheetHost: Equatable { case main, settings }
    private var presentedSheetHost: SheetHost = .main
    public var sheetHost: SheetHost {
        get { presentedSheetHost }
        set {
            guard addSheetRequest == nil || !presentedSheet.isAddNote || newValue == .main else { return }
            presentedSheetHost = newValue
        }
    }
    private var presentedSheet: SheetRoute?
    public var sheet: SheetRoute? {
        get { presentedSheet }
        set {
            if presentedSheet.isAddNote || newValue.isAddNote, let addSheetRequest {
                addSheetRequest(newValue)
            } else {
                presentedSheet = newValue
            }
        }
    }
    // Installed only by the production editor composition. Isolated shell
    // fixtures retain their existing presentation behavior.
    @ObservationIgnored var addSheetRequest: (@MainActor (SheetRoute?) -> Void)?
    @ObservationIgnored var retainedAddPresentation: AddNotePresentation?

    func commitAddSheet(_ route: SheetRoute?) {
        guard route == nil || route.isAddNote else { return }
        if route != nil { sheetHost = .main }
        presentedSheet = route
    }
    public private(set) var review: ReviewPresentation?
    /// The sidebar layout's columns. The sidebar's own toggle, ⌃⌘S and the
    /// iPad edge swipe write it (R20).
    public var columnVisibility: NavigationSplitViewVisibility = .all
    /// Browse's search text, as a deep link (R319) or another screen hands
    /// it to Browse (U210).
    public var browseSearch = ""

    private var paths: [AppSection: [Destination]] = [:] {
        didSet {
            // N20.9: the Deck Options page left its stack (back, a pop, another
            // row or section): its draft ends. A layout switch keeps the stack.
            let open = { (paths: [AppSection: [Destination]]) in
                paths.values.contains { $0.contains { if case .deckOptions = $0 { true } else { false } } }
            }
            if open(oldValue), !open(paths) { onDeckOptionsLeft?() }
        }
    }
    /// The collection ends the Deck Options draft when its page leaves the stacks.
    @ObservationIgnored public var onDeckOptionsLeft: (() -> Void)?
    /// Whether the Deck Options page is on a stack.
    public var showsDeckOptions: Bool {
        paths.values.contains { $0.contains { if case .deckOptions = $0 { true } else { false } } }
    }
    private var scrollPositions: [ScrollAnchorKey: ScrollPosition] = [:]
    /// The sidebar visibility to restore when the review ends (R21).
    private var columnVisibilityBeforeReview: NavigationSplitViewVisibility?
    /// The sidebar visibility to restore when a narrow window widens again
    /// (N22.1: the Note Types Cards tab under about 1,100 points).
    private var columnVisibilityBeforeNarrow: NavigationSplitViewVisibility?
    /// The Settings tab's own section: Settings, or Note Types pushed on it.
    private var settingsTabSection: AppSection = .settings
    /// Set by a restore: the launch layout decides whether the deck page
    /// opens (R13).
    private var restoredDeckPagePending = false
    @ObservationIgnored private var featureStates: [ObjectIdentifier: AnyObject] = [:]

    /// Production deck rows await the collection-owned save before opening a page.
    @ObservationIgnored public var deckSelectionAction: (@MainActor (DeckID) -> Void)?
    @ObservationIgnored private(set) var intentRevision: UInt64 = 0

    func beginDeckSelection() -> UInt64 {
        intentRevision &+= 1
        return intentRevision
    }

    public init() {}

    /// The navigation a relaunch starts with (R12, R13): the last section,
    /// selected deck and sidebar visibility, never a review. Call
    /// `adoptLaunchLayout(_:)` once the first layout is known.
    public init(restoring state: UIState) {
        section = state.section
        selectedDeck = state.selectedDeck
        showsDeckPage = selectedDeck != nil
        restoredDeckPagePending = showsDeckPage
        if state.section.tab == .settings {
            settingsTabSection = state.section
        }
        columnVisibility = state.sidebarVisible ? .all : .detailOnly
    }

    // MARK: Stacks (R19)

    /// The screens pushed on `section`'s root.
    public func path(_ section: AppSection) -> [Destination] {
        paths[section] ?? []
    }

    public func setPath(_ path: [Destination], for section: AppSection) {
        guard path != self.path(section) else { return }
        intentRevision &+= 1
        paths[section] = path.isEmpty ? nil : path
    }

    /// Pushes `destination` on the current section's stack.
    public func push(_ destination: Destination) {
        setPath(path(section) + [destination], for: section)
    }

    /// N20.9: back from Deck Options to its deck page, once.
    /// Pops the Deck Options page from whichever stack holds it, so a draft
    /// that ends while another section is current leaves no stale page.
    public func popDeckOptions(_ deck: DeckID) {
        for (stack, path) in paths {
            guard case .deckOptions(deck)? = path.last else { continue }
            setPath(Array(path.dropLast()), for: stack)
        }
    }

    /// The deck whose page the Home section shows, if it shows one.
    public var homeDeckPage: DeckID? {
        showsDeckPage ? selectedDeck : nil
    }

    // MARK: Sidebar layout (R17, R21, R22)

    /// The sidebar row of the current section, or of the deck whose page
    /// Home shows.
    public var sidebarSelection: SidebarItem {
        if section == .home, let homeDeckPage {
            return .deck(homeDeckPage)
        }
        return .section(section)
    }

    /// Selects a sidebar row. Another row than the selected one ends an open
    /// review first, without asking (R22).
    public func select(_ item: SidebarItem) {
        if case .deck(let deck) = item, let deckSelectionAction {
            deckSelectionAction(deck)
            return
        }
        intentRevision &+= 1
        guard item != sidebarSelection else { return }
        endReview()
        switch item {
        case let .section(section):
            if section == .home {
                closeDeckPage()
            }
            show(section)
        case let .deck(deck):
            openDeckPage(deck)
        }
    }

    /// The detail column's stack: the current section's.
    public var detailPath: [Destination] {
        get { path(section) }
        set { setPath(newValue, for: section) }
    }

    // MARK: Tab layout (R18, R19, R23)

    public var compactTab: CompactTab { section.tab }

    /// A tab's stack. The Decks tab holds the deck page Home shows above
    /// Home, the Settings tab Note Types above Settings.
    public func compactPath(_ tab: CompactTab) -> [Destination] {
        switch tab {
        case .decks:
            return homeDeckPage.map { [.deckPage($0)] + path(.home) } ?? path(.home)
        case .settings:
            return settingsTabSection == .noteTypes ? [.noteTypes] + path(.noteTypes) : path(.settings)
        case .browse, .stats:
            return path(tab.section)
        }
    }

    /// What a tab's `NavigationStack` writes back: a push or a pop.
    public func setCompactPath(_ newPath: [Destination], for tab: CompactTab) {
        if newPath != compactPath(tab) { intentRevision &+= 1 }
        switch tab {
        case .decks:
            if case let .deckPage(deck)? = newPath.first {
                showDeckPage(deck)
                setPath(Array(newPath.dropFirst()), for: .home)
            } else {
                // A pop off the deck page, or Home's own stack: a hidden
                // page's deck stays selected.
                closeDeckPage()
                setPath(newPath, for: .home)
            }
        case .settings:
            let tabSection: AppSection
            if newPath.first == .noteTypes {
                tabSection = .noteTypes
                setPath(Array(newPath.dropFirst()), for: .noteTypes)
            } else {
                tabSection = .settings
                setPath(newPath, for: .settings)
            }
            settingsTabSection = tabSection
            if section.tab == .settings {
                section = tabSection
            }
        case .browse, .stats:
            setPath(newPath, for: tab.section)
        }
    }

    /// A tap on a tab. On the selected tab it pops the stack to its root, or,
    /// at the root, scrolls the root to the top (R19).
    public func selectTab(_ tab: CompactTab) {
        intentRevision &+= 1
        guard tab == compactTab else {
            show(tab == .settings ? settingsTabSection : tab.section)
            return
        }
        if compactPath(tab).isEmpty {
            scrollToTop(.section(tab.section))
        } else {
            setCompactPath([], for: tab)
        }
    }

    // MARK: Moving around

    /// Shows `section` as it was left.
    public func show(_ section: AppSection) {
        intentRevision &+= 1
        if section.tab == .settings {
            settingsTabSection = section
        }
        self.section = section
    }

    /// Opens `deck`'s page in the Home section: the sidebar selects the deck,
    /// the Decks tab pushes its page. An open review ends.
    public func openDeckPage(_ deck: DeckID) {
        intentRevision &+= 1
        endReview()
        showDeckPage(deck)
        setPath([], for: .home)
        show(.home)
    }

    /// Home, at its root.
    public func openHome() {
        intentRevision &+= 1
        endReview()
        closeDeckPage()
        setPath([], for: .home)
        show(.home)
    }

    /// Selects `deck` and shows its page in Home. A new deck page starts
    /// with an empty stack on it, as does a page shown again over Home's
    /// root.
    private func showDeckPage(_ deck: DeckID) {
        guard deck != homeDeckPage else { return }
        selectedDeck = deck
        showsDeckPage = true
        setPath([], for: .home)
    }

    /// Leaves the deck page Home shows, which deselects its deck. With no
    /// page shown, nothing changes.
    private func closeDeckPage() {
        guard showsDeckPage else { return }
        selectedDeck = nil
        showsDeckPage = false
        setPath([], for: .home)
    }

    /// N20.7, N14.1: when the selected deck or a deck page on Home's stack is
    /// among `decks` (deleted, or the default deck emptied), Home returns to
    /// its root (Today, or the tab layout's deck list). The section stays.
    func deckPagesRemoved(_ decks: Set<DeckID>) {
        let stacked = path(.home).contains {
            if case .deckPage(let deck) = $0 { return decks.contains(deck) }
            return false
        }
        guard stacked || selectedDeck.map(decks.contains) == true else { return }
        intentRevision &+= 1
        selectedDeck = nil; showsDeckPage = false; restoredDeckPagePending = false
        setPath([], for: .home)
    }

    /// Drop presentation bound to the old generation. Entry keeps its progress
    /// sheet until its FFI result confirms that the new host is retained. The
    /// server workflow survives adoption to show its outcome and media decision.
    public func collectionReplaced(keepingEntrySheet: Bool = false) {
        intentRevision &+= 1
        endReview()
        selectedDeck = nil; showsDeckPage = false; restoredDeckPagePending = false
        paths.removeAll(); scrollPositions.removeAll(); featureStates.removeAll()
        browseSearch = ""; section = .home; settingsTabSection = .settings
        if sheet != .manualTransfer, !keepingEntrySheet || sheet != .collectionEntry { dismissSheet() }
    }

    // MARK: Scroll positions (R5, R19)

    public func scrollPosition(for key: ScrollAnchorKey) -> ScrollPosition {
        scrollPositions[key] ?? ScrollPosition()
    }

    public func setScrollPosition(_ position: ScrollPosition, for key: ScrollAnchorKey) {
        scrollPositions[key] = position
    }

    public func scrollToTop(_ key: ScrollAnchorKey) {
        var position = scrollPosition(for: key)
        position.scrollTo(edge: .top)
        scrollPositions[key] = position
    }

    // MARK: Review (R21–R23)

    /// Presents a review. In the sidebar layout the review fills the detail
    /// area and the sidebar collapses until the review ends (R21).
    /// N22.1: a screen that needs the window's width (the Note Types Cards
    /// tab under about 1,100 points) hides the sidebar, as Mail and Xcode
    /// hide their navigators; it comes back as it was once the window is
    /// wide again or the screen goes. The person's own choice is what the
    /// app remembers.
    public func setNarrowWorkspace(_ narrow: Bool) {
        if narrow {
            guard columnVisibilityBeforeNarrow == nil, columnVisibility != .detailOnly else { return }
            columnVisibilityBeforeNarrow = columnVisibility
            columnVisibility = .detailOnly
        } else {
            guard let saved = columnVisibilityBeforeNarrow else { return }
            columnVisibilityBeforeNarrow = nil
            // The person opened the sidebar meanwhile: leave it as it is.
            if columnVisibility == .detailOnly { columnVisibility = saved }
        }
    }

    public func startReview(_ presentation: ReviewPresentation) {
        if review == nil {
            columnVisibilityBeforeReview = columnVisibility
        }
        columnVisibility = .detailOnly
        review = presentation
    }

    /// Closes the review, if one is open, and restores the sidebar as it was
    /// before the review.
    public func endReview() {
        guard review != nil else { return }
        review = nil
        if let saved = columnVisibilityBeforeReview {
            columnVisibility = saved
        }
        columnVisibilityBeforeReview = nil
    }

    // MARK: Sheets (R25)

    public func present(_ sheet: SheetRoute) {
        if self.sheet.isAddNote || Optional(sheet).isAddNote, addSheetRequest != nil {
            self.sheet = sheet
            return
        }
        if self.sheet != sheet { sheetHost = .main }
        self.sheet = sheet
    }

    public func dismissSheet() {
        sheet = nil
    }

    // MARK: Launch and relaunch (R12, R13)

    /// What the device keeps for the next launch (KTD12). During a review the
    /// sidebar visibility is the one the review will restore, not its
    /// automatic collapse.
    public var uiState: UIState {
        UIState(
            section: section,
            selectedDeck: selectedDeck,
            sidebarVisible: (columnVisibilityBeforeReview ?? columnVisibilityBeforeNarrow ?? columnVisibility) != .detailOnly
        )
    }

    /// Tells a restored navigation which layout the app launched in: the tab
    /// layout opens on Home rather than on the last deck page, with the deck
    /// still selected, so the next launch in the sidebar layout opens its
    /// page again (R13). Only the first call after a restore does anything.
    public func adoptLaunchLayout(_ layout: ShellLayout) {
        guard restoredDeckPagePending else { return }
        restoredDeckPagePending = false
        if layout == .compact {
            showsDeckPage = false
        }
    }

    // MARK: Feature state

    /// The feature model of type `State`, made by `make` the first time it is
    /// asked for and kept for the app's run, whatever the layout does to the
    /// views that use it.
    public func featureState<State: AnyObject>(_ type: State.Type = State.self, make: () -> State) -> State {
        let key = ObjectIdentifier(type)
        if let state = featureStates[key] as? State {
            return state
        }
        let state = make()
        featureStates[key] = state
        return state
    }
}

extension Optional where Wrapped == SheetRoute {
    var isAddNote: Bool {
        if case .add = self { return true }
        return false
    }
}

extension View {
    /// Keeps this scroll view's position in `navigation` under `key`: the
    /// row at the top as the user scrolls, so the screen comes back there
    /// after a layout switch or a pop (R5), and a scroll to the top when the
    /// navigation asks for one (a second tab reselect, R19). Apply it to a
    /// `ScrollView` whose content has a `scrollTargetLayout()` of rows
    /// identified by `idType`.
    public func navigationScrollPosition<ID: Hashable & Sendable>(
        _ navigation: AppNavigation, _ key: ScrollAnchorKey, idType: ID.Type
    ) -> some View {
        modifier(NavigationScrollPosition<ID>(navigation: navigation, key: key))
    }
}

private struct NavigationScrollPosition<ID: Hashable & Sendable>: ViewModifier {
    let navigation: AppNavigation
    let key: ScrollAnchorKey
    /// The scroll view's own position, for the scrolls the navigation asks
    /// for. Writing the user's position into a bound `ScrollPosition` would
    /// make the scroll view jump to it while the user scrolls.
    @State private var position = ScrollPosition(idType: ID.self)
    /// The kept row the view is scrolling back to. Until it is on screen,
    /// the rows on screen are not the user's (a view built hidden, under a
    /// pushed screen, shows its top rows) and are not kept.
    @State private var restoring: ID?
    @State private var tracking = false

    func body(content: Content) -> some View {
        content
            .scrollPosition($position, anchor: .top)
            .onScrollTargetVisibilityChange(idType: ID.self) { visible in
                guard let top = visible.first else { return }
                guard tracking else {
                    if let restoring, visible.contains(restoring) {
                        tracking = true
                    }
                    return
                }
                navigation.setScrollPosition(ScrollPosition(id: top, anchor: .top), for: key)
            }
            // Built again (a layout switch) or shown again (a pop): back to
            // the row that was at the top, after the first layout pass, which
            // a lazy stack needs before it can scroll to a row.
            .task {
                await Task.yield()
                let saved = navigation.scrollPosition(for: key)
                if let id = saved.viewID as? ID {
                    restoring = id
                    position.scrollTo(id: id, anchor: .top)
                } else {
                    if saved.edge == .top {
                        position.scrollTo(edge: .top)
                    }
                    tracking = true
                }
            }
            .onDisappear {
                tracking = false
            }
            .onChange(of: navigation.scrollPosition(for: key).edge) { _, edge in
                if edge == .top {
                    position.scrollTo(edge: .top)
                }
            }
    }
}
