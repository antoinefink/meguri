@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// Home before its first read (native-interface-contract N27): what each state shows, the
/// kept snapshot (written, read, keyed to one file, dropped), held counts, and a press on
/// kept content. The real-collection cases own the process's collection: run this suite
/// with a focused selector.
@MainActor
@Suite(.serialized) struct HomeLaunchTests {
    // MARK: N27.1, what each state shows

    @Test func notKnownStatesNeverShowTheWelcomeOrNoDecks() {
        let unknown: [ReviewCollection.State] = [.unopened, .loading, .ready]
        for state in unknown {
            for kept in [false, true] {
                // `decksEmpty` is true: before the first read the tree is empty.
                let phase = HomePhase.resolve(state: state, treeRead: false, decksEmpty: true, kept: kept)
                #expect(phase == (kept ? .kept : .pending), "\(state), kept \(kept)")
                #expect(phase != .welcome && phase != .noDecks)
            }
        }
        // An exclusive phase before any read: the kept read, else the busy state; never "No Decks".
        #expect(HomePhase.resolve(state: .unavailable, treeRead: false, decksEmpty: true, kept: true) == .kept)
        #expect(HomePhase.resolve(state: .unavailable, treeRead: false, decksEmpty: true, kept: false) == .unavailable)
    }

    @Test func welcomeAndNoDecksShowOnlyWhenTheyAreKnown() {
        // No collection to open: the welcome, kept or not (a kept read is never for a missing file).
        #expect(HomePhase.resolve(state: .noFile, treeRead: false, decksEmpty: true, kept: false) == .welcome)
        #expect(HomePhase.resolve(state: .noFile, treeRead: false, decksEmpty: true, kept: true) == .welcome)
        // A finished read: none, or the decks.
        #expect(HomePhase.resolve(state: .ready, treeRead: true, decksEmpty: true, kept: true) == .noDecks)
        #expect(HomePhase.resolve(state: .ready, treeRead: true, decksEmpty: false, kept: true) == .decks)
        // The other states keep their own views.
        #expect(HomePhase.resolve(state: .failed("x"), treeRead: false, decksEmpty: true, kept: true) == .failed("x"))
        #expect(HomePhase.resolve(state: .creationRecovery, treeRead: false, decksEmpty: true, kept: true) == .creationRecovery)
        #expect(HomePhase.resolve(state: .unavailable, treeRead: true, decksEmpty: false, kept: false) == .unavailable)
        // After a read, an exclusive phase never brings the kept (older) read back.
        #expect(HomePhase.resolve(state: .unavailable, treeRead: true, decksEmpty: false, kept: true) == .unavailable)
        // A first read that failed is known: its error, never a wait or "No Decks", kept or not.
        #expect(HomePhase.resolve(state: .ready, treeRead: false, readFailed: true, decksEmpty: true, kept: true) == .readFailed)
        #expect(HomePhase.resolve(state: .ready, treeRead: false, readFailed: true, decksEmpty: true, kept: false) == .readFailed)
    }

    // MARK: N27.2, the snapshot file

