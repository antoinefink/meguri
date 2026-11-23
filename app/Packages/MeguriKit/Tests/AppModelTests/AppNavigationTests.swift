import SwiftUI
import Testing

import AppModel

/// The navigation model above the layout switch (U150, KTD10): one state,
/// read by the tab layout and the sidebar layout alike (R2–R5, R17–R23,
/// R25).
@MainActor
@Suite struct AppNavigationTests {
    private let spanishMining = DeckID(1_700_000_000_001)
    private let japanese = DeckID(1_700_000_000_002)

    /// A stand-in for a feature's model (U175's review session, U195's Add
    /// draft): only its identity matters here.
    private final class FeatureModel {
        var revealed = false
    }

    // MARK: Structure (R17, R18)

    @Test func theCompactLayoutHasFourTabsAndNoteTypesIsNotOne() {
        // N25.1: Statistics between Decks and Browse.
        #expect(CompactTab.allCases == [.decks, .stats, .browse, .settings])
        #expect(CompactTab.allCases.map(\.section) == [.home, .stats, .browse, .settings])
        // Note Types lives in the Settings tab (R18).
        #expect(AppSection.noteTypes.tab == .settings)
    }

    @Test func theSidebarListsItsSectionsInOrder() {
        // N25.1: Statistics second, under Today.
        #expect(AppSection.sidebar == [.home, .stats, .browse, .noteTypes, .settings])
    }

    /// Statistics shows where its page has a layout (the Mac, the iPad), second under Today
    /// (N25.1, plan U307's part for those platforms); the iPhone waits for U304.
    @Test func statisticsShowsOnTheMacAndTheIPadSecondUnderToday() {
        #expect(AppSection.statisticsShown(on: .mac))
        #expect(AppSection.statisticsShown(on: .pad))
        // U304: the narrow page exists, so the iPhone and the Duo show it too.
        #expect(AppSection.statisticsShown(on: .phone))
        #expect(AppSection.productSidebar(on: .mac, statistics: true) == [.home, .stats, .browse, .noteTypes])
        #expect(AppSection.productSidebar(on: .pad, statistics: true) == [.home, .stats, .noteTypes, .settings])
        #expect(AppSection.productSidebar(on: .phone, statistics: false) == [.home, .noteTypes, .settings])
        // The flag off (`-MeguriStatsRollout off`) gives the rows Statistics had joined.
        #expect(AppSection.productSidebar(on: .mac, statistics: false) == [.home, .browse, .noteTypes])
        #expect(AppSection.productSidebar(on: .pad, statistics: false) == [.home, .noteTypes, .settings])
    }

    @Test func aNewNavigationOpensOnHomeWithTheSidebarShown() {
        let navigation = AppNavigation()
        #expect(navigation.section == .home)
        #expect(navigation.selectedDeck == nil)
        #expect(navigation.sidebarSelection == .section(.home))
        #expect(navigation.compactTab == .decks)
        #expect(navigation.columnVisibility == .all)
        #expect(navigation.review == nil)
        #expect(navigation.sheet == nil)
        #expect(CompactTab.allCases.allSatisfy { navigation.compactPath($0).isEmpty })
    }

    // MARK: Tab reselection (R19)

    @Test func reselectingBrowsePopsItsStackThenScrollsToTop() {
        let navigation = AppNavigation()
        navigation.selectTab(.browse)
        navigation.push(.noteTypes)
        navigation.setScrollPosition(ScrollPosition(id: "row-20"), for: .section(.browse))
        #expect(navigation.compactPath(.browse) == [.noteTypes])

        navigation.selectTab(.browse)
        #expect(navigation.compactPath(.browse).isEmpty)
        #expect(navigation.path(.browse).isEmpty)
        // The first reselect only pops: the root keeps its scroll position.
        #expect(navigation.scrollPosition(for: .section(.browse)) == ScrollPosition(id: "row-20"))

        navigation.selectTab(.browse)
        #expect(navigation.scrollPosition(for: .section(.browse)).edge == .top)
        #expect(navigation.section == .browse)
    }

    @Test func reselectingDecksClosesTheDeckPage() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        #expect(navigation.compactPath(.decks) == [.deckPage(spanishMining)])

