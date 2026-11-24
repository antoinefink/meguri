import CoreBridge
import Foundation
import Observation
import Synchronization
import Testing
@testable import AppModel

/// Synthetic rows through the collection getter used by Home and Browse. No
/// collection is opened, no FFI is called, and every async operation completes
/// locally without a held continuation or a polling loop.
@MainActor
@Suite struct StudyDisclosureTests {
    @Test(.timeLimit(.minutes(1)))
    func thousandKnownParentGettersDoNotReadTheTree() async {
        let store = Store(parents: 1_000)
        let owner = makeOwner(store)
        await owner.loadCounts()
        let rows = owner.homeDecks
        let knownChanges = Mutex(0), lookupChanges = Mutex(0)
        var evaluations = 0
        withObservationTracking {
            for row in rows {
                #expect(!owner.studyExpanded(row))
                evaluations += 1
            }
        } onChange: { knownChanges.withLock { $0 += 1 } }
        // Positive control: ID-only resolution really does depend on the tree.
        withObservationTracking {
            #expect(!owner.studyExpanded(deckID: 1_999))
        } onChange: { lookupChanges.withLock { $0 += 1 } }
        #expect(evaluations == 1_000)
        #expect(rows.count + rows.reduce(0) { $0 + $1.children.count } == 2_000)

        store.collapsed.removeAll()
        await owner.loadCounts()
        #expect(lookupChanges.withLock { $0 } == 1)
        // A search of decks/homeDecks would subscribe to this replacement.
        #expect(knownChanges.withLock { $0 } == 0)
        #expect(owner.homeDecks.map(\.id) == rows.map(\.id))
        #expect(owner.homeDecks.allSatisfy { owner.studyExpanded($0) })
    }

    @Test(.timeLimit(.minutes(1)))
    func knownRowsFollowKeptThenCurrentFlagsForTheSameIDs() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let location = AppModel.CollectionLocation(directory: directory)
        // Only file identity is inspected by snapshot loading; this is no database.
        try Data("synthetic file identity".utf8).write(to: location.collectionFile)
        let identity = try #require(CollectionFileIdentity.of(location.collectionFile))
        let store = Store(parents: 1)
        let snapshotFile = directory.appending(path: "home-snapshot.json")
        let snapshots = HomeSnapshotStore(file: snapshotFile)
        snapshots.save(HomeSnapshot(collection: identity, dayEndsAt: nil, decks: store.nodes,
            studied: nil, nextLearningReadyAt: nil, previous: nil, previousAnswers: nil))
        snapshots.flush()
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = location
        configuration.homeSnapshotFile = snapshotFile
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        defer { owner.flushHomeSnapshot() }
        owner.loadDeckTree = { store.tree }
        #expect(owner.homePhase == .kept)
        let keptParent = try #require(owner.homeDecks.first)
        #expect(!owner.studyExpanded(keptParent))
        #expect(owner.studyExpanded(keptParent.children[0]))
        #expect(keptParent.browserCollapsed == false)
        #expect(keptParent.children[0].browserCollapsed == true)

