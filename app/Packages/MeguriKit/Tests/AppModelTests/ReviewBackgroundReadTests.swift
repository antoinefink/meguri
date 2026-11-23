@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

/// A read nobody asked for (a sync round's rebuild, a deadline, a day check,
/// media arriving) runs under the card on screen: the answer area keeps its
/// controls as they were, and input given meanwhile is held, then given once
/// to the side it was given on (Antoine, 2026-11-22: Show Answer disappeared
/// and came back a second or two after the next card arrived).
extension ReviewCoordinatorTests {
    /// What the answer area draws and admits.
    struct Bar: Equatable, CustomTestStringConvertible {
        let phase: ReviewCoordinator.Phase
        let enabled: Bool
        let canReveal: Bool
        let canRate: Bool
        let canUndo: Bool
        let card: Int64?
        init(_ model: ReviewCoordinator) {
            phase = model.phase; enabled = !model.paused
            canReveal = model.canReveal; canRate = model.canRate; canUndo = model.canUndo
            card = model.item?.cardId
        }
        var testDescription: String {
            "\(phase) enabled=\(enabled) reveal=\(canReveal) rate=\(canRate) undo=\(canUndo) card=\(card.map(String.init) ?? "-")"
        }
    }

    enum Background: String, CaseIterable, CustomTestStringConvertible {
        /// A Meguri Sync round's merge commit: the session rebuilds and its
        /// head differs (`sched/review-session` C13, C17), with the same card first.
        case syncRebuild
        case currentCardMemoryOnly
        case currentCardEligible
        /// The head's `validUntil` (C7) or a day check: `requestRefresh()`.
        case deadline
        /// A terminal hint while the head is active (C17.1).
        case terminalHint
        case mediaArrival
        var testDescription: String { rawValue }
        var readsHead: Bool { self != .mediaArrival }
    }

    static func deliver(_ background: Background, to fixture: Fixture) {
        let model = fixture.model!
        let item = model.item?.cardId ?? 0
        switch background {
        case .syncRebuild:
            model.receive(CoreEvent(seq: 40, generation: 1, kind: .queueHeadChanged(sessionId: 5,
                diff: ReviewHeadDiff(head: fixture.client.currentHead, marks: [], removed: []))))
        case .currentCardMemoryOnly:
            model.receive(CoreEvent(seq: 41, generation: 1, kind: .currentCardChanged(sessionId: 5, cardId: item, change: .memoryOnly)))
        case .currentCardEligible:
            model.receive(CoreEvent(seq: 42, generation: 1, kind: .currentCardChanged(sessionId: 5, cardId: item, change: .eligible)))
        case .deadline:
            model.requestRefresh()
        case .terminalHint:
            model.receive(CoreEvent(seq: 43, generation: 1, kind: .waitingUntil(sessionId: 5, instant: 1_800_000_600)))
        case .mediaArrival:
            model.mediaArrived(["arrived.png"])
        }
    }

    /// Holds the background read at its first suspension, so the test sees
    /// what the screen draws while it runs.
    static func hold(_ background: Background, with gate: CancellableGate, in fixture: Fixture) {
        if background.readsHead {
            fixture.client.onHeadResult = { try await gate.wait() }
        } else {
            fixture.model.retryMedia = { _ in try await gate.wait(); return true }
        }
    }

    static func release(_ fixture: Fixture) {
        fixture.client.onHeadResult = nil
        fixture.model.retryMedia = { [surface = fixture.surface] names in surface.arrivals.append(names); return true }
    }

    enum Side: String, CaseIterable, CustomTestStringConvertible {
        case question, answer
        var testDescription: String { rawValue }
        var phase: ReviewCoordinator.Phase { self == .question ? .question : .answer }
    }

    /// Answer the first card; the next one is on its question side, or its answer side.
    static func nextCard(_ fixture: Fixture, side: Side) async {
        await fixture.model.start()
        fixture.model.reveal(); await fixture.model.settled()
        fixture.model.rate(.good); await fixture.model.settled()
        if side == .answer { fixture.model.reveal(); await fixture.model.settled() }
    }