    @Test func snapshotBelongsToOneCollectionFile() throws {
        let directory = try Self.temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let collection = directory.appending(path: "collection.anki2")
        try Data("one".utf8).write(to: collection)
        let store = HomeSnapshotStore(file: directory.appending(path: "Caches/home-snapshot.json"))
        let identity = try #require(CollectionFileIdentity.of(collection))
        let snapshot = Self.snapshot(identity, dayEndsAt: 2_000_000_000)
        store.save(snapshot); store.flush()
        #expect(store.load(for: collection) == snapshot)
        #expect(try store.file.resourceValues(forKeys: [.isExcludedFromBackupKey]).isExcludedFromBackup == true)

        // Another file at another path is not this collection: the snapshot is deleted.
        let other = directory.appending(path: "other.anki2")
        try Data("one".utf8).write(to: other)
        #expect(store.load(for: other) == nil)
        store.flush()
        #expect(!FileManager.default.fileExists(atPath: store.file.path(percentEncoded: false)))

        // A replaced file at the same path (an import, a download, a fixture) is another file.
        store.save(snapshot); store.flush()
        let replacement = directory.appending(path: "replacement.anki2")
        try Data("two".utf8).write(to: replacement)
        _ = try FileManager.default.replaceItemAt(collection, withItemAt: replacement)
        #expect(CollectionFileIdentity.of(collection) != identity)
        #expect(store.load(for: collection) == nil)

        // No collection file at all: nothing is kept.
        store.save(snapshot); store.flush()
        try FileManager.default.removeItem(at: collection)
        #expect(store.load(for: collection) == nil)
        store.flush()
        #expect(!FileManager.default.fileExists(atPath: store.file.path(percentEncoded: false)))
    }

    @Test func snapshotOfAnotherVersionIsDropped() throws {
        let directory = try Self.temporaryDirectory()
        defer { try? FileManager.default.removeItem(at: directory) }
        let collection = directory.appending(path: "collection.anki2")
        try Data("one".utf8).write(to: collection)
        let store = HomeSnapshotStore(file: directory.appending(path: "home-snapshot.json"))
        var snapshot = Self.snapshot(try #require(CollectionFileIdentity.of(collection)), dayEndsAt: 2_000_000_000)
        snapshot.version = HomeSnapshot.currentVersion + 1
        store.save(snapshot); store.flush()
        #expect(store.load(for: collection) == nil)
    }

    /// N14.3: the kept read keeps which deck pages were empty; a file written before has none.
    @Test func snapshotKeepsEmptyDeckPages() throws {
        let identity = CollectionFileIdentity(fileNumber: 1, createdSeconds: 1, createdNanoseconds: 0)
        var snapshot = Self.snapshot(identity, dayEndsAt: 2_000_000_000)
        #expect(snapshot.emptyDecks == [])
        snapshot.emptyDecks = [2]
        let data = try JSONEncoder().encode(snapshot)
        #expect(try JSONDecoder().decode(HomeSnapshot.self, from: data) == snapshot)
        var object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
        object.removeValue(forKey: "emptyDecks")
        let older = try JSONDecoder().decode(HomeSnapshot.self, from: JSONSerialization.data(withJSONObject: object))
        #expect(older.emptyDecks == nil)
        #expect(older.decks == snapshot.decks)
    }

    // MARK: N27.3, counts of an earlier study day

    @Test func countsOfAnEarlierStudyDayAreHeldBack() throws {
        let identity = CollectionFileIdentity(fileNumber: 1, createdSeconds: 1, createdNanoseconds: 0)
        let dayEnd: Int64 = 1_894_744_800
        let before = Date(timeIntervalSince1970: Double(dayEnd - 60))
        let after = Date(timeIntervalSince1970: Double(dayEnd + 60))
        let current = KeptHome(snapshot: Self.snapshot(identity, dayEndsAt: dayEnd), now: before)
        #expect(current.countsCurrent)
        #expect(current.nodes.first?.counts == ReviewCounts(new: 3, learning: 1, review: 7))
        let earlier = KeptHome(snapshot: Self.snapshot(identity, dayEndsAt: dayEnd), now: after)
        #expect(!earlier.countsCurrent)
        // Names and the tree's shape stay; no count is drawn or spoken.
        #expect(earlier.nodes.map(\.id) == current.nodes.map(\.id))
        #expect(earlier.nodes.map(\.name) == current.nodes.map(\.name))
        #expect(earlier.nodes.first?.children.map(\.id) == current.nodes.first?.children.map(\.id))
        #expect(Self.allCounts(earlier.nodes).allSatisfy { $0 == ReviewCounts(new: 0, learning: 0, review: 0) })
        // An unknown day end is never taken for today.
        #expect(!KeptHome(snapshot: Self.snapshot(identity, dayEndsAt: nil), now: before).countsCurrent)
    }

    // MARK: Support

    final class Phases {
        var values: [HomePhase]
        init(_ first: HomePhase) { values = [first] }
    }

    final class Fixture {
        let directory: URL
        let snapshotFile: URL
        init() throws {
            directory = try HomeLaunchTests.temporaryDirectory()
            snapshotFile = directory.deletingLastPathComponent().appending(path: "\(directory.lastPathComponent)-home-snapshot.json")
            let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
            try FileManager.default.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
        }
        @MainActor func owner(keptExtraDeck: Int64? = nil) -> ReviewCollection {
            if let keptExtraDeck, let data = try? Data(contentsOf: snapshotFile),
               var snapshot = try? JSONDecoder().decode(HomeSnapshot.self, from: data) {
                snapshot.decks.append(.init(id: keptExtraDeck, name: "Gone", filtered: false, studyCollapsed: false,
                    browserCollapsed: false, new: 1, learning: 0, review: 0, children: []))
                try? JSONEncoder().encode(snapshot).write(to: snapshotFile)
            }
            var configuration = LaunchConfiguration()
            configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
            configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
            configuration.timeZone = TimeZone(secondsFromGMT: 0)
            configuration.homeSnapshotFile = snapshotFile
            let sync = SyncCoordinator(directory: directory, defaults: nil)
            let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
                providerDirectory: directory.appending(path: "synthetic-installation", directoryHint: .isDirectory))
            owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
            return owner
        }
        func remove() {
            try? FileManager.default.removeItem(at: directory)
            try? FileManager.default.removeItem(at: snapshotFile)
        }
    }

