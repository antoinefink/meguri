import CardRendering
@testable import CoreBridge
import Foundation
import enum MeguriCoreBindings.CoreEventKind
import enum MeguriCoreBindings.OpSource
import Observation
import Testing
@testable import AppModel

/// Study opens on its first card (N16.8; Antoine, 2026-11-21: "remove that
/// screen … make the Study click instant"): the screen Study was on stays
/// until the card is ready, a slow start shows the review with its progress
/// indicator after the delay, and the warm-up before the click only reads.
/// The collection tests own the process's real collection, so the suite sits
/// inside `ReviewCollectionStartupTests`'s serialised trait.
extension ReviewCollectionStartupTests {
@MainActor
@Suite(.serialized) struct StudyStartPresentationTests {
    @Test func theReviewAppearsWithItsFirstCardAndNoScreenBefore() async throws {
        try await withOwner { owner, client, surface in
            var shownDuringStart: Bool?
            var shownAtSwap: Bool?
            var phaseAtSwap: ReviewCoordinator.Phase?
            client.onStartResult = { shownDuringStart = owner.navigation.review != nil }
            surface.onPresent = {
                shownAtSwap = owner.navigation.review != nil
                phaseAtSwap = owner.review?.phase
            }
            await owner.startStudy(deckID: 1)
            // Nothing showed while the core started the session.
            #expect(shownDuringStart == false)
            // The review was asked for in the same turn as the swap, before the card
            // counted as shown: its timer and audio start after this.
            #expect(shownAtSwap == true && phaseAtSwap == .loading)
            #expect(owner.review?.phase == .question && owner.navigation.review != nil)
            #expect(!owner.studyStarting)
            await owner.finishReview()
        }
    }

    @Test func aSlowStartShowsTheReviewAfterTheDelayThenItsCard() async throws {
        try await withOwner { owner, client, _ in
            let gate = ReviewCoordinatorTests.CancellableGate(), wait = ReviewCoordinatorTests.CancellableGate()
            var asked: Duration?
            owner.reviewRevealWait = { delay in asked = delay; try? await wait.wait() }
            client.onStartResult = { try await gate.wait() }
            let study = Task { await owner.startStudy(deckID: 1) }
            await gate.requested()
            await wait.requested()
            // The wait counts from the click: what is left of the 60 s set here.
            #expect(owner.navigation.review == nil && owner.studyStarting)
            let waited = try #require(asked)
            #expect(waited <= .seconds(60) && waited > .zero)
            // Past the delay the review shows, still loading: the progress indicator.
            wait.release()
            let deadline = ContinuousClock.now + .seconds(5)
            while owner.navigation.review == nil, ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
            #expect(owner.navigation.review != nil && owner.review?.phase == .loading)
            gate.release()
            await study.value
            #expect(owner.review?.phase == .question)
            await owner.finishReview()
        }
    }

    @Test func aRefusedStartNeverShowsTheReview() async throws {
        try await withOwner { owner, client, _ in
            client.refusal = .presetUnsupported(presetIds: [3])
            let changed = Flag()
            withObservationTracking { _ = owner.navigation.review } onChange: { changed.value = true }
            await owner.startStudy(deckID: 1)
            #expect(!changed.value && owner.navigation.review == nil && owner.review == nil)
            #expect(owner.error != nil)
            // The deck page gets warm pages again for its next Study.
            await owner.studyWarmup.settled()
            #expect(owner.studyWarmup.surface != nil)
        }
    }

    /// Going elsewhere while Study waits ends the session without showing it (R22).
    @Test func goingElsewhereDuringTheWaitEndsTheSessionUnseen() async throws {
        try await withOwner { owner, client, _ in
            let gate = ReviewCoordinatorTests.CancellableGate()
            client.onStartResult = { try await gate.wait() }
            let study = Task { await owner.startStudy(deckID: 1) }
            await gate.requested()
            owner.navigation.select(.section(.browse))
            gate.release()
            await study.value
            let deadline = ContinuousClock.now + .seconds(5)
            while owner.review != nil, ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
            #expect(owner.navigation.review == nil && owner.review == nil && client.ends == 1)
        }
    }

