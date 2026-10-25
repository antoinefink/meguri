import CoreBridge
import Foundation
import Testing
@testable import AppModel

// Shares the existing serialized real-collection suite and generated package fixture.
extension ReviewCollectionStartupTests {
    @Test func savedDeckSelectionAndNestedStudyDisclosureSurviveRealCollectionCloseReopen() async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: root.appending(path: "Collection"))
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        var owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let decks = CoreDeckService()
        do {
            await owner.start()
            let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            try #require(owner.state == .ready)

            let aID = try await decks.addDeck(name: "U152 A").id
            let bID = try await decks.addDeck(name: "U152 A::B").id
            let cID = try await decks.addDeck(name: "U152 A::B::C").id
            _ = try await decks.collapseDecks(ids: [aID, bID], scope: .browser, collapsed: false)
            await owner.loadCounts()
            try #require(!owner.studyExpanded(deckID: aID))
            try #require(!owner.studyExpanded(deckID: bID))
            try #require(await owner.openDeckPage(deckID: bID).value)
            let unchanged = try await decks.selectDeck(id: bID)
            #expect(unchanged.changes.configKeys.isEmpty)
            #expect(unchanged.undoName == nil)

            await owner.setStudyExpanded(deckID: aID, expanded: true, includingDescendants: true).value
            await owner.setStudyExpanded(deckID: bID, expanded: false).value
            await owner.loadCounts()
            #expect(owner.navigation.homeDeckPage == DeckID(bID))
            let aBeforeClose = try #require(owner.deck(aID))
            let bBeforeClose = try #require(owner.deck(bID))
            #expect(aBeforeClose.children.map(\.id) == [bID])
            #expect(bBeforeClose.children.map(\.id) == [cID])
            try #require(await owner.close())

            // Reopen with a fresh owner and no UIState/defaults store.
            owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
            await owner.start()
            try #require(owner.state == .ready)
            let reopenedA = try #require(owner.deck(aID))
            let reopenedB = try #require(owner.deck(bID))
            let reopenedC = try #require(owner.deck(cID))
            #expect(reopenedA.children.map(\.id) == [bID])
            #expect(reopenedB.children.map(\.id) == [cID])
            #expect(owner.studyExpanded(deckID: aID))
            #expect(!owner.studyExpanded(deckID: bID))
            #expect(owner.studyExpanded(deckID: cID))
            #expect(!reopenedA.browserCollapsed)
            #expect(!reopenedB.browserCollapsed)
            #expect(reopenedC.browserCollapsed)
            // With no public current-deck getter, a core no-op proves the saved selection.
            let durableSelection = try await decks.selectDeck(id: bID)
            #expect(durableSelection.changes.configKeys.isEmpty)
            #expect(durableSelection.undoName == nil)

            await owner.setStudyExpanded(deckID: bID, expanded: true).value
            await owner.loadCounts()
            #expect(owner.studyExpanded(deckID: cID))
            let expandedB = try #require(owner.deck(bID))
            #expect(!expandedB.browserCollapsed)
            try #require(await owner.close())
        } catch { _ = await owner.close(); throw error }
    }
}
