@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// Today's Study (contract N24.3, `decks/deck-list` H12) and Today's numbers (N24.2, T12).
/// The collection tests own the process's real collection: run this suite with a focused selector.
@MainActor
@Suite(.serialized) struct StudyWalkTests {
    // MARK: The list and the order of turns

    private static func node(_ id: Int64, _ name: String, _ counts: (UInt32, UInt32, UInt32), filtered: Bool = false,
                             _ children: [CountedDeckNode] = []) -> CountedDeckNode {
        CountedDeckNode(id: id, name: name, filtered: filtered, studyCollapsed: false, browserCollapsed: false,
            counts: ReviewCounts(new: counts.0, learning: counts.1, review: counts.2), children: children)
    }

    @Test func theListIsTheTopLevelNormalDecksWithCardsWaitingInListOrder() {
        let tree = [Self.node(1, "Languages", (2, 1, 0), [Self.node(2, "Languages::Español", (2, 1, 0))]),
                    Self.node(3, "Quick Review", (0, 0, 9), filtered: true),
                    Self.node(4, "Reading", (0, 0, 0)),
                    Self.node(5, "Science", (0, 0, 4))]
        let decks = StudyWalk.decks(from: tree)
        // Filtered decks are never in it (H3); subdecks are studied through their top-level deck.
        #expect(decks == [.init(id: 1, name: "Languages", waiting: 3), .init(id: 5, name: "Science", waiting: 4)])
    }

    @Test func turnsGoRoundTheListWhileAPassAnswersACard() {
        var cursor = StudyWalk.Cursor(count: 3)
        #expect(cursor.next(answers: 0) == 0)
        #expect(cursor.next(answers: 4) == 1)
        #expect(cursor.next(answers: 4) == 2)
        // The first pass answered four cards: round again.
        #expect(cursor.next(answers: 4) == 0)
        #expect(cursor.next(answers: 5) == 1)
        #expect(cursor.next(answers: 5) == 2)
        // This pass answered one: once more.
        #expect(cursor.next(answers: 5) == 0)
        #expect(cursor.next(answers: 5) == 1)
        #expect(cursor.next(answers: 5) == 2)
        // A whole pass answered nothing: the walk ends, so it always ends.
        #expect(cursor.next(answers: 5) == nil)
        // After Keep Going a new pass starts at the first deck.
        cursor.restart(answers: 6)
        #expect(cursor.next(answers: 6) == 0)
        var empty = StudyWalk.Cursor(count: 0)
        #expect(empty.next(answers: 0) == nil)
    }

