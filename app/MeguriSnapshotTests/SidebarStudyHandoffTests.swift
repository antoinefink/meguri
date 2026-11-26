#if os(macOS)
import AppModel
import Testing
@testable import Meguri

/// Exercise the Return action used by the guarded window monitor without a window.
@MainActor @Suite struct SidebarStudyHandoffTests {
    @Test(arguments: [SidebarItem.deck(DeckID(2)), .section(.home)])
    func returnConsumesHeldSelectionAndStudiesItsTargetAfterCommit(item: SidebarItem) throws {
        let navigation = AppNavigation()
        navigation.openDeckPage(DeckID(1))
        var studied: [SidebarItem] = []
        var features = ShellFeatures.standard
        features.sidebarStudy = { target in
            #expect(!navigation.sidebarInteractionPending)
            #expect(navigation.sidebarSelection == target)
            studied.append(target)
            return true
        }
        let sidebar = SidebarView(navigation: navigation, features: features)
        navigation.selectFromSidebar(item)
        let intent = try #require(navigation.macSidebarSelectionID)
        #expect(sidebar.studyOnReturn())
        #expect(studied.isEmpty)
        #expect(navigation.homeDeckPage == DeckID(1))
        navigation.commitMacSidebarSelection(intent)
        #expect(studied == [item])
        #expect(navigation.homeDeckPage == (item == .section(.home) ? nil : DeckID(2)))
        navigation.commitMacSidebarSelection(intent)
        #expect(studied == [item])
    }

    @Test(arguments: [SidebarItem.deck(DeckID(2)), .section(.home)], [false, true])
    func cancelledOrSupersededReturnDoesNotStudy(item: SidebarItem, superseded: Bool) throws {
        let navigation = AppNavigation()
        navigation.openDeckPage(DeckID(1))
        var studies = 0
        var features = ShellFeatures.standard
        features.sidebarStudy = { _ in studies += 1; return true }
        let sidebar = SidebarView(navigation: navigation, features: features)
        navigation.selectFromSidebar(item)
        let intent = try #require(navigation.macSidebarSelectionID)
        #expect(sidebar.studyOnReturn())
        if superseded { navigation.selectFromSidebar(.section(.browse)) }
        else { navigation.cancelMacSidebarSelection() }
        navigation.commitMacSidebarSelection(intent)
        if let latest = navigation.macSidebarSelectionID { navigation.commitMacSidebarSelection(latest) }
        #expect(studies == 0)
        #expect(navigation.sidebarSelection == (superseded ? .section(.browse) : .deck(DeckID(1))))
    }

    @Test func unrelatedHeldSectionKeepsReturnAndCommittedSelectionUsesExistingAction() throws {
        let navigation = AppNavigation()
        var studied: [SidebarItem] = []
        var features = ShellFeatures.standard
        features.sidebarStudy = { item in studied.append(item); return item == .section(.home) }
        let sidebar = SidebarView(navigation: navigation, features: features)
        navigation.selectFromSidebar(.section(.browse))
        let intent = try #require(navigation.macSidebarSelectionID)
        #expect(!sidebar.studyOnReturn())
        navigation.commitMacSidebarSelection(intent)
        #expect(studied.isEmpty)
        #expect(!sidebar.studyOnReturn())
        #expect(studied == [.section(.browse)])
        navigation.select(.section(.home))
        #expect(sidebar.studyOnReturn())
        #expect(studied == [.section(.browse), .section(.home)])
    }
}
#endif