    nonisolated static func temporaryDirectory() throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }

    static func snapshot(_ identity: CollectionFileIdentity, dayEndsAt: Int64?) -> HomeSnapshot {
        let child = CountedDeckNode(id: 2, name: "Parent::Child", filtered: false, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: 1, learning: 1, review: 2), children: [])
        let parent = CountedDeckNode(id: 1, name: "Parent", filtered: false, studyCollapsed: true, browserCollapsed: false,
            counts: ReviewCounts(new: 3, learning: 1, review: 7), children: [child])
        return HomeSnapshot(collection: identity, dayEndsAt: dayEndsAt, decks: [parent],
            studied: StudiedToday(answers: 4, cards: 3, timeMilliseconds: 60_000, recentAnswers: 20, recentTimeMilliseconds: 300_000),
            nextLearningReadyAt: nil, previous: .init(deckNames: ["Parent"], walk: false, cards: 3, endedAt: Date(timeIntervalSince1970: 1)),
            previousAnswers: 4)
    }

    static func flattened(_ nodes: [CountedDeckNode]) -> [CountedDeckNode] {
        nodes.flatMap { [$0] + flattened($0.children) }
    }

    static func allCounts(_ nodes: [CountedDeckNode]) -> [ReviewCounts] { flattened(nodes).map(\.counts) }
}

/// The real-collection cases: the process has one collection, so they run serialized with
/// the other suites that own it.
extension ReviewCollectionStartupTests {
    // MARK: The real collection: write, read, row identities, a press while not ready

