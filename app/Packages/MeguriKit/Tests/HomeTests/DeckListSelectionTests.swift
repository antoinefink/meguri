import Testing
@testable import AppModel
@testable import CoreBridge
@testable import Home

@MainActor @Suite struct DeckListSelectionTests {
    private func node(_ id: Int64, _ name: String, _ children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: false, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: 1, learning: 0, review: 2), children: children)
    }

    @Test func todayIsMarkedUntilADeckPageShowsAndAgainAfterGoingHome() {
        let navigation = AppNavigation()
        #expect(DeckListSelection(homeDeckPage: navigation.homeDeckPage).today)

        navigation.openDeckPage(DeckID(3))
        let picked = DeckListSelection(homeDeckPage: navigation.homeDeckPage)
        #expect(!picked.today)
        #expect(picked.deck == 3)
        #expect(picked.marks(node(3, "Reading"), expanded: true))
        #expect(!picked.marks(node(4, "Science"), expanded: true))

        // The Today row's action.
        navigation.openHome()
        let home = DeckListSelection(homeDeckPage: navigation.homeDeckPage)
        #expect(home.today)
        #expect(!home.marks(node(3, "Reading"), expanded: true))
    }

    @Test func aCollapsedParentStandsForTheSubdeckItHides() {
        let leaf = node(5, "Languages::Japanese::Grammar")
        let middle = node(4, "Languages::Japanese", [leaf])
        let top = node(1, "Languages", [middle, node(6, "Languages::Spanish")])
        let selection = DeckListSelection(deck: 5)
        // Open down to the deck: only its own row.
        #expect(!selection.marks(top, expanded: true))
        #expect(!selection.marks(middle, expanded: true))
        #expect(selection.marks(leaf, expanded: true))
        // Collapsed above it at either level: the row that hides it.
        #expect(selection.marks(middle, expanded: false))
        #expect(selection.marks(top, expanded: false))
        // A collapsed deck elsewhere marks nothing, and Today marks no deck.
        #expect(!selection.marks(node(7, "Reading", [node(8, "Reading::News")]), expanded: false))
        #expect(!DeckListSelection(deck: nil).marks(top, expanded: false))
    }
}