    /// The core's two renders of the first card are the same payload, so the
    /// session can use the one prepared before it (`sched/review-session` C3, C6).
    @Test func thePeekedCardsRenderIsTheSessionsOwn() async throws {
        try await withOwner(realDecks: true) { owner, _, _ in
            let deck: Int64 = 1_788_249_600_212
            guard case .card(let peeked) = try await Core.peekFirstCard(deckID: deck) else {
                Issue.record("The fixture deck has no first card"); return
            }
            let early = try await Core.renderCard(id: peeked.cardId, mode: .review, platform: .mac)
            guard case .started(let session, _, let head, _) = try await Core.startSession(deckID: deck) else {
                Issue.record("The session did not start"); return
            }
            let first = try #require(head.items.first)
            #expect(StudyWarmup.sameCard(first, peeked))
            let rendered = try await Core.renderSessionHead(sessionID: session, keys: [first.renderKey], platform: .mac)
            #expect(rendered == [early])
            let sessionCard = try #require(rendered.first)
            #expect(CardPayload(early, audioPolicy: .init()) == CardPayload(sessionCard, audioPolicy: .init()))
            try await Core.endSession(sessionID: session)
        }
    }

    /// Study's own deck selection moves no card: the preload stays.
    @Test func selectingTheDeckKeepsThePreload() async throws {
        try await withOwner { owner, _, _ in
            owner.studyWarmup.peek = { _ in .card(item: ReviewCoordinatorTests.head(1).items[0]) }
            owner.studyWarmup.settleDelay = .seconds(60)
            owner.showStudyAction(deckID: 1, owner: UUID())
            await owner.studyWarmup.settled()
            owner.receive(Self.event(owner.generation, .changes(changes: .init(
                wholeCollection: false, source: .userNonUndoable, cards: [], notes: [], notetypes: [], decks: [], deckConfigs: [],
                configKeys: [.init(name: "curDeck", kind: .updated)], tags: [], revlogAdded: [], revlogRemoved: [],
                mediaAdded: [], mediaRemoved: [], skippedIncomingCards: [], skippedIncomingNotes: [],
                skippedIncomingDecks: [], collectionRow: false))))
            #expect(!owner.studyWarmup.stale && owner.studyWarmup.prepared?.deckID == 1)
            let resolver = try #require(await owner.mediaResolverForReview())
            let taken = try #require(owner.studyWarmup.take(deckID: 1, resolver: resolver))
            #expect(taken.preloaded)
        }
    }