    /// N24.3's end: the different cards over the whole walk and the sum of its sessions' active time.
    @Test func theWalksNumbersCoverEverySession() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        var now: TimeInterval = 0
        func coordinator(deck: Int64, card: Int64, client: ReviewCoordinatorTests.Client) -> ReviewCoordinator {
            client.currentHead = ReviewCoordinatorTests.head(card)
            client.answerHead = .accepted(headDiff: ReviewHeadDiff(head: ReviewCoordinatorTests.emptyHead(.finished), marks: [], removed: [card]),
                                          leech: false, suspended: false, rolledOver: false)
            return ReviewCoordinator(deckID: deck, surface: ReviewCoordinatorTests.Surface(),
                store: ReviewRecordStore(file: directory.appending(path: "\(deck).json")),
                clock: .fixed(Date(timeIntervalSince1970: 1_800_000_000)), client: client, uptime: { now })
        }
        let walk = StudyWalk(decks: [.init(id: 1, name: "A", waiting: 2), .init(id: 2, name: "B", waiting: 1)])
        let firstClient = ReviewCoordinatorTests.Client()
        let first = coordinator(deck: 1, card: 1, client: firstClient)
        walk.present(first)
        await first.start()
        // Card 1 comes back once (a learning step), then the deck is done: 20 seconds.
        firstClient.answerHead = .accepted(headDiff: ReviewHeadDiff(head: ReviewCoordinatorTests.head(1), marks: [], removed: []),
                                           leech: false, suspended: false, rolledOver: false)
        first.reveal(); await first.settled()
        now = 10; first.rate(.again); await first.settled()
        firstClient.answerHead = .accepted(headDiff: ReviewHeadDiff(head: ReviewCoordinatorTests.emptyHead(.finished), marks: [], removed: [1]),
                                           leech: false, suspended: false, rolledOver: false)
        first.reveal(); await first.settled()
        now = 20; first.rate(.good); await first.settled()
        #expect(first.phase == .finished)
        walk.close(first, leftForEnd: false)
        _ = await first.end()
        // The second deck: one card, 45 seconds.
        now = 100
        let second = coordinator(deck: 2, card: 2, client: ReviewCoordinatorTests.Client())
        walk.present(second)
        await second.start()
        second.reveal(); await second.settled()
        now = 145; second.rate(.good); await second.settled()
        #expect(second.phase == .finished)
        #expect(walk.answers == 3)
        #expect(walk.summary == ReviewSessionSummary(cards: 2, seconds: 65))
        let previous = try #require(walk.previousSession(at: Date(timeIntervalSince1970: 1_800_000_200)))
        #expect(previous.decks == [1, 2] && previous.cards == 2 && previous.seconds == 65)
        // Closing a session twice counts it once.
        walk.close(first, leftForEnd: false)
        #expect(walk.answers == 3)
        _ = await second.end()
    }

    // MARK: The walk in the collection

    @Test func studyWalksTheDecksInOrderInOneReview() async throws {
        let tree = [Self.node(1, "Languages", (2, 0, 0)), Self.node(3, "Quick Review", (0, 0, 5), filtered: true),
                    Self.node(4, "Reading", (0, 0, 0)), Self.node(2, "Science", (3, 0, 0))]
        let client = WalkClient([1: [[11, 12]], 2: [[21, 22, 23]], 3: [[31]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            let presentation = try #require(owner.navigation.review?.id)
            #expect(walk.decks.map(\.id) == [1, 2])
            #expect(owner.review?.record.deckID == 1 && owner.review?.phase == .question)
            // The bar: the open deck's queue and the next deck's cards waiting at the start.
            #expect(walk.progress == ReviewSessionProgress(answered: 0, remaining: 2 + 3))
            #expect(!owner.canStudy)
            try await answer(owner)
            #expect(walk.progress == ReviewSessionProgress(answered: 1, remaining: 1 + 3))
            try await answer(owner)
            // The next deck starts at once, in the same presentation.
            #expect(owner.review?.record.deckID == 2 && owner.review?.phase == .question)
            #expect(owner.navigation.review?.id == presentation)
            #expect(walk.current === owner.review && walk.turns == 2 && !walk.advancing)
            #expect(walk.progress == ReviewSessionProgress(answered: 2, remaining: 3))
            for _ in 0..<3 { try await answer(owner) }
            // One finished screen, on the last deck, with the whole walk's numbers.
            #expect(walk.ended && !walk.advancing)
            #expect(owner.review?.record.deckID == 2 && owner.review?.phase == .finished)
            #expect(walk.summary.cards == 5)
            // Filtered and empty decks never started; every deck's start wrote its selection.
            #expect(client.starts == [1, 2])
            #expect(client.endedDecks == [1])
            await owner.finishReview()
            #expect(owner.walk == nil && owner.review == nil && owner.navigation.review == nil)
            let previous = try #require(owner.previousWalk)
            #expect(previous.decks == [1, 2] && previous.cards == 5)
            // A deck page shows a previous session only for a single-deck session.
            #expect(owner.previousRecord == nil)
            #expect(owner.canStudy)
        }
    }

    @Test func aDeckWaitingForLearningIsLeftForTheEndAndTheWalkGoesRoundAgain() async throws {
        let tree = [Self.node(1, "Languages", (1, 1, 0)), Self.node(2, "Science", (1, 0, 0))]
        // Languages answers 11, then waits for a learning card that is ready by its next turn.
        let client = WalkClient([1: [[11], [12]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            try await answer(owner)
            // It never shows Waiting for Learning: Science starts.
            #expect(owner.review?.record.deckID == 2 && owner.review?.phase == .question)
            #expect(walk.standing[1] == .leftForEnd)
            try await answer(owner)
            // Round again with the deck that has a card ready now.
            #expect(owner.review?.record.deckID == 1 && owner.review?.phase == .question)
            try await answer(owner)
            #expect(walk.ended && owner.review?.record.deckID == 1 && owner.review?.phase == .finished)
            #expect(client.starts == [1, 2, 1])
            #expect(walk.summary.cards == 3 && walk.answers == 3)
        }
    }

    @Test func aDeckWithNoCardAtItsTurnIsSkipped() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(5, "Emptied", (1, 0, 0)),
                    Self.node(6, "Deleted", (1, 0, 0)), Self.node(7, "Raced", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        // Emptied has nothing ready; Deleted is gone before its turn; Raced looked ready but its start finds no card.
        let client = WalkClient([1: [[11]], 5: [[]], 7: [[]], 2: [[21]]])
        client.deleted = [6]
        client.claimsReady = [7]
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            try await answer(owner)
            #expect(owner.review?.record.deckID == 2 && owner.review?.phase == .question)
            #expect(client.starts == [1, 7, 2])
            try await answer(owner)
            let walk = try #require(owner.walk)
            #expect(walk.ended && walk.summary.cards == 2)
        }
    }

    @Test func backToDecksStopsTheWalkAndReturnsToToday() async throws {
        let tree = [Self.node(1, "Languages", (2, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11, 12]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            try await answer(owner)
            await owner.finishReview()
            // The open session ended as any session ends; no other deck started.
            #expect(owner.walk == nil && owner.review == nil && owner.navigation.review == nil)
            #expect(client.starts == [1] && client.endedDecks == [1])
            let previous = try #require(owner.previousWalk)
            #expect(previous.decks == [1] && previous.cards == 1)
            await owner.waitForWalk()
            #expect(client.starts == [1])
        }
    }

    @Test func aRefusedStartStopsTheWalkAtToday() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        client.refused = [2]
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            try await answer(owner)
            // The next deck could not start: Today, with what was answered kept.
            #expect(owner.walk == nil && owner.review == nil && owner.navigation.review == nil)
            #expect(client.starts == [1, 2] && client.endedDecks == [1])
            #expect(owner.previousWalk?.cards == 1)
            #expect(owner.error == "This preset needs a newer Meguri")
        }
    }

    @Test func undoReachesOnlyTheOpenDecksAnswers() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (2, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21, 22]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            // The walk's first deck is an ordinary session.
            #expect(owner.review?.canUndo == true)
            try await answer(owner)
            // At the start of the next deck Undo waits for that deck's first answer.
            let second = try #require(owner.review)
            #expect(second.record.deckID == 2 && second.sessionScopedUndo)
            #expect(!second.canUndo && !second.canRedo)
            #expect(second.undoStatus == .unavailable(reason: .none))
            try await answer(owner)
            #expect(second.canUndo && !second.canRedo)
        }
    }

    // MARK: Today's numbers

    @Test func todaysNumbersAndTheNextLearningCardWhileNothingWaits() async throws {
        let tree = [Self.node(1, "Languages", (0, 0, 0)), Self.node(2, "Science", (0, 0, 0)),
                    Self.node(3, "Quick Review", (0, 0, 0), filtered: true)]
        try await withWalkOwner(tree: tree, client: WalkClient([:])) { owner in
            let asked = Calls()
            let ready: [Int64: Int64] = [1: 1_894_709_000, 2: 1_894_705_400, 3: 1_894_701_700]
            owner.loadDeckStudyState = { deck in
                asked.decks.append(deck)
                return WalkClient.state(deck: deck, ready: false, learningLater: true, readyAt: ready[deck])
            }
            let numbers = StudiedToday(answers: 140, cards: 128, timeMilliseconds: 1_140_000, recentAnswers: 900,
                                       recentTimeMilliseconds: 5_400_000, generation: owner.generation)
            owner.studiedTodayService = FixedStudied(value: numbers)
            await owner.loadTodayNumbers()
            #expect(owner.studiedToday == numbers)
            // The earliest among the top-level normal decks: never the filtered deck's.
            #expect(asked.decks == [1, 2])
            #expect(owner.nextLearningReadyAt == Date(timeIntervalSince1970: 1_894_705_400))
            // A read that fails keeps the last value; one of another collection is dropped.
            owner.studiedTodayService = FailingStudied()
            await owner.loadTodayNumbers()
            #expect(owner.studiedToday == numbers)
            owner.studiedTodayService = FixedStudied(value: StudiedToday(answers: 1, cards: 1, timeMilliseconds: 1,
                recentAnswers: 1, recentTimeMilliseconds: 1, generation: owner.generation &+ 1))
            await owner.loadTodayNumbers()
            #expect(owner.studiedToday == numbers)
        }
    }

    @Test func noLearningReadIsMadeWhileCardsWait() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0))]
        try await withWalkOwner(tree: tree, client: WalkClient([1: [[11]]])) { owner in
            let asked = Calls()
            owner.loadDeckStudyState = { deck in
                asked.decks.append(deck)
                return WalkClient.state(deck: deck, ready: true, learningLater: false)
            }
            await owner.loadTodayNumbers()
            #expect(asked.decks.isEmpty && owner.nextLearningReadyAt == nil)
        }
    }

    // MARK: Fixture

    private func answer(_ owner: ReviewCollection) async throws {
        let model = try #require(owner.review)
        try #require(model.phase == .question)
        model.reveal(); await model.settled()
        model.rate(.good); await model.settled()
        await owner.waitForWalk()
    }

    private func withWalkOwner(tree: [CountedDeckNode], client: WalkClient,
                               _ body: (ReviewCollection) async throws -> Void) async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: source, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let sync = SyncCoordinator(directory: directory, configuration: nil, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: directory.appending(path: "synthetic-installation", directoryHint: .isDirectory))
        owner.makeReviewSurface = { _ in ReviewCoordinatorTests.Surface() }
        owner.reviewSessionClient = client
        owner.loadDeckTree = { [unowned owner] in
            CountedDeckTree(generation: owner.generation, nextChangeAt: Int64.max, nodes: tree)
        }
        owner.saveSelectedDeck = { [unowned owner] _ in owner.generation }
        owner.studiedTodayService = FailingStudied()
        owner.loadDeckStudyState = { _ in throw BridgeError.internal("No study state scripted") }
        do {
            await owner.start()
            try #require(owner.state == .ready)
            await owner.waitForTodayNumbers()
            if owner.policy != .ok { await owner.enableFsrs() }
            try #require(owner.policy == .ok)
            try #require(owner.decks == tree)
            try await body(owner)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }

    private final class Calls { var decks: [Int64] = [] }

    private struct FixedStudied: StudiedTodayService {
        let value: StudiedToday
        func numbers() async throws(BridgeError) -> StudiedToday { value }
    }

    private struct FailingStudied: StudiedTodayService {
        func numbers() async throws(BridgeError) -> StudiedToday { throw .internal("No numbers scripted") }
    }

    /// One scripted deck per id: its rounds of cards. A round's cards are ready now; once a
    /// round is answered the deck waits for learning while another round follows (ready by
    /// its next turn, once its session ended), and is finished after its last.
    final class WalkClient: ReviewSessionClient {
        private(set) var rounds: [Int64: [[Int64]]]
        private(set) var starts: [Int64] = []
        private(set) var endedDecks: [Int64] = []
        var refused: Set<Int64> = []
        var deleted: Set<Int64> = []
        /// Decks whose study state says a card is ready although their start finds none.
        var claimsReady: Set<Int64> = []
        private var sessions: [UInt64: Int64] = [:]
        private var nextSession: UInt64 = 100

        init(_ rounds: [Int64: [[Int64]]]) { self.rounds = rounds }

        private func cards(_ deck: Int64) -> [Int64] { rounds[deck]?.first ?? [] }

        private func head(deck: Int64) -> ReviewHead {
            let cards = cards(deck)
            guard let card = cards.first else {
                return ReviewCoordinatorTests.emptyHead((rounds[deck]?.count ?? 0) > 1 ? .waitingUntil(instant: 4_000_000_000) : .finished)
            }
            return ReviewHead(items: ReviewCoordinatorTests.head(card).items,
                              counts: ReviewCounts(new: UInt32(cards.count), learning: 0, review: 0),
                              validUntil: Int64.max, terminalState: .active)
        }

        func start(deckID: Int64) async throws -> ReviewStartResult {
            starts.append(deckID)
            if refused.contains(deckID) { return .refused(reason: .presetUnsupported(presetIds: [1])) }
            nextSession += 1
            sessions[nextSession] = deckID
            return .started(sessionId: nextSession, generation: 1, head: head(deck: deckID), timeLim: 0)
        }
        func head(sessionID: UInt64) async throws -> ReviewHead { head(deck: sessions[sessionID] ?? 0) }
        func answer(sessionID: UInt64, item: ReviewHeadItem, rating: ReviewRating, at: Int64, elapsed: UInt64) async throws -> ReviewAnswerOutcome {
            let deck = sessions[sessionID] ?? 0
            if var deckRounds = rounds[deck], !deckRounds.isEmpty, !deckRounds[0].isEmpty {
                deckRounds[0].removeFirst()
                rounds[deck] = deckRounds
            }
            return .accepted(headDiff: ReviewHeadDiff(head: head(deck: deck), marks: [], removed: [item.cardId]),
                             leech: false, suspended: false, rolledOver: false)
        }
        func undoStatus(sessionID: UInt64, redo: Bool) async throws -> ReviewUndoStatus {
            .available(step: ReviewStepInfo(id: 1, name: "Answer Card", kind: .answer, session: sessionID, longWrite: nil))
        }
        func restore(sessionID: UInt64, redo: Bool) async throws -> ReviewHead { head(deck: sessions[sessionID] ?? 0) }
        func studyState(deckID: Int64) async throws -> ReviewDeckStudyState {
            if deleted.contains(deckID) { throw BridgeError.internal("No such deck") }
            let deckRounds = rounds[deckID] ?? []
            return Self.state(deck: deckID, ready: !cards(deckID).isEmpty || claimsReady.contains(deckID),
                              learningLater: deckRounds.count > 1)
        }
        func end(sessionID: UInt64) async throws {
            let deck = sessions[sessionID] ?? 0
            endedDecks.append(deck)
            // The learning card it waited for is ready by its next turn.
            if var deckRounds = rounds[deck], deckRounds.count > 1, deckRounds[0].isEmpty {
                deckRounds.removeFirst()
                rounds[deck] = deckRounds
            }
        }

        static func state(deck: Int64, ready: Bool, learningLater: Bool, readyAt: Int64? = nil) -> ReviewDeckStudyState {
            let none = ReviewCounts(new: 0, learning: 0, review: 0)
            return ReviewDeckStudyState(deck: deck, queueCounts: none, listCounts: none,
                difference: .init(new: 0, learning: 0, review: 0), finished: !ready,
                learningRemainingToday: learningLater ? 1 : 0, learningLaterToday: learningLater ? 1 : 0,
                nextLearningDue: readyAt, nextLearningReadyAt: readyAt, reviewLimitReached: false, newLimitReached: false,
                learningRemaining: learningLater ? 1 : 0, secsUntilNextLearn: 0, reviewRemaining: false, newRemaining: false,
                siblingBuried: 0, userBuried: 0, tomorrow: none, empty: false)
        }
    }
}