        owner.receiveLifecycle(.ready(generation: 1))
        #expect(owner.homePhase == .kept)
        store.collapsed = [2] // Same IDs, opposite study flags in the first read.
        await owner.loadCounts()
        #expect(owner.homePhase == .decks)
        let currentParent = try #require(owner.homeDecks.first)
        #expect(currentParent.id == keptParent.id)
        #expect(currentParent.children.map(\.id) == keptParent.children.map(\.id))
        #expect(owner.studyExpanded(currentParent))
        #expect(!owner.studyExpanded(currentParent.children[0]))
        // The node seam uses the caller's chosen row, never re-resolves its ID.
        #expect(!owner.studyExpanded(keptParent))
        #expect(owner.studyExpanded(deckID: keptParent.id))
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true], [false, true])
    func pendingDescendantExpansionCommitsOrRollsBack(expanded: Bool, fails: Bool) async throws {
        let store = Store(parents: 1)
        store.collapsed = expanded ? [1, 2] : []
        store.fails = fails
        let owner = makeOwner(store)
        await owner.loadCounts()
        let parent = try #require(owner.homeDecks.first)
        let changed = Mutex(false)
        withObservationTracking {
            #expect(owner.studyExpanded(parent) == !expanded)
        } onChange: { changed.withLock { $0 = true } }
        let changing = owner.setStudyExpanded(deckID: 1, expanded: expanded, includingDescendants: true)
        #expect(changed.withLock { $0 }) // Optimistic changes still invalidate rows.
        #expect(owner.studyExpanded(parent) == expanded)
        #expect(owner.studyExpanded(parent.children[0]) == expanded)
        #expect(owner.isStudyExpansionPending(deckID: 1))
        await changing.value
        await owner.loadCounts()
        let after = try #require(owner.homeDecks.first)
        #expect(owner.studyExpanded(after) == (fails ? !expanded : expanded))
        #expect(owner.studyExpanded(after.children[0]) == (fails ? !expanded : expanded))
        #expect(!owner.isStudyExpansionPending(deckID: 1))
        #expect(after.browserCollapsed == parent.browserCollapsed)
        #expect(after.children[0].browserCollapsed == parent.children[0].browserCollapsed)
        #expect(after.children.map(\.id) == [2])
        #expect(store.writes == [[1, 2]])
        #expect((owner.error != nil) == fails)
    }

    @Test(.timeLimit(.minutes(1)))
    func latestPendingChildOverridesParentAndGenerationChangeClearsIt() async throws {
        let store = Store(parents: 1)
        let owner = makeOwner(store)
        await owner.loadCounts()
        let parent = try #require(owner.homeDecks.first)
        let expanding = owner.setStudyExpanded(deckID: 1, expanded: true, includingDescendants: true)
        let collapsingChild = owner.setStudyExpanded(deckID: 2, expanded: false)
        #expect(owner.studyExpanded(parent))
        #expect(!owner.studyExpanded(parent.children[0]))
        await expanding.value
        await collapsingChild.value
        await owner.loadCounts()
        let saved = try #require(owner.homeDecks.first)
        #expect(owner.studyExpanded(saved))
        #expect(!owner.studyExpanded(saved.children[0]))
        #expect(store.writes == [[1], [2]])

        let stale = owner.setStudyExpanded(deckID: 2, expanded: true)
        #expect(owner.studyExpanded(saved.children[0]))
        store.generation = 2
        owner.receiveLifecycle(.ready(generation: 2))
        await owner.loadCounts()
        #expect(!owner.studyExpanded(saved.children[0]))
        #expect(!owner.isStudyExpansionPending(deckID: 2))
        await stale.value
        #expect(store.writes == [[1], [2]])
        #expect(owner.error == nil)
    }

    private func makeOwner(_ store: Store) -> ReviewCollection {
        let owner = ReviewCollection(configuration: LaunchConfiguration(), navigation: AppNavigation())
        owner.loadDeckTree = { store.tree }
        owner.saveStudyCollapse = { ids, collapsed in
            store.writes.append(ids)
            if store.fails { throw BridgeError.internal("synthetic save failure") }
            for id in ids {
                if collapsed { store.collapsed.insert(id) } else { store.collapsed.remove(id) }
            }
            return store.generation
        }
        owner.receiveLifecycle(.ready(generation: store.generation))
        return owner
    }

    private final class Store {
        let parents: Int
        var generation: UInt64 = 1
        var collapsed: Set<Int64>
        var fails = false
        var writes: [[Int64]] = []
        init(parents: Int) {
            self.parents = parents
            collapsed = Set((0..<parents).map { Int64($0 * 2 + 1) })
        }
        var tree: CountedDeckTree {
            CountedDeckTree(generation: generation, nextChangeAt: Int64.max, nodes: nodes)
        }
        var nodes: [CountedDeckNode] {
            (0..<parents).map { index in
                let id = Int64(index * 2 + 1)
                let child = CountedDeckNode(id: id + 1, name: "Child", filtered: false,
                    studyCollapsed: collapsed.contains(id + 1), browserCollapsed: true,
                    counts: ReviewCounts(new: 0, learning: 0, review: 0), children: [])
                return CountedDeckNode(id: id, name: "Parent \(index)", filtered: false,
                    studyCollapsed: collapsed.contains(id), browserCollapsed: false,
                    counts: ReviewCounts(new: 0, learning: 0, review: 0), children: [child])
            }
        }
    }
}
