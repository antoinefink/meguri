import CoreBridge
import Testing
@testable import Home

/// N20.1 and `decks/deck-list` H6.3: the sidebar search's rows, highlight and
/// arrows, and D18's create row.
@MainActor
@Suite struct DeckSearchHighlightTests {
    private func node(_ id: Int64, _ name: String, _ children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: false, studyCollapsed: true, browserCollapsed: true,
            counts: ReviewCounts(new: 0, learning: 0, review: 0), children: children)
    }
    private var tree: [CountedDeckNode] {
        [node(1, "Languages", [node(2, "日本語", [node(3, "文法")]), node(4, "Español")]), node(5, "Science")]
    }

    @Test func resultsReadAsPathsWithChevrons() {
        let search = DeckSearch(nodes: tree, query: "文法")
        #expect(search.matches.map(\.id) == [3])
        #expect(search.matches.first?.ancestors == ["Languages", "日本語"])
        #expect(search.matches.first?.spokenPath == "Languages › 日本語 › 文法")
    }

    @Test func theFirstRowIsHighlightedOrTheCreateRowWhenNothingIsListed() {
        let listed = DeckSearch(nodes: tree, query: "a").targets(offeringCreate: true)
        #expect(listed == [.deck(1), .deck(2), .deck(3), .deck(4), .create])
        #expect(DeckSearch.highlight(nil, in: listed) == .deck(1))
        let nothing = DeckSearch(nodes: tree, query: "Korean").targets(offeringCreate: true)
        #expect(nothing == [.create])
        #expect(DeckSearch.highlight(nil, in: nothing) == .create)
        #expect(DeckSearch.highlight(nil, in: []) == nil)
        // A choice that is no longer listed falls back to the first row.
        #expect(DeckSearch.highlight(.deck(99), in: [.deck(2), .create]) == .deck(2))
        #expect(DeckSearch.highlight(.create, in: [.deck(2), .create]) == .create)
    }

    @Test func arrowsMoveOverAllRowsAndWrap() {
        let rows: [DeckSearchTarget] = [.deck(1), .deck(2), .create]
        #expect(DeckSearch.move(.deck(1), by: 1, in: rows) == .deck(2))
        #expect(DeckSearch.move(.deck(2), by: 1, in: rows) == .create)
        #expect(DeckSearch.move(.create, by: 1, in: rows) == .deck(1))
        #expect(DeckSearch.move(.deck(1), by: -1, in: rows) == .create)
        #expect(DeckSearch.move(nil, by: 1, in: rows) == .deck(1))
        #expect(DeckSearch.move(nil, by: -1, in: rows) == .create)
        #expect(DeckSearch.move(.deck(1), by: 1, in: []) == nil)
    }

    @Test func anEditHighlightsTheFirstRowAgain() {
        let search = DeckListSearch()
        search.text = "a"
        search.chosen = .deck(4)
        search.text = "ab"
        #expect(search.chosen == nil)
    }

    @Test func createIsOfferedOnlyAfterACompletedReadForThisTextAndGeneration() async {
        let search = DeckListSearch()
        search.text = "Korean"
        #expect(!search.offersCreate(generation: 11), "No read yet")
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 11, deck: nil) }
        #expect(search.offersCreate(generation: 11))
        #expect(!search.offersCreate(generation: 12), "Another generation needs its own read")
        search.text = "Korean "
        #expect(!search.offersCreate(generation: 11), "Another text needs its own read")

        // An exact match, a read of another generation and a failed read never offer it.
        search.text = "languages"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in
            DeckNameMatch(generation: 11, deck: DeckNameEntry(id: 1, fullName: "Languages", filtered: false))
        }
        #expect(search.nameCheck(generation: 11) == .exists && !search.offersCreate(generation: 11))
        search.text = "Korean"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 12, deck: nil) }
        #expect(search.nameCheck(generation: 11) == .failed && !search.offersCreate(generation: 11))
        search.retryNameCheck()
        #expect(search.nameCheck(generation: 11) == nil, "Try Again starts a new read")
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in throw BridgeError.cancelled }
        #expect(search.nameCheck(generation: 11) == .failed)
    }

    @Test func aBlankTextIsNeverOffered() async {
        let search = DeckListSearch()
        for blank in ["", "  ", "\u{3000}"] {
            search.text = blank
            await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 11, deck: nil) }
            #expect(!search.offersCreate(generation: 11))
        }
        search.text = "::"
        await search.checkName(search.nameCheckKey(generation: 11)) { _ in DeckNameMatch(generation: 11, deck: nil) }
        #expect(search.offersCreate(generation: 11), "\"::\" is not blank (R58)")
    }
}
