import Foundation
import SwiftUI
import Testing

import AppModel

/// The device's UI state (R12, R13, R20; KTD12): section, selected deck and
/// sidebar visibility, kept in the app's own defaults and restored at
/// launch. A relaunch never lands inside a review.
@MainActor
@Suite struct UIStateStoreTests {
    private let deck = DeckID(1_700_000_000_001)

    /// A defaults domain of its own, removed after the test.
    private final class ScratchDefaults {
        let name = "meguri.tests.uiState.\(UUID().uuidString)"
        let defaults: UserDefaults

        init() throws {
            defaults = try #require(UserDefaults(suiteName: name))
        }

        deinit {
            UserDefaults.standard.removePersistentDomain(forName: name)
        }
    }

    private final class Session {}

    /// Quits (saves) and relaunches (restores) through `store`.
    private func relaunch(_ navigation: AppNavigation, through store: UIStateStore) -> AppNavigation {
        store.save(navigation.uiState)
        return store.load().map(AppNavigation.init(restoring:)) ?? AppNavigation()
    }

    @Test func theStateRoundTrips() throws {
        let scratch = try ScratchDefaults()
        let store = UIStateStore(defaults: scratch.defaults)
        #expect(store.load() == nil)
        let state = UIState(section: .stats, selectedDeck: deck, sidebarVisible: false)
        store.save(state)
        #expect(store.load() == state)
        // A second store on the same defaults sees it: it is on disk, not in memory.
        #expect(UIStateStore(defaults: scratch.defaults).load() == state)
    }

    @Test func togglingTheSidebarIsRememberedAcrossARelaunch() throws {
        let scratch = try ScratchDefaults()
        let store = UIStateStore(defaults: scratch.defaults)
        let navigation = AppNavigation()
        #expect(navigation.columnVisibility == .all)

        // ⌃⌘S (the system's sidebar command) hides it.
        navigation.columnVisibility = .detailOnly
        let relaunched = relaunch(navigation, through: store)
        #expect(relaunched.columnVisibility == .detailOnly)

        relaunched.columnVisibility = .all
        #expect(relaunch(relaunched, through: store).columnVisibility == .all)
    }

    @Test func aReviewsAutomaticCollapseIsNotRememberedAsTheUsersChoice() throws {
        let scratch = try ScratchDefaults()
        let store = UIStateStore(defaults: scratch.defaults)
        let navigation = AppNavigation()
        navigation.startReview(ReviewPresentation(deck: deck, session: Session()))
        #expect(navigation.columnVisibility == .detailOnly)
        #expect(navigation.uiState.sidebarVisible)
        #expect(relaunch(navigation, through: store).columnVisibility == .all)
    }

    @Test func aRelaunchFromADeckPageOpensItInTheSidebarLayoutAndHomeInTheTabLayout() throws {
        let scratch = try ScratchDefaults()
        let store = UIStateStore(defaults: scratch.defaults)
        let navigation = AppNavigation()
        navigation.openDeckPage(deck)

        let sidebar = relaunch(navigation, through: store)
        sidebar.adoptLaunchLayout(.sidebar)
        #expect(sidebar.section == .home)
        #expect(sidebar.sidebarSelection == .deck(deck))

        let compact = relaunch(navigation, through: store)
        compact.adoptLaunchLayout(.compact)
        #expect(compact.section == .home)
        #expect(compact.compactTab == .decks)
        #expect(compact.compactPath(.decks).isEmpty)
        #expect(compact.selectedDeck == nil)
    }

    @Test func aRelaunchRestoresTheLastSectionInBothLayouts() throws {
        let scratch = try ScratchDefaults()
        let store = UIStateStore(defaults: scratch.defaults)
        let navigation = AppNavigation()
        navigation.openDeckPage(deck)
        navigation.select(.section(.noteTypes))

        let compact = relaunch(navigation, through: store)
        compact.adoptLaunchLayout(.compact)
        #expect(compact.section == .noteTypes)
        #expect(compact.compactPath(.settings) == [.noteTypes])

        let sidebar = relaunch(navigation, through: store)
        sidebar.adoptLaunchLayout(.sidebar)
        #expect(sidebar.sidebarSelection == .section(.noteTypes))
    }

    @Test func quittingMidReviewDoesNotRelaunchIntoTheReview() throws {
        let scratch = try ScratchDefaults()
        let store = UIStateStore(defaults: scratch.defaults)
        let navigation = AppNavigation()
        navigation.openDeckPage(deck)
        navigation.startReview(ReviewPresentation(deck: deck, session: Session()))

        let relaunched = relaunch(navigation, through: store)
        relaunched.adoptLaunchLayout(.sidebar)
        #expect(relaunched.review == nil)
        #expect(relaunched.sidebarSelection == .deck(deck))
        #expect(relaunched.columnVisibility == .all)
    }

    /// Only the launch layout decides: a state set after launch (a route, a
    /// deep link) is never dropped by a later layout change.
    @Test func onlyTheFirstLayoutAfterARestoreDropsTheDeckPage() throws {
        let navigation = AppNavigation(restoring: UIState(section: .home, selectedDeck: deck, sidebarVisible: true))
        navigation.adoptLaunchLayout(.sidebar)
        navigation.adoptLaunchLayout(.compact)
        #expect(navigation.selectedDeck == deck)

        let fresh = AppNavigation()
        fresh.openDeckPage(deck)
        fresh.adoptLaunchLayout(.compact)
        #expect(fresh.selectedDeck == deck)
    }

    @Test func aStoreWithoutDefaultsKeepsNothing() {
        let store = UIStateStore(defaults: nil)
        store.save(UIState(section: .browse, selectedDeck: nil, sidebarVisible: true))
        #expect(store.load() == nil)
    }

    @Test func unreadableStateIsIgnored() throws {
        let scratch = try ScratchDefaults()
        scratch.defaults.set(Data("not json".utf8), forKey: UIStateStore.key)
        #expect(UIStateStore(defaults: scratch.defaults).load() == nil)
        scratch.defaults.set(Data(#"{"section":"review","sidebarVisible":true}"#.utf8), forKey: UIStateStore.key)
        #expect(UIStateStore(defaults: scratch.defaults).load() == nil)
    }
}