    @Test(.timeLimit(.minutes(3))) func homeLaunchWritesTheSnapshotAndTheNextLaunchDrawsItFirst() async throws {
        let fixture = try HomeLaunchTests.Fixture()
        defer { fixture.remove() }
        // First launch: nothing kept; the open shows no welcome and no "No Decks".
        let first = fixture.owner()
        #expect(first.keptHome == nil)
        #expect(first.homePhase == .pending)
        let seen = HomeLaunchTests.Phases(first.homePhase)
        first.startupStep = { [weak first] _ in if let first { seen.values.append(first.homePhase) } }
        await first.start()
        seen.values.append(first.homePhase)
        #expect(first.homePhase == .decks)
        #expect(!seen.values.contains(.welcome) && !seen.values.contains(.noDecks), "\(seen.values)")
        let read = first.decks
        first.flushHomeSnapshot()
        #expect(await first.close())

        // Second launch: the kept tree before any open, with today's counts and the saved disclosures.
        let second = fixture.owner()
        let kept = try #require(second.keptHome)
        #expect(kept.countsCurrent)
        #expect(second.homePhase == .kept)
        #expect(second.homeDecks == read)
        for node in HomeLaunchTests.flattened(read) {
            #expect(second.studyExpanded(deckID: node.id) == !node.studyCollapsed)
        }
        let phases = HomeLaunchTests.Phases(second.homePhase)
        second.startupStep = { [weak second] _ in if let second { phases.values.append(second.homePhase) } }
        await second.start()
        phases.values.append(second.homePhase)
        #expect(!phases.values.contains(.welcome) && !phases.values.contains(.noDecks) && !phases.values.contains(.pending),
                "\(phases.values)")
        // In place: the read tree has the kept rows' identities, in the same order.
        #expect(HomeLaunchTests.flattened(second.homeDecks).map(\.id) == HomeLaunchTests.flattened(read).map(\.id))
        #expect(second.homePhase == .decks)
        #expect(await second.close())
    }

    /// N14.3, H5.4: a deck page read empty offers Add Note on the next launch's kept page,
    /// and still after the tree read, until its own page reads it again.
    @Test(.timeLimit(.minutes(3))) func homeLaunchKeepsWhichDeckPagesWereEmpty() async throws {
        let fixture = try HomeLaunchTests.Fixture()
        defer { fixture.remove() }
        let first = fixture.owner()
        await first.start()
        let deck = try #require(first.decks.first(where: { !$0.filtered }))
        #expect(!first.homeDeckEmpty(deck.id))
        #expect(await first.openDeckPage(deckID: deck.id).value)
        first.loadFixtureDeckEmptiness = { _ in true }
        await first.refreshDeckPage(try #require(first.deckPageRefreshRequest))
        #expect(first.homeDeckEmpty(deck.id))
        first.flushHomeSnapshot()
        #expect(await first.close())

        let second = fixture.owner()
        try #require(second.homePhase == .kept)
        #expect(second.keptHome?.snapshot.emptyDecks == [deck.id])
        #expect(second.homeDeckEmpty(deck.id))
        await second.start()
        #expect(second.homePhase == .decks)
        #expect(second.homeDeckEmpty(deck.id))
        #expect(await second.close())
    }

    @Test(.timeLimit(.minutes(3))) func homeLaunchKeepsEmptyDecksWhenAPageReadsBeforeTheTreeRetry() async throws {
        let fixture = try HomeLaunchTests.Fixture()
        defer { fixture.remove() }
        let first = fixture.owner()
        await first.start()
        let deck = try #require(first.decks.first(where: { !$0.filtered }))
        // Use another fixture deck when present, otherwise a synthetic page ID.
        let freshID = HomeLaunchTests.flattened(first.decks).first(where: { !$0.filtered && $0.id != deck.id })?.id ?? -1
        #expect(await first.openDeckPage(deckID: deck.id).value)
        first.loadFixtureDeckEmptiness = { _ in true }
        await first.refreshDeckPage(try #require(first.deckPageRefreshRequest))
        #expect(first.homeDeckEmpty(deck.id))
        first.flushHomeSnapshot()
        #expect(await first.close())

        let second = fixture.owner()
        try #require(second.keptHome?.snapshot.emptyDecks == [deck.id])
        let loadDeckTree = second.loadDeckTree
        // Startup retries a failed initial read: keep every attempt failing until restored.
        second.loadDeckTree = { throw CocoaError(.fileReadUnknown) }
        await second.start()
        try #require(second.state == .ready)
        try #require(second.homePhase == .readFailed)
        second.navigation.openDeckPage(DeckID(freshID))
        second.loadFixtureDeckEmptiness = { _ in true }
        await second.refreshDeckPage(try #require(second.deckPageRefreshRequest))
        #expect(second.emptyDeckPageID == freshID)
        #expect(second.homeDeckEmpty(freshID))

        second.loadDeckTree = loadDeckTree
        await second.loadCounts()
        #expect(second.homePhase == .decks)
        #expect(second.deck(deck.id) != nil)
        #expect(second.homeDeckEmpty(deck.id))
        #expect(second.homeDeckEmpty(freshID))
        #expect(await second.close())
    }

    @Test(.timeLimit(.minutes(3))) func homeLaunchASnapshotOfAnotherCollectionNeverShows() async throws {
        let fixture = try HomeLaunchTests.Fixture()
        defer { fixture.remove() }
        let first = fixture.owner()
        await first.start()
        first.flushHomeSnapshot()
        #expect(await first.close())
        // The same path, another file (a restored backup, an import done elsewhere).
        let collection = fixture.directory.appending(path: "collection.anki2")
        let copy = fixture.directory.appending(path: "copy.anki2")
        try FileManager.default.copyItem(at: collection, to: copy)
        _ = try FileManager.default.replaceItemAt(collection, withItemAt: copy)
        let second = fixture.owner()
        #expect(second.keptHome == nil)
        #expect(second.homePhase == .pending)
    }

    @Test(.timeLimit(.minutes(3))) func homeLaunchAReplacementDropsTheKeptRead() async throws {
        let fixture = try HomeLaunchTests.Fixture()
        defer { fixture.remove() }
        let first = fixture.owner()
        await first.start()
        first.flushHomeSnapshot()
        #expect(await first.close())
        let second = fixture.owner()
        try #require(second.keptHome != nil)
        second.receive(CoreEvent(seq: 1, generation: 99, kind: .collectionReplaced(generation: 99)))
        #expect(second.keptHome == nil)
        #expect(second.homePhase != .kept)
        second.flushHomeSnapshot()
        #expect(!FileManager.default.fileExists(atPath: fixture.snapshotFile.path(percentEncoded: false)))
        await second.waitForCollectionActivation()
        _ = await second.close()
    }

    @Test(.timeLimit(.minutes(3))) func homeLaunchAPressOnKeptContentWaitsAndActsOnTheReadTree() async throws {
        let fixture = try HomeLaunchTests.Fixture()
        defer { fixture.remove() }
        let first = fixture.owner()
        await first.start()
        let deck = try #require(first.decks.first)
        first.flushHomeSnapshot()
        #expect(await first.close())

        // A deck the read tree has: the press waits for the read, then opens it.
        let second = fixture.owner()
        try #require(second.homePhase == .kept)
        let open = second.openDeckPage(deckID: deck.id)
        #expect(second.navigation.homeDeckPage == nil, "Nothing opens before the read")
        await second.start()
        #expect(await open.value)
        #expect(second.navigation.homeDeckPage == DeckID(deck.id))
        #expect(await second.close())

        // A kept deck the read tree lacks (deleted elsewhere): the press does nothing.
        let third = fixture.owner(keptExtraDeck: 987_654_321)
        try #require(third.homeDeck(987_654_321) != nil)
        let stale = third.openDeckPage(deckID: 987_654_321)
        await third.start()
        #expect(!(await stale.value))
        #expect(third.navigation.homeDeckPage == nil)

        // A later press supersedes an earlier one that still waits.
        #expect(await third.close())
        let fourth = fixture.owner()
        let superseded = fourth.openDeckPage(deckID: deck.id)
        fourth.navigation.openHome()
        await fourth.start()
        #expect(!(await superseded.value))
        #expect(fourth.navigation.homeDeckPage == nil)
        #expect(await fourth.close())
    }

}
