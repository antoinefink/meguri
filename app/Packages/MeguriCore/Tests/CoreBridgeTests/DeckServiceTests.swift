import Testing
import MeguriCoreBindings
@testable import CoreBridge

@Suite struct DeckServiceTests {
    @Test func countedTreePreservesNestedOrderingCountsAndRefreshBoundary() {
        let raw = MeguriCoreBindings.CountedDeckTree(generation: 29, nextChangeAt: 1_894_766_400, nodes: [
            .init(id: -1, name: "alpha", filtered: false, studyCollapsed: true, browserCollapsed: false,
                  counts: .init(new: 13, learning: 17, review: 19), children: [
                    .init(id: 7, name: "Child", filtered: true, studyCollapsed: false, browserCollapsed: true,
                          counts: .init(new: 2, learning: 3, review: 5), children: [])]),
            .init(id: 9, name: "Zoo", filtered: true, studyCollapsed: false, browserCollapsed: false,
                  counts: .init(new: 23, learning: 29, review: 31), children: []),
        ])
        #expect(CoreBridge.CountedDeckTree(raw) == CoreBridge.CountedDeckTree(generation: 29, nextChangeAt: 1_894_766_400, nodes: [
            .init(id: -1, name: "alpha", filtered: false, studyCollapsed: true, browserCollapsed: false,
                  counts: .init(new: 13, learning: 17, review: 19), children: [
                    .init(id: 7, name: "Child", filtered: true, studyCollapsed: false, browserCollapsed: true,
                          counts: .init(new: 2, learning: 3, review: 5), children: [])]),
            .init(id: 9, name: "Zoo", filtered: true, studyCollapsed: false, browserCollapsed: false,
                  counts: .init(new: 23, learning: 29, review: 31), children: []),
        ]))
    }
}
