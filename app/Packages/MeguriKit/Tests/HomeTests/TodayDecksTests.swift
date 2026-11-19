import Testing
@testable import CoreBridge
@testable import Home

@Suite struct TodayDecksTests {
    private func node(_ id: Int64, _ name: String, _ counts: (UInt32, UInt32, UInt32), _ children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: false, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: counts.0, learning: counts.1, review: counts.2), children: children)
    }

    @Test func listsTheDeepestDecksWithCardsWaiting() {
        let tree = [
            node(1, "Languages", (15, 1, 30), [
                node(2, "Languages::Español", (10, 1, 18)),
                node(3, "Languages::日本語", (5, 0, 12), [
                    node(4, "Languages::日本語::文法", (5, 0, 12)),
                    node(5, "Languages::日本語::語彙", (0, 0, 0)),
                ]),
            ]),
            node(6, "Reading", (0, 0, 0)),
            // A parent whose own cards wait while its subdecks have none is listed itself.
            node(7, "Science", (4, 0, 3), [node(8, "Science::Chemistry", (0, 0, 0))]),
        ]
        let today = TodayDecks(nodes: tree)
        #expect(today.entries.map(\.id) == [2, 4, 7])
        #expect(today.entries.map(\.name) == ["Español", "文法", "Science"])
        #expect(today.entries.map(\.ancestors) == [["Languages"], ["Languages", "日本語"], []])
        #expect(today.entries[1].path == "Languages::日本語::文法")
        // Totals come from the top-level decks, which already hold their subdecks' counts.
        #expect(today.totals == ReviewCounts(new: 19, learning: 1, review: 33))
    }

    @Test func aParentWithItsOwnCardsIsListedBeforeItsWaitingSubdecks() {
        // The parent holds 3 new and 2 review cards of its own on top of its subdeck's.
        let tree = [node(1, "Languages", (13, 1, 20), [
            node(2, "Languages::Español", (10, 1, 18)),
            node(3, "Languages::Latin", (0, 0, 0)),
        ])]
        let today = TodayDecks(nodes: tree)
        #expect(today.entries.map(\.id) == [1, 2])
        #expect(today.entries.map(\.ancestors) == [[], ["Languages"]])
        // Its row shows what Study on it reaches: its own cards and its subdecks'.
        #expect(today.entries[0].counts == ReviewCounts(new: 13, learning: 1, review: 20))
        #expect(today.totals == ReviewCounts(new: 13, learning: 1, review: 20))
    }

    @Test func aParentWhoseSubdecksHoldAllItsCardsIsNotListed() {
        let tree = [node(1, "Languages", (10, 1, 18), [node(2, "Languages::Español", (10, 1, 18))])]
        #expect(TodayDecks(nodes: tree).entries.map(\.id) == [2])
        // Daily limits can make a parent's counts smaller than its subdecks' sum: still only the subdecks.
        let limited = [node(1, "Languages", (5, 1, 18), [node(2, "Languages::Español", (10, 1, 18))])]
        #expect(TodayDecks(nodes: limited).entries.map(\.id) == [2])
    }

    @Test func aFilteredDeckWithCardsWaitingIsListedLikeAnyOther() {
        let filtered = CountedDeckNode(id: 9, name: "Cram", filtered: true, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: 0, learning: 2, review: 7), children: [])
        let today = TodayDecks(nodes: [node(1, "Reading", (0, 0, 0)), filtered])
        #expect(today.entries.map(\.id) == [9])
        #expect(today.totals == ReviewCounts(new: 0, learning: 2, review: 7))
    }

    @Test func nothingWaitingListsNothing() {
        let today = TodayDecks(nodes: [node(1, "Default", (0, 0, 0))])
        #expect(today.entries.isEmpty)
        #expect(today.totals == ReviewCounts(new: 0, learning: 0, review: 0))
    }
}