    /// The warm-up before the click: a peek and a render, and nothing else.
    @Test func thePreloadReadsOnlyAndStudyTakesIt() async throws {
        try await withOwner { owner, client, surface in
            var peeks: [Int64] = [], renders: [Int64] = [], selections = 0
            owner.saveSelectedDeck = { [unowned owner] _ in selections += 1; return owner.generation }
            owner.studyWarmup.peek = { deck in peeks.append(deck); return .card(item: ReviewCoordinatorTests.head(1).items[0]) }
            owner.studyWarmup.render = { id in renders.append(id); return CardPayload(cardID: id, question: "Q", answer: "A") }
            let decks = owner.decks
            let screen = UUID()
            owner.showStudyAction(deckID: 1, owner: screen)
            await owner.studyWarmup.settled()
            #expect(peeks == [1] && renders == [1])
            #expect(surface.calls == ["load", "prepareFirst"] && surface.presented.isEmpty)
            #expect(owner.studyWarmup.prepared?.deckID == 1)
            // No session, selection, record, review or count change before Study.
            #expect(client.starts == 0 && selections == 0 && owner.review == nil && owner.navigation.review == nil)
            #expect(owner.decks == decks)
            #expect(!FileManager.default.fileExists(atPath: owner.configuration.collectionLocation.directory
                .appending(path: "meguri-review-session.json").path))
            await owner.startStudy(deckID: 1)
            // The session runs on the warm pages, loaded once.
            #expect(owner.review?.surface === surface && owner.review?.phase == .question)
            #expect(client.starts == 1 && surface.calls.filter { $0 == "prepareFirst" }.count == 1)
            await owner.finishReview()
            owner.hideStudyAction(owner: screen)
        }
    }

    /// Each change that can move the first card holds the preload back: Study
    /// right after it prepares its card as without one.
    @Test(arguments: Trigger.allCases)
    func aChangeHoldsThePreloadBack(trigger: Trigger) async throws {
        try await withOwner { owner, _, surface in
            owner.studyWarmup.peek = { _ in .card(item: ReviewCoordinatorTests.head(1).items[0]) }
            owner.studyWarmup.render = { id in CardPayload(cardID: id, question: "Q", answer: "A") }
            owner.studyWarmup.settleDelay = .seconds(60)
            let screen = UUID()
            owner.showStudyAction(deckID: 1, owner: screen)
            await owner.studyWarmup.settled()
            #expect(owner.studyWarmup.prepared?.deckID == 1 && !owner.studyWarmup.stale)
            switch trigger {
            case .localChange: owner.receive(Self.changes(owner.generation, source: .userUndoable))
            case .sync: owner.receive(Self.changes(owner.generation, source: .syncIncoming))
            case .optionsChange: owner.receive(Self.changes(owner.generation, source: .userUndoable, deckConfig: true))
            case .dayRollover: owner.receive(Self.event(owner.generation, .rolledOver(oldDay: 10, newDay: 11)))
            case .nextDay: owner.receive(Self.event(owner.generation, .nextDayAtChanged(nextDayAt: 1_900_000_000)))
            case .timeZone: owner.receive(Self.event(owner.generation, .timeZoneChanged(zoneId: "Asia/Tokyo")))
            case .countRefresh:
                // A learning card came due: the counts moved.
                owner.loadDeckTree = { [unowned owner] in
                    CountedDeckTree(generation: owner.generation, nextChangeAt: Int64.max, nodes: [
                        CountedDeckNode(id: 1, name: "Default", filtered: false, studyCollapsed: false, browserCollapsed: false,
                                        counts: CoreBridge.ReviewCounts(new: 1, learning: 1, review: 0), children: [])])
                }
                await owner.loadCounts()
            case .deckSwitch: owner.showStudyAction(deckID: 2, owner: screen)
            case .screenDisappears: owner.hideStudyAction(owner: screen)
            case .mediaArrived: owner.receive(Self.event(owner.generation, .mediaArrived(names: ["late.png"])))
            }
            let taken = owner.studyWarmup.take(deckID: 1, resolver: try #require(await owner.mediaResolverForReview()))
            let warm = try #require(taken)
            #expect(!warm.preloaded)
            #expect(warm.surface === surface && surface.calls.last == "discardFirst")
        }
    }

    enum Trigger: CaseIterable, Sendable {
        case localChange, sync, optionsChange, dayRollover, nextDay, timeZone, countRefresh, deckSwitch, screenDisappears, mediaArrived
    }

    // MARK: Support

    final class StartSurface: ReviewCoordinatorTests.Surface {}
    nonisolated final class Flag: @unchecked Sendable { var value = false }

    private static func event(_ generation: UInt64, _ kind: CoreEventKind) -> CoreBridge.CoreEvent {
        CoreBridge.CoreEvent(.init(seq: 1, generation: generation, causeSeq: nil, kind: kind))
    }

    private static func changes(_ generation: UInt64, source: OpSource, deckConfig: Bool = false) -> CoreBridge.CoreEvent {
        event(generation, .changes(changes: .init(
            wholeCollection: false, source: source,
            cards: deckConfig ? [] : [.init(id: 1, noteId: 1, kind: .updated, memoryStateOnly: false)],
            notes: [], notetypes: [], decks: [], deckConfigs: deckConfig ? [.init(id: 1, kind: .updated)] : [], configKeys: [], tags: [],
            revlogAdded: [], revlogRemoved: [], mediaAdded: [], mediaRemoved: [],
            skippedIncomingCards: [], skippedIncomingNotes: [], skippedIncomingDecks: [], collectionRow: false)))
    }

    private func withOwner(revealDelay: Duration = .seconds(60), realDecks: Bool = false,
                           _ body: (ReviewCollection, ReviewCoordinatorTests.Client, StartSurface) async throws -> Void) async throws {
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
        let client = ReviewCoordinatorTests.Client()
        var surfaces: [StartSurface] = []
        owner.makeReviewSurface = { _ in let surface = StartSurface(); surfaces.append(surface); return surface }
        owner.reviewSessionClient = client
        owner.reviewRevealDelay = revealDelay
        let tree = [CountedDeckNode(id: 1, name: "Default", filtered: false, studyCollapsed: false, browserCollapsed: false,
                                    counts: CoreBridge.ReviewCounts(new: 1, learning: 0, review: 0), children: []),
                    CountedDeckNode(id: 2, name: "Other", filtered: false, studyCollapsed: false, browserCollapsed: false,
                                    counts: CoreBridge.ReviewCounts(new: 1, learning: 0, review: 0), children: [])]
        if !realDecks {
            owner.loadDeckTree = { [unowned owner] in CountedDeckTree(generation: owner.generation, nextChangeAt: Int64.max, nodes: tree) }
            owner.saveSelectedDeck = { [unowned owner] _ in owner.generation }
        }
        owner.studyWarmup.settleDelay = .zero
        owner.studyWarmup.peek = { _ in .empty }
        owner.studyWarmup.render = { id in CardPayload(cardID: id, question: "Q", answer: "A") }
        do {
            await owner.start()
            try #require(owner.state == .ready)
            if owner.policy != .ok { await owner.enableFsrs() }
            try #require(owner.policy == .ok)
            await owner.studyWarmup.settled()
            // The warm pages, made when the collection opened.
            let surface = try #require(surfaces.first)
            try #require(owner.studyWarmup.surface === surface)
            surface.calls.removeAll()
            try await body(owner, client, surface)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }
}
}