        navigation.selectTab(.decks)
        #expect(navigation.compactPath(.decks).isEmpty)
        #expect(navigation.selectedDeck == nil)
        #expect(navigation.section == .home)
    }

    @Test func selectingAnotherTabKeepsEachTabsStack() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        navigation.selectTab(.browse)
        navigation.push(.noteTypes)
        navigation.selectTab(.stats)
        #expect(navigation.section == .stats)
        #expect(navigation.compactPath(.decks) == [.deckPage(spanishMining)])
        #expect(navigation.compactPath(.browse) == [.noteTypes])

        navigation.selectTab(.decks)
        #expect(navigation.section == .home)
        #expect(navigation.selectedDeck == spanishMining)
    }

    @Test func poppingTheDeckPageInTheTabLayoutClearsTheSelectedDeck() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        navigation.setCompactPath([], for: .decks)
        #expect(navigation.selectedDeck == nil)
        #expect(navigation.sidebarSelection == .section(.home))

        navigation.setCompactPath([.deckPage(japanese)], for: .decks)
        #expect(navigation.selectedDeck == japanese)
        #expect(navigation.sidebarSelection == .deck(japanese))
    }

    // MARK: Note Types in the tab layout (R18)

    @Test func noteTypesShowsInsideTheSettingsTabAndRoundTrips() {
        let navigation = AppNavigation()
        navigation.select(.section(.noteTypes))
        #expect(navigation.compactTab == .settings)
        #expect(navigation.compactPath(.settings) == [.noteTypes])

        // Back from Note Types in the Settings tab lands on Settings.
        navigation.setCompactPath([], for: .settings)
        #expect(navigation.section == .settings)
        #expect(navigation.sidebarSelection == .section(.settings))

        // Pushing Note Types from Settings makes it the current section.
        navigation.setCompactPath([.noteTypes], for: .settings)
        #expect(navigation.section == .noteTypes)
        #expect(navigation.sidebarSelection == .section(.noteTypes))
    }

    @Test func theSettingsTabReturnsToNoteTypesWhenItWasLeftThere() {
        let navigation = AppNavigation()
        navigation.select(.section(.noteTypes))
        navigation.selectTab(.decks)
        navigation.selectTab(.settings)
        #expect(navigation.section == .noteTypes)
        #expect(navigation.compactPath(.settings) == [.noteTypes])
    }

    // MARK: Layout switches (R5)

    /// Neither layout writes the state when it reads it: switching from one
    /// to the other and back changes nothing.
    @Test func bothLayoutsReadTheSameStateWithoutChangingIt() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        navigation.select(.section(.browse))
        navigation.push(.noteTypes)
        navigation.setScrollPosition(ScrollPosition(id: "row-20"), for: .section(.browse))
        let before = Snapshot(navigation)

        // Tab layout.
        #expect(navigation.compactTab == .browse)
        #expect(navigation.compactPath(.browse) == [.noteTypes])
        #expect(navigation.compactPath(.decks) == [.deckPage(spanishMining)])
        // Sidebar layout.
        #expect(navigation.sidebarSelection == .section(.browse))
        #expect(navigation.detailPath == [.noteTypes])
        #expect(Snapshot(navigation) == before)
        #expect(navigation.selectedDeck == spanishMining)
        #expect(navigation.scrollPosition(for: .section(.browse)) == ScrollPosition(id: "row-20"))
    }

    @Test func aDeckPageOpenInTheSidebarIsPushedInTheTabLayout() {
        let navigation = AppNavigation()
        navigation.select(.deck(spanishMining))
        #expect(navigation.section == .home)
        #expect(navigation.detailPath.isEmpty)
        #expect(navigation.compactPath(.decks) == [.deckPage(spanishMining)])
    }

    /// A tab-layout relaunch selects the deck without showing its page
    /// (R13): both layouts show Home, and opening a deck from there works
    /// as from any Home.
    @Test func aSelectedDeckWhosePageIsHiddenShowsHomeInBothLayouts() {
        let navigation = AppNavigation(restoring: UIState(section: .home, selectedDeck: spanishMining, sidebarVisible: true))
        navigation.adoptLaunchLayout(.compact)
        let before = Snapshot(navigation)
        #expect(navigation.selectedDeck == spanishMining)
        #expect(navigation.homeDeckPage == nil)
        #expect(navigation.compactPath(.decks).isEmpty)
        #expect(navigation.sidebarSelection == .section(.home))
        #expect(navigation.detailPath.isEmpty)
        // Reselecting Decks or selecting the Home row already shown changes nothing.
        navigation.selectTab(.decks)
        navigation.select(.section(.home))
        #expect(navigation.selectedDeck == spanishMining)
        #expect(navigation.path(.home).isEmpty)
        #expect(Snapshot(navigation).selectedDeck == before.selectedDeck)

        // Tapping a deck on Home pushes its page; going back closes it.
        navigation.setCompactPath([.deckPage(japanese)], for: .decks)
        #expect(navigation.homeDeckPage == japanese)
        #expect(navigation.sidebarSelection == .deck(japanese))
        navigation.setCompactPath([], for: .decks)
        #expect(navigation.homeDeckPage == nil)
        #expect(navigation.selectedDeck == nil)

        // The sidebar's deck row opens the page that was hidden.
        let restored = AppNavigation(restoring: UIState(section: .home, selectedDeck: spanishMining, sidebarVisible: true))
        restored.adoptLaunchLayout(.compact)
        restored.select(.deck(spanishMining))
        #expect(restored.homeDeckPage == spanishMining)
        #expect(restored.compactPath(.decks) == [.deckPage(spanishMining)])
    }

    @Test func featureStateIsHeldByTheNavigationNotByAView() {
        let navigation = AppNavigation()
        let first = navigation.featureState { FeatureModel() }
        first.revealed = true
        let second = navigation.featureState { FeatureModel() }
        #expect(first === second)
        #expect(second.revealed)
    }

    @Test func changingTheSelectedDeckStartsAFreshStackOnIt() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        navigation.push(.noteTypes)
        #expect(navigation.detailPath == [.noteTypes])
        navigation.openDeckPage(japanese)
        #expect(navigation.detailPath.isEmpty)
        #expect(navigation.compactPath(.decks) == [.deckPage(japanese)])
    }

    // MARK: Review (R21–R23)

    @Test func aReviewHidesTheSidebarAndItsEndRestoresIt() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        let session = FeatureModel()
        navigation.startReview(ReviewPresentation(deck: spanishMining, session: session))
        #expect(navigation.columnVisibility == .detailOnly)
        #expect(navigation.review?.session === session)

        // The user may reveal the sidebar during the review (R21).
        navigation.columnVisibility = .all
        navigation.columnVisibility = .detailOnly
        navigation.endReview()
        #expect(navigation.review == nil)
        #expect(navigation.columnVisibility == .all)
    }

    @Test func aReviewEndRestoresAHiddenSidebarHidden() {
        let navigation = AppNavigation()
        navigation.columnVisibility = .detailOnly
        navigation.startReview(ReviewPresentation(deck: spanishMining, session: FeatureModel()))
        navigation.columnVisibility = .all
        navigation.endReview()
        #expect(navigation.columnVisibility == .detailOnly)
    }

    @Test func selectingAnotherSidebarRowDuringAReviewEndsItAndNavigates() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        navigation.startReview(ReviewPresentation(deck: spanishMining, session: FeatureModel()))

        // The row that is already selected changes nothing.
        navigation.select(.deck(spanishMining))
        #expect(navigation.review != nil)

        navigation.select(.section(.stats))
        #expect(navigation.review == nil)
        #expect(navigation.section == .stats)
        #expect(navigation.columnVisibility == .all)

        navigation.startReview(ReviewPresentation(deck: japanese, session: FeatureModel()))
        navigation.select(.deck(spanishMining))
        #expect(navigation.review == nil)
        #expect(navigation.sidebarSelection == .deck(spanishMining))
    }

    @Test func theReviewSessionSurvivesAsLongAsThePresentation() {
        let navigation = AppNavigation()
        let session = FeatureModel()
        navigation.startReview(ReviewPresentation(deck: spanishMining, session: session))
        session.revealed = true
        // Whatever a layout does with its views, the session is the
        // navigation's: the same object, in the same state.
        #expect(navigation.review?.session === session)
        #expect((navigation.review?.session as? FeatureModel)?.revealed == true)
    }

    // MARK: Sheets (R25)

    @Test func serverWorkflowKeepsItsHostAcrossCollectionAdoptionUntilExplicitlyClosed() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        navigation.push(.noteTypes)
        navigation.present(.manualTransfer)
        navigation.sheetHost = .settings

        navigation.collectionReplaced()

        #expect(navigation.sheet == .manualTransfer)
        #expect(navigation.sheetHost == .settings)
        #expect(navigation.selectedDeck == nil)
        #expect(navigation.detailPath.isEmpty)
        navigation.dismissSheet()
        #expect(navigation.sheet == nil)
    }

    @Test func addOpensOverTheDeckPageAndClosingItReturnsToIt() {
        let navigation = AppNavigation()
        navigation.openDeckPage(spanishMining)
        navigation.setScrollPosition(ScrollPosition(id: "row-20"), for: .deckPage)
        let before = Snapshot(navigation)

        navigation.present(.add(deck: spanishMining))
        #expect(navigation.sheet == .add(deck: spanishMining))
        navigation.dismissSheet()
        #expect(navigation.sheet == nil)
        #expect(Snapshot(navigation) == before)
        #expect(navigation.sidebarSelection == .deck(spanishMining))
        #expect(navigation.scrollPosition(for: .deckPage) == ScrollPosition(id: "row-20"))
    }

    /// The navigation state that layout switches and sheets must leave alone.
    private struct Snapshot: Equatable {
        let section: AppSection
        let selectedDeck: DeckID?
        let paths: [AppSection: [Destination]]
        let scroll: [ScrollAnchorKey: ScrollPosition]
        let columns: NavigationSplitViewVisibility

        init(_ navigation: AppNavigation) {
            section = navigation.section
            selectedDeck = navigation.selectedDeck
            paths = Dictionary(uniqueKeysWithValues: AppSection.allCases.map { ($0, navigation.path($0)) })
            scroll = Dictionary(uniqueKeysWithValues: ([.deckPage] + AppSection.allCases.map { .section($0) }).map {
                ($0, navigation.scrollPosition(for: $0))
            })
            columns = navigation.columnVisibility
        }
    }
}
