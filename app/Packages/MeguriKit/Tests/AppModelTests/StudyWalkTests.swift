@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// Today's Study (contract N24.3, `decks/deck-list` H12) and Today's numbers (N24.2, T12).
/// The collection tests own the process's real collection: the suite sits inside
/// `ReviewCollectionStartupTests`, whose serialised trait also covers its child suites, so it
/// never runs beside the other real-collection suites. Run it with a focused selector.
extension ReviewCollectionStartupTests {
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
                    Self.node(6, "Unreadable", (1, 0, 0)), Self.node(7, "Raced", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        // Emptied has nothing ready; Unreadable's state cannot be read, so its start decides and
        // finds no card; Raced looked ready but its start finds no card.
        let client = WalkClient([1: [[11]], 5: [[]], 7: [[]], 2: [[21]]])
        client.deleted = [6]
        client.claimsReady = [7]
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            try await answer(owner)
            #expect(owner.review?.record.deckID == 2 && owner.review?.phase == .question)
            #expect(client.starts == [1, 6, 7, 2])
            try await answer(owner)
            let walk = try #require(owner.walk)
            #expect(walk.ended && walk.summary.cards == 2)
        }
    }

    @Test func aFailedReadinessReadStillStudiesTheDeck() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        client.deleted = [2]
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            try await answer(owner)
            // A read error is not an empty deck: Science starts and is studied.
            #expect(owner.review?.record.deckID == 2 && owner.review?.phase == .question)
            try await answer(owner)
            #expect(owner.walk?.summary.cards == 2)
        }
    }

    @Test func aRejectedAnswerWhileTheSessionEndsIsNotCounted() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            // Languages' last answer fails to commit while its session ends.
            client.onEnd = { [unowned owner] deck in
                if deck == 1 { owner.review?.commitFailed(cardID: 11, error: .internal("Rejected while ending")) }
            }
            try await answer(owner)
            #expect(owner.review?.record.deckID == 2)
            #expect(walk.answers == 0 && walk.summary.cards == 0)
            try await answer(owner)
            #expect(walk.ended && walk.answers == 1 && walk.summary.cards == 1)
            await owner.finishReview()
            #expect(owner.previousWalk?.decks == [2] && owner.previousWalk?.cards == 1)
        }
    }

    @Test func aDeckListStartGivesWayToALaterSelection() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            owner.openDeckPage(deckID: 1)
            await owner.deckPageOpened()
            // A double-click on Languages, then a click on Science before its start runs.
            let superseded = owner.deckListStudyRequest(deckID: 1)
            owner.openDeckPage(deckID: 2)
            await owner.startFromDeckList(superseded)
            #expect(owner.review == nil && client.starts.isEmpty)
            #expect(owner.navigation.homeDeckPage?.rawValue == 2)
            // A double-click whose second click opens the same deck again still starts it.
            let request = owner.deckListStudyRequest(deckID: 2)
            owner.openDeckPage(deckID: 2)
            await owner.startFromDeckList(request)
            #expect(owner.review?.record.deckID == 2 && client.starts == [2])
            await owner.finishReview()
            // A later section (Today) cancels it too.
            let left = owner.deckListStudyRequest(deckID: 2)
            owner.navigation.select(.section(.home))
            await owner.startFromDeckList(left)
            #expect(owner.review == nil && client.starts == [2])
        }
    }

    @Test func todaysStudyChecksItsAvailabilityWhenItRuns() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0))]
        let client = WalkClient([1: [[11]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            #expect(owner.studyAllAvailable)
            // A sheet presented after the press, before the start runs.
            owner.navigation.present(.deckName(.create))
            #expect(!owner.studyAllAvailable)
            await owner.startStudyAll()
            #expect(owner.walk == nil && owner.review == nil && client.starts.isEmpty)
            owner.navigation.dismissSheet()
            await owner.startStudyAll()
            #expect(owner.review?.record.deckID == 1)
            await owner.finishReview()
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
        let tree = [Self.node(1, "Languages", (2, 0, 0)), Self.node(2, "Science", (2, 0, 0))]
        let client = WalkClient([1: [[11, 12]], 2: [[21, 22]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            // The first deck's Undo also reaches only its own answers.
            let first = try #require(owner.review)
            #expect(first.sessionScopedUndo && !first.canUndo)
            try await answer(owner)
            #expect(first.canUndo)
            // While the walk moves between decks, Undo and Redo wait.
            walk.markAdvancing()
            #expect(!first.canUndo)
            walk.settle()
            #expect(first.canUndo)
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

    @Test func aDecksOwnSessionReplacesThePreviousWalk() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            try await answer(owner)
            await owner.finishReview()
            #expect(owner.previousWalk?.decks == [1])
            // One deck's session: Today's Previous Session is that deck's, not the walk's.
            await owner.startStudy(deckID: 2)
            #expect(owner.previousWalk == nil)
            try await answer(owner)
            await owner.finishReview()
            #expect(owner.previousWalk == nil && owner.previousRecord?.deckID == 2)
        }
    }

    @Test func aSessionThatFailsToEndStaysOnScreenAndBackToDecksTriesAgain() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        client.failEndOnce = [1]
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            let first = try #require(owner.review)
            try await answer(owner)
            // Its answers are kept and no other deck starts: the review shows its error.
            #expect(owner.review === first && first.phase == .error)
            #expect(walk.stopped && !walk.advancing && owner.navigation.review != nil)
            #expect(client.starts == [1] && client.endedDecks.isEmpty)
            await owner.finishReview()
            #expect(owner.review == nil && owner.walk == nil && owner.navigation.review == nil)
            #expect(client.endedDecks == [1] && client.starts == [1])
            #expect(owner.previousWalk?.cards == 1)
        }
    }

    @Test func progressCountsWhatAStudiedDeckLeftForTheEnd() async throws {
        // The list counted 5 cards in Languages; its session leaves 1 learning card for the end.
        let tree = [Self.node(1, "Languages", (4, 1, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11], [12]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            #expect(walk.progress == ReviewSessionProgress(answered: 0, remaining: 1 + 1))
            try await answer(owner)
            #expect(owner.review?.record.deckID == 2)
            // Science's queue, and what Languages left: never its count from the start.
            #expect(walk.progress == ReviewSessionProgress(answered: 1, remaining: 1 + 1))
        }
    }

    @Test func keepGoingOnTheWalksFinishedScreenGoesOnAsInTurns() async throws {
        let tree = [Self.node(2, "Science", (1, 0, 0)), Self.node(1, "Languages", (1, 1, 0))]
        let client = WalkClient([2: [[21]], 1: [[11], [12]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            try await answer(owner)
            try await answer(owner)
            // Languages waits for learning and nothing else has a card: the walk's finished screen.
            let open = try #require(owner.review)
            #expect(walk.ended && open.record.deckID == 1 && open.phase == .waiting && !open.continueAvailable)
            // Its learning card comes due: its Keep Going shows, and goes on with it.
            client.makeReady(1)
            open.requestRefresh(); await open.settled()
            #expect(open.continueAvailable && open.canContinue)
            let asked = client.stateRequests[2] ?? 0
            open.continueReview(); await open.settled()
            #expect(open.phase == .question)
            try await answer(owner)
            // Another pass checked the other deck, then the walk ended again on Languages.
            #expect((client.stateRequests[2] ?? 0) > asked)
            #expect(walk.ended && owner.review === open && open.phase == .finished)
            #expect(client.starts == [2, 1] && walk.summary.cards == 3)
        }
    }

    @Test func aRefusalAtATurnOffersTryAgainForTheWalk() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        client.policyRefused = [2]
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            try await answer(owner)
            // The core refused twice (F1 item 5): F5.3's refusal, for the walk, at Today.
            #expect(owner.studyRefusal?.studyAll == true)
            #expect(owner.walk == nil && owner.review == nil && owner.navigation.review == nil)
            #expect(client.starts == [1, 2, 2])
            // The second try was the same turn, not another one.
            #expect(walk.turns == 2)
            // Try Again starts Study again, with what waits now.
            client.policyRefused = []
            await owner.retryRefusedStudy()
            let again = try #require(owner.walk)
            #expect(again !== walk && owner.studyRefusal == nil)
            #expect(owner.review?.record.deckID == 2 && owner.review?.phase == .question)
        }
    }

    @Test func anInactiveWindowBetweenDecksStopsTheWalkAtToday() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            // The window stops being active while the walk asks whether Science is ready.
            client.onStudyState = { [unowned owner] deck in if deck == 2 { owner.setSceneActive(false) } }
            try await answer(owner)
            await owner.waitForLifecycle()
            #expect(owner.walk == nil && owner.review == nil && owner.navigation.review == nil)
            #expect(client.starts == [1] && client.endedDecks == [1])
            #expect(owner.previousWalk?.cards == 1)
            owner.setSceneActive(true)
            await owner.waitForLifecycle()
        }
    }

    @Test func backToDecksWhileTheNextDeckStartsStopsTheWalk() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        let finishing = Finishing()
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudyAll()
            client.onStart = { [unowned owner] deck in
                if deck == 2 { finishing.task = Task { await owner.finishReview() } }
            }
            try await answer(owner)
            await finishing.task?.value
            await owner.waitForWalk()
            #expect(owner.walk == nil && owner.review == nil && owner.navigation.review == nil)
            #expect(client.starts == [1, 2] && client.endedDecks == [1, 2])
            #expect(owner.previousWalk?.decks == [1] && owner.previousWalk?.cards == 1)
        }
    }

    @Test func aStartThatStopsWithItsSessionOpenStaysOnScreen() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            let record = owner.configuration.collectionLocation.directory.appending(path: "meguri-review-session.json")
            await owner.startStudyAll()
            let walk = try #require(owner.walk)
            // Science's start: the window stops being active and the session record cannot be
            // saved, so the pause after startup fails while the core's session is open.
            client.onStart = { [unowned owner] deck in
                guard deck == 2 else { return }
                try? FileManager.default.removeItem(at: record)
                try? FileManager.default.createDirectory(at: record, withIntermediateDirectories: false)
                owner.setSceneActive(false)
            }
            try await answer(owner)
            let open = try #require(owner.review)
            #expect(open.record.deckID == 2 && open.sessionID != nil && open.phase != .ended)
            #expect(walk.stopped && owner.navigation.review != nil && client.endedDecks == [1])
            // Back to Decks closes it once the record can be saved again.
            try FileManager.default.removeItem(at: record)
            owner.setSceneActive(true)
            await owner.waitForLifecycle()
            await owner.finishReview()
            #expect(owner.review == nil && owner.walk == nil && owner.navigation.review == nil)
            #expect(client.endedDecks == [1, 2] && owner.previousWalk?.cards == 1)
        }
    }

    @Test func finishingAReviewThatWasReplacedMeanwhileLeavesTheSuccessor() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0))]
        let client = WalkClient([1: [[11]]])
        let successor = ReviewCoordinatorTests.Fixture()
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudy(deckID: 1)
            let first = try #require(owner.review)
            // While the first review's session ends, another review takes its place.
            client.onEnd = { [unowned owner] deck in if deck == 1 { owner.presentReview(successor.model) } }
            await owner.finishReview()
            #expect(owner.review === successor.model && owner.navigation.review != nil)
            #expect(owner.previousRecord == nil && first.phase == .ended)
            client.onEnd = nil
            await owner.finishReview()
            #expect(owner.review == nil)
        }
    }

    @Test func aWalkWithNoAnswerLeavesThePreviousSessionAsItWas() async throws {
        let tree = [Self.node(1, "Languages", (1, 0, 0)), Self.node(2, "Science", (1, 0, 0))]
        let client = WalkClient([1: [[11]], 2: [[21]]])
        try await withWalkOwner(tree: tree, client: client) { owner in
            await owner.startStudy(deckID: 2)
            try await answer(owner)
            await owner.finishReview()
            #expect(owner.previousRecord?.deckID == 2)
            // A walk stopped before its first answer.
            await owner.startStudyAll()
            #expect(owner.review?.record.deckID == 1)
            await owner.finishReview()
            #expect(owner.previousRecord?.deckID == 2 && owner.previousWalk == nil)
            // A walk whose every deck is skipped: nothing is presented and nothing changes.
            await owner.startStudy(deckID: 1)
            try await answer(owner)
            await owner.finishReview()
            #expect(owner.previousRecord?.deckID == 1)
            await owner.startStudyAll()
            #expect(owner.walk == nil && owner.review == nil && client.starts == [2, 1, 1])
            #expect(owner.previousRecord?.deckID == 1 && owner.previousWalk == nil)
        }
    }

    // MARK: Today's numbers

    @Test func aDeckThatCannotBeReadLeavesTheNextLearningTimeUnknown() async throws {
        let tree = [Self.node(1, "Languages", (0, 0, 0)), Self.node(2, "Science", (0, 0, 0))]
        try await withWalkOwner(tree: tree, client: WalkClient([:])) { owner in
            // A read that succeeds with no learning card still counts.
            owner.loadDeckStudyState = { deck in
                WalkClient.state(deck: deck, ready: false, learningLater: deck == 1, readyAt: deck == 1 ? 1_894_705_400 : nil)
            }
            await owner.loadTodayNumbers()
            #expect(owner.nextLearningReadyAt == Date(timeIntervalSince1970: 1_894_705_400))
            // One deck that cannot be read: the time is unknown, not the other deck's.
            owner.loadDeckStudyState = { deck in
                if deck == 2 { throw BridgeError.internal("Not readable now") }
                return WalkClient.state(deck: deck, ready: false, learningLater: true, readyAt: 1_894_705_400)
            }
            await owner.loadTodayNumbers()
            #expect(owner.nextLearningReadyAt == nil)
        }
    }

    @Test func aScanOfOlderCountsIsDropped() async throws {
        let tree = [Self.node(1, "Languages", (0, 0, 0))]
        try await withWalkOwner(tree: tree, client: WalkClient([:])) { owner in
            let once = Calls()
            owner.loadDeckStudyState = { [unowned owner] deck in
                if once.decks.isEmpty {
                    once.decks.append(deck)
                    // Newer counts arrive while the scan reads: it belongs to the old ones.
                    await owner.loadCounts()
                }
                return WalkClient.state(deck: deck, ready: false, learningLater: true, readyAt: 1_894_705_400)
            }
            await owner.loadTodayNumbers()
            #expect(owner.nextLearningReadyAt == nil)
            // The newer counts' own scan sets it.
            await owner.waitForTodayNumbers()
            #expect(owner.nextLearningReadyAt == Date(timeIntervalSince1970: 1_894_705_400))
        }
    }

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
    private final class Finishing { var task: Task<Void, Never>? }

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
        /// Decks the core refuses to start until FSRS is on (F1 item 5).
        var policyRefused: Set<Int64> = []
        /// Decks whose session's first end fails.
        var failEndOnce: Set<Int64> = []
        var onStart: ((Int64) -> Void)?
        var onStudyState: ((Int64) -> Void)?
        /// Called as a session's end reaches the core, before it is answered.
        var onEnd: ((Int64) -> Void)?
        private(set) var stateRequests: [Int64: Int] = [:]
        private var sessions: [UInt64: Int64] = [:]
        private var nextSession: UInt64 = 100

        init(_ rounds: [Int64: [[Int64]]]) { self.rounds = rounds }

        private func cards(_ deck: Int64) -> [Int64] { rounds[deck]?.first ?? [] }

        private func head(deck: Int64) -> ReviewHead {
            let cards = cards(deck)
            guard let card = cards.first else {
                // Waiting for learning: the next round's cards are its learning count.
                guard let next = rounds[deck]?.dropFirst().first else { return ReviewCoordinatorTests.emptyHead(.finished) }
                return ReviewHead(items: [], counts: ReviewCounts(new: 0, learning: UInt32(next.count), review: 0),
                                  validUntil: Int64.max, terminalState: .waitingUntil(instant: 4_000_000_000))
            }
            return ReviewHead(items: ReviewCoordinatorTests.head(card).items,
                              counts: ReviewCounts(new: UInt32(cards.count), learning: 0, review: 0),
                              validUntil: Int64.max, terminalState: .active)
        }

        func start(deckID: Int64) async throws -> ReviewStartResult {
            starts.append(deckID)
            onStart?(deckID)
            if refused.contains(deckID) { return .refused(reason: .presetUnsupported(presetIds: [1])) }
            if policyRefused.contains(deckID) { return .refused(reason: .fsrsPolicyPending) }
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
            stateRequests[deckID, default: 0] += 1
            onStudyState?(deckID)
            if deleted.contains(deckID) { throw BridgeError.internal("No such deck") }
            let deckRounds = rounds[deckID] ?? []
            return Self.state(deck: deckID, ready: !cards(deckID).isEmpty || claimsReady.contains(deckID),
                              learningLater: deckRounds.count > 1)
        }
        func end(sessionID: UInt64) async throws {
            let deck = sessions[sessionID] ?? 0
            onEnd?(deck)
            if failEndOnce.remove(deck) != nil { throw BridgeError.internal("The session could not end") }
            endedDecks.append(deck)
            // The learning card it waited for is ready by its next turn.
            if var deckRounds = rounds[deck], deckRounds.count > 1, deckRounds[0].isEmpty {
                deckRounds.removeFirst()
                rounds[deck] = deckRounds
            }
        }

        /// The learning card a waiting deck waits for comes due, in its open session.
        func makeReady(_ deck: Int64) {
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
}