    @Test(arguments: Background.allCases, Side.allCases)
    func backgroundReadKeepsTheAnswerAreaOfTheCardOnScreen(background: Background, side: Side) async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: side)
        let before = Bar(fixture.model), presented = fixture.surface.presented, reveals = fixture.surface.reveals
        #expect(before.phase == side.phase && before.card == 2)
        let gate = CancellableGate()
        defer { gate.release() }
        fixture.now += 1.5
        Self.hold(background, with: gate, in: fixture)
        Self.deliver(background, to: fixture)
        await gate.requested()
        #expect(Bar(fixture.model) == before, "while the read runs")
        gate.release(); await fixture.model.settled(); Self.release(fixture)
        #expect(Bar(fixture.model) == before, "once it is done")
        #expect(fixture.surface.presented == presented && fixture.surface.reveals == reveals, "the card is not presented again")
        #expect(fixture.client.headReads == (background.readsHead ? 1 : 0))
        #expect(await fixture.model.end())
    }

    enum HeldInput: String, CaseIterable, CustomTestStringConvertible {
        case reveal, rate, undo
        var testDescription: String { rawValue }
        var side: Side { self == .reveal ? .question : .answer }
    }

    @Test(arguments: HeldInput.allCases)
    func inputDuringABackgroundReadIsGivenOnceItEnds(input: HeldInput) async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: input.side)
        let reveals = fixture.surface.reveals, answers = fixture.client.answers
        let gate = CancellableGate()
        defer { gate.release() }
        Self.hold(.syncRebuild, with: gate, in: fixture)
        Self.deliver(.syncRebuild, to: fixture)
        await gate.requested()
        switch input {
        case .reveal: fixture.model.reveal(); fixture.model.reveal()
        case .rate: fixture.model.rate(.easy); fixture.model.rate(.again)
        case .undo: fixture.model.restore()
        }
        #expect(fixture.surface.reveals == reveals && fixture.client.answers == answers && fixture.client.restores.isEmpty,
                "nothing runs beside the read")
        gate.release(); await fixture.model.settled(); Self.release(fixture)
        switch input {
        case .reveal:
            #expect(fixture.surface.reveals == reveals + 1 && fixture.model.phase == .answer)
        case .rate:
            #expect(fixture.client.answers == answers + 1)
            #expect(fixture.client.used.last?.rating == .easy && fixture.client.used.last?.item.cardId == 2)
            #expect(fixture.model.record.answers.map(\.rating) == ["good", "easy"])
        case .undo:
            #expect(fixture.client.restores == [false])
        }
        #expect(await fixture.model.end())
    }

    @Test(arguments: Side.allCases)
    func inputHeldUnderAReadThatReplacesTheCardIsDropped(side: Side) async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: side)
        let reveals = fixture.surface.reveals, answers = fixture.client.answers
        let gate = CancellableGate()
        defer { gate.release() }
        Self.hold(.syncRebuild, with: gate, in: fixture)
        // The round brought a change: another card first, or the same card with another state.
        fixture.client.currentHead = side == .question ? Self.head(3) : Self.head(2, token: 9)
        Self.deliver(.syncRebuild, to: fixture)
        await gate.requested()
        if side == .question { fixture.model.reveal() } else { fixture.model.rate(.easy) }
        gate.release(); await fixture.model.settled(); Self.release(fixture)
        #expect(fixture.surface.reveals == reveals && fixture.client.answers == answers, "it was given to a card no longer shown")
        if side == .question {
            #expect(fixture.model.phase == .question && fixture.surface.presented.last == 3)
        } else {
            #expect(fixture.model.phase == .answer && fixture.model.canRate)
        }
        #expect(await fixture.model.end())
    }

    /// A session with sync rounds, deadlines, media and hints between its
    /// answers: the answer area changes only on the person's input and on a
    /// new card.
    @Test func answerAreaChangesOnlyOnInputAndCardChangesAcrossASession() async throws {
        let fixture = Fixture()
        await fixture.model.start()
        var expected = Bar(fixture.model)
        var changes: [String] = []
        func background(_ events: [Background]) async {
            for event in events {
                let gate = CancellableGate()
                Self.hold(event, with: gate, in: fixture)
                fixture.now += 1.5
                Self.deliver(event, to: fixture)
                await gate.requested()
                if Bar(fixture.model) != expected { changes.append("\(event) running: \(Bar(fixture.model).testDescription)") }
                gate.release(); await fixture.model.settled(); Self.release(fixture)
                if Bar(fixture.model) != expected { changes.append("\(event) done: \(Bar(fixture.model).testDescription)") }
            }
        }
        for card in Int64(1)...4 {
            #expect(fixture.model.item?.cardId == card && fixture.model.phase == .question)
            await background([.syncRebuild, .deadline, .mediaArrival, .currentCardMemoryOnly])
            fixture.model.reveal(); await fixture.model.settled()
            #expect(fixture.model.phase == .answer)
            expected = Bar(fixture.model)
            await background([.mediaArrival, .syncRebuild, .terminalHint, .currentCardEligible])
            let next = Self.head(card + 1)
            fixture.client.answerHead = .accepted(headDiff: .init(head: next, marks: [], removed: [card]),
                leech: false, suspended: false, rolledOver: false)
            fixture.model.rate(.good); await fixture.model.settled()
            expected = Bar(fixture.model)
            await background([.syncRebuild])
        }
        #expect(changes.isEmpty, "\(changes)")
        #expect(fixture.client.answers == 4 && fixture.surface.presented == [1, 2, 3, 4, 5])
        #expect(await fixture.model.end())
    }


    /// A read that draws the side on screen again (an edit synced from another
    /// device: a new render key) shows its new labels before any answer counts:
    /// an answer pressed while the page is replaced is refused (F3).
    @Test func answerPressedWhileTheSideIsDrawnAgainIsRefused() async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: .answer)
        let answers = fixture.client.answers
        let gate = CancellableGate()
        defer { gate.release() }
        fixture.client.currentHead = Self.head(2, token: 2, key: 7)
        fixture.surface.onReveal = { try await gate.wait() }
        Self.deliver(.currentCardEligible, to: fixture)
        await gate.requested()
        fixture.model.rate(.easy)
        gate.release(); await fixture.model.settled()
        fixture.surface.onReveal = nil
        #expect(fixture.client.answers == answers && fixture.model.phase == .answer)
        #expect(fixture.surface.presented.last == 2 && fixture.model.canRate)
        #expect(await fixture.model.end())
    }

    /// While a read presents another card, the answer area keeps the labels
    /// of the card still on screen.
    @Test func answerAreaKeepsTheShownCardsLabelsUntilTheNextCardIsPresented() async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: .answer)
        let gate = CancellableGate()
        defer { gate.release() }
        fixture.client.currentHead = Self.head(3)
        fixture.surface.onPresent = { try await gate.wait() }
        Self.deliver(.syncRebuild, to: fixture)
        await gate.requested()
        #expect(fixture.model.item?.cardId == 3, "the head is read")
        #expect(fixture.model.shownItem?.cardId == 2 && fixture.model.phase == .answer, "card 2 is still on screen")
        fixture.model.rate(.easy)
        gate.release(); await fixture.model.settled()
        fixture.surface.onPresent = nil
        #expect(fixture.model.shownItem?.cardId == 3 && fixture.model.phase == .question)
        #expect(fixture.client.answers == 1, "the press was for card 2")
        #expect(await fixture.model.end())
    }

    /// Edit Note's save is the person's step: the side is drawn again and the
    /// answers wait for it, rather than being held (E3, F3).
    @Test func savedEditWaitsForTheNewSide() async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: .answer)
        let answers = fixture.client.answers
        #expect(fixture.model.beginEditingNote())
        let gate = CancellableGate()
        defer { gate.release() }
        Self.hold(.syncRebuild, with: gate, in: fixture)
        fixture.model.endEditingNote(saved: true)
        await gate.requested()
        #expect(fixture.model.phase == .transition && !fixture.model.canRate)
        fixture.model.rate(.easy)
        gate.release(); await fixture.model.settled(); Self.release(fixture)
        #expect(fixture.client.answers == answers && fixture.model.phase == .answer)
        #expect(await fixture.model.end())
    }

    /// A press acts at once or not at all: one held behind a long read is dropped.
    @Test func inputHeldTooLongIsDropped() async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: .question)
        let reveals = fixture.surface.reveals
        let gate = CancellableGate()
        defer { gate.release() }
        Self.hold(.syncRebuild, with: gate, in: fixture)
        Self.deliver(.syncRebuild, to: fixture)
        await gate.requested()
        fixture.model.reveal()
        fixture.now += ReviewCoordinator.heldInputLimit + 1
        gate.release(); await fixture.model.settled(); Self.release(fixture)
        #expect(fixture.surface.reveals == reveals && fixture.model.phase == .question)
        #expect(await fixture.model.end())
    }

    /// A held Undo undoes the step that was named when it was pressed, or nothing.
    @Test func heldUndoNeedsTheSameStep() async throws {
        let fixture = Fixture()
        await Self.nextCard(fixture, side: .question)
        let gate = CancellableGate()
        defer { gate.release() }
        Self.hold(.syncRebuild, with: gate, in: fixture)
        Self.deliver(.syncRebuild, to: fixture)
        await gate.requested()
        fixture.model.restore()
        fixture.client.undoStep = (name: "Update Note", kind: .other)
        gate.release(); await fixture.model.settled(); Self.release(fixture)
        #expect(fixture.client.restores.isEmpty)
        #expect(await fixture.model.end())
    }

    /// `head(_:)` with another state token (and render key): the same card, changed elsewhere.
    static func head(_ id: Int64, token: UInt8, key: UInt8? = nil) -> ReviewHead {
        let base = head(id), item = base.items[0]
        let changed = ReviewHeadItem(cardId: item.cardId, noteId: item.noteId, kind: item.kind,
            stateToken: ReviewStateToken(bytes: Array(repeating: token, count: 32)),
            renderKey: key.map { ReviewRenderKey(bytes: Array(repeating: $0, count: 32)) } ?? item.renderKey,
            nextStates: item.nextStates, predictedNext: item.predictedNext, flag: item.flag, marked: item.marked,
            homePreset: item.homePreset)
        return ReviewHead(items: [changed], counts: base.counts, validUntil: base.validUntil, terminalState: base.terminalState)
    }
}
