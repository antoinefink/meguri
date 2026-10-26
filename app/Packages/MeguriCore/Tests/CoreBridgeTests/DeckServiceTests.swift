import Testing
import MeguriCoreBindings
@testable import CoreBridge

@Suite struct DeckServiceTests {
    @Test func deckNameSnapshotPreservesSignedIDsGenerationSpellingAndOrder() {
        let raw = MeguriCoreBindings.DeckNameSnapshot(generation: .max, decks: [
            .init(id: .min, fullName: " E\u{301} :: :猫: ", filtered: true),
            .init(id: .max, fullName: "alpha::É::東京", filtered: false),
            .init(id: 1, fullName: "Personal", filtered: false),
        ])
        let value = CoreBridge.DeckNameSnapshot(raw)
        #expect(value == CoreBridge.DeckNameSnapshot(generation: .max, decks: [
            .init(id: .min, fullName: " E\u{301} :: :猫: ", filtered: true),
            .init(id: .max, fullName: "alpha::É::東京", filtered: false),
            .init(id: 1, fullName: "Personal", filtered: false),
        ]))
        #expect(value.decks[0].fullName.utf8.elementsEqual(" E\u{301} :: :猫: ".utf8))
    }
    @Test func committingProgressDisablesCancellationWithoutInventingCounts() {
        let value = CoreBridge.CoreProgress(.init(jobId: 37, phase: .committing, etaMillis: nil, completed: 0, total: nil))
        #expect(value.phase == .committing && !value.cancellable)
        #expect(value.job.rawValue == 37 && value.completed == 0 && value.total == nil)
    }
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

#if os(macOS) && arch(arm64)
private extension QueryLane {
    func measureCountedTreeDecode() throws {
        try call("test.countedTreeDecode") {
            // Exactly 2,000 nodes in one tree record, including its root.
            var children: [MeguriCoreBindings.CountedDeckNode] = []
            children.reserveCapacity(1_999)
            for id: Int in 2...2_000 {
                let counts = MeguriCoreBindings.ReviewCounts(new: UInt32(id), learning: UInt32(id + 1), review: UInt32(id + 2))
                let child = MeguriCoreBindings.CountedDeckNode(id: Int64(id), name: "Deck \(id)", filtered: id.isMultiple(of: 3),
                    studyCollapsed: id.isMultiple(of: 2), browserCollapsed: id.isMultiple(of: 5),
                    counts: counts, children: [])
                children.append(child)
            }
            let raw = MeguriCoreBindings.CountedDeckTree(generation: 37, nextChangeAt: 1_894_766_400, nodes: [
                .init(id: 1, name: "Root", filtered: false, studyCollapsed: false, browserCollapsed: true,
                      counts: .init(new: 7, learning: 11, review: 13), children: children)
            ])
            let expected = CoreBridge.CountedDeckTree(raw)
            // Lower all ten fresh buffers before timing any sample. Lift
            // consumes and frees each; no query, seed, build or reuse is timed.
            let buffers = (1...10).map { _ in FfiConverterTypeCountedDeckTree_lower(raw) }
            #if DEBUG
            print("Debug checks counted-tree decoding and values. R1802 timing uses swift-test-core-profile.")
            #endif
            let clock = ContinuousClock()
            for (index, buffer) in buffers.enumerated() {
                let sample = index + 1
                let started = clock.now
                let value = CoreBridge.CountedDeckTree(try FfiConverterTypeCountedDeckTree_lift(buffer))
                let elapsed = clock.now - started
                let milliseconds = Double(elapsed.components.seconds) * 1_000
                    + Double(elapsed.components.attoseconds) / 1e15
                print("U37 native counted tree sample \(sample): \(milliseconds) ms (2000 nodes)")
                #if !DEBUG
                #expect(elapsed < .milliseconds(5))
                #endif
                #expect(value == expected)
                #expect(value.nodes.count == 1)
                #expect(value.nodes[0].id == 1 && value.nodes[0].children.count == 1_999)
                #expect(value.nodes[0].children.map(\.id) == (2...2_000).map { Int64($0) })
                #expect(value.nodes[0].children.allSatisfy { $0.children.isEmpty })
            }
        }
    }
}
extension DeckServiceTests {
    @Test func nativeGeneratedCountedTreeLiftAndValueConversionUnderFiveMilliseconds() async throws {
        try await QueryLane.shared.measureCountedTreeDecode()
    }
}
#endif
