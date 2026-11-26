import CoreBridge
import Testing
@testable import Stats

struct StatsDeckDirectoryTests {
    @Test func preparedMenuFollowsEachTreeSnapshotIncludingRenameMoveAndRemoval() {
        let original = StatsDeckDirectory(nodes: [node(1, "Languages", children: [node(2, "Japanese")]),
            node(3, "Ahead", filtered: true), node(4, "Science")])
        #expect(original.menu.map(\.id) == [1, 2, 4, 3])
        #expect(original.menu.map(\.depth) == [0, 1, 0, 0])
        let replacement = StatsDeckDirectory(nodes: [node(4, "Science", children: [node(2, "Vocabulary")]),
            node(3, "Ahead", filtered: true)])
        #expect(replacement.menu.map(\.id) == [4, 2, 3])
        #expect(replacement.menu.map(\.name) == ["Science", "Vocabulary", "Ahead"])
        #expect(replacement.deck(2)?.fullName == "Science::Vocabulary")
        #expect(replacement.deckCount(.deck(4)) == 2)
        #expect(replacement.isMissing(.deck(1)))
        // Retained chart jobs keep their own snapshot; publishing another directory
        // cannot mutate the menu or names associated with an older result.
        #expect(original.deck(2)?.fullName == "Languages::Japanese")
        #expect(original.menu.map(\.id) == [1, 2, 4, 3])
    }

    private func node(_ id: Int64, _ name: String, filtered: Bool = false,
                      children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: 1, learning: 0, review: 0), children: children)
    }
}
