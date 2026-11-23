import Foundation
import Testing
@testable import AppModel

/// `sync/meguri-provider#MP6.10`: a review session's local changes wait for one
/// 60 s window, on a clock the test moves.
@MainActor @Suite struct MeguriReviewRoundsTests {
    /// Time the test moves; a sleep ends when its deadline has passed.
    @MainActor final class Clock {
        private(set) var now: Duration = .zero
        private var sleepers: [(deadline: Duration, id: UUID, resume: CheckedContinuation<Void, any Error>)] = []
        func sleep(_ duration: Duration) async throws {
            let id = UUID(), deadline = now + duration
            try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { sleepers.append((deadline, id, $0)) }
            } onCancel: {
                Task { @MainActor in self.cancel(id) }
            }
        }
        private func cancel(_ id: UUID) {
            guard let index = sleepers.firstIndex(where: { $0.id == id }) else { return }
            sleepers.remove(at: index).resume.resume(throwing: CancellationError())
        }
        func advance(by duration: Duration) async {
            await settle()
            now += duration
            let due = sleepers.filter { $0.deadline <= now }
            sleepers.removeAll { $0.deadline <= now }
            for sleeper in due { sleeper.resume.resume() }
            await settle()
        }
        func settle() async { for _ in 0..<20 { await Task.yield() } }
    }

    /// The rounds asked for; each one asked for starts at once unless `refuse`.
    @MainActor final class Rig {
        let clock = Clock()
        var rounds: [Duration] = []
        var refuse = false
        var model: MeguriReviewRounds!
        init() {
            model = MeguriReviewRounds { [unowned self] in
                guard !refuse else { return }
                rounds.append(clock.now)
                model.roundStarting()
            }
            let clock = self.clock
            model.sleep = { try await clock.sleep($0) }
        }
    }

    @Test(arguments: [1, 7, 30, 61])
    func answersInASessionCostAtMostOneRoundAMinutePlusTheEnd(answers: Int) async {
        let rig = Rig()
        rig.model.sessionStarted(5)
        for _ in 0..<answers {
            #expect(rig.model.localChange(), "an answer waits for the window")
            await rig.clock.advance(by: .seconds(7))
        }
        let duration = Double(answers * 7)
        let windowRounds = rig.rounds.count
        #expect(Double(windowRounds) <= (duration / 60).rounded(.up))
        // Each window opens at the first answer after a round and ends 60 s later,
        // seen at the next 7 s step: rounds at 63, 126, 189 … while answers go on.
        let expected = stride(from: 63, through: 7 * answers, by: 63).map { Duration.seconds($0) }
        #expect(rig.rounds == expected, "every window's round runs")
        // Rounds never come closer than the window.
        for (earlier, later) in zip(rig.rounds, rig.rounds.dropFirst()) { #expect(later - earlier >= .seconds(60)) }
        rig.model.sessionEnded(5)
        #expect(rig.rounds.count == windowRounds + 1, "the session's end always runs one")
        #expect(rig.rounds.last == rig.clock.now)
        #expect(!rig.model.waiting && !rig.model.windowOpen, "that round carries every answer")
        await rig.clock.advance(by: .seconds(120))
        #expect(rig.rounds.count == windowRounds + 1, "nothing fires afterwards")
    }

    @Test func theSessionsEndDoesNotWaitForTheWindow() async {
        let rig = Rig()
        rig.model.sessionStarted(5)
        rig.model.localChange()
        await rig.clock.advance(by: .seconds(10))
        #expect(rig.rounds.isEmpty && rig.model.windowOpen && rig.model.waiting)
        rig.model.sessionEnded(5)
        #expect(rig.rounds == [.seconds(10)] && !rig.model.windowOpen)
        await rig.clock.advance(by: .seconds(60))
        #expect(rig.rounds.count == 1, "the closed window's timer runs nothing")
    }

    @Test func leavingTheForegroundSendsWaitingChanges() async {
        let rig = Rig()
        rig.model.sessionStarted(5)
        #expect(!rig.model.flush(), "nothing waits: no round")
        rig.model.localChange()
        await rig.clock.advance(by: .seconds(20))
        #expect(rig.model.flush())
        #expect(rig.rounds == [.seconds(20)] && !rig.model.waiting)
        await rig.clock.advance(by: .seconds(60))
        #expect(rig.rounds.count == 1)
    }

    /// Sync Now (or a foreground or network-return round) starts at once and carries the waiting changes.
    @Test func anotherRoundClosesTheWindow() async {
        let rig = Rig()
        rig.model.sessionStarted(5)
        rig.model.localChange()
        await rig.clock.advance(by: .seconds(30))
        rig.model.roundStarting()
        #expect(!rig.model.waiting && !rig.model.windowOpen)
        await rig.clock.advance(by: .seconds(60))
        #expect(rig.rounds.isEmpty, "the window's own round no longer runs")
        rig.model.localChange()
        #expect(rig.model.windowOpen, "a later change opens a new window")
    }

    @Test func outsideASessionEveryChangeAsksAtOnce() async {
        let rig = Rig()
        #expect(!rig.model.localChange())
        #expect(!rig.model.localChange())
        #expect(rig.rounds.count == 2 && !rig.model.windowOpen)
        rig.model.sessionStarted(5)
        rig.model.sessionEnded(5)
        #expect(rig.rounds.count == 3)
        rig.model.localChange()
        #expect(rig.rounds.count == 4, "the session is over")
    }

    /// A round that cannot start (offline, signed out) leaves the changes waiting for the next trigger.
    @Test func aRefusedRoundKeepsTheChangesWaiting() async {
        let rig = Rig()
        rig.model.sessionStarted(5)
        rig.refuse = true
        rig.model.localChange()
        await rig.clock.advance(by: .seconds(60))
        #expect(rig.rounds.isEmpty && rig.model.waiting && !rig.model.windowOpen)
        rig.refuse = false
        rig.model.localChange()
        #expect(rig.model.windowOpen)
        rig.model.sessionEnded(5)
        #expect(rig.rounds.count == 1 && !rig.model.waiting)
    }

    @Test func theStatusStaysPendingWhileChangesWait() {
        #expect(SyncCoordinator.meguriRoutine(.idle, changesWaiting: true) == .pending)
        #expect(SyncCoordinator.meguriRoutine(.idle, changesWaiting: false) == .idle)
        for other: MeguriRoutineState in [.pending, .exchanging, .network, .authentication, .failed, .mediaBlocked] {
            #expect(SyncCoordinator.meguriRoutine(other, changesWaiting: true) == other, "a failure keeps its own row")
        }
        #expect(SyncCoordinator.meguriRoutine(nil, changesWaiting: true) == nil)
    }

    /// The background wait ends at its budget, at once when cancelled (a foreground
    /// return, the system's expiry), and never cancels the round itself.
    @Test(.timeLimit(.minutes(1))) func theRoundWaitIsBounded() async {
        let round = Task { _ = try? await Task.sleep(for: .seconds(30)) }
        defer { round.cancel() }
        let started = ContinuousClock.now
        await SyncCoordinator.wait(for: round, atMost: .milliseconds(200))
        #expect(started.duration(to: .now) < .seconds(5), "the budget ends the wait")
        #expect(!round.isCancelled)
        let waiting = Task { await SyncCoordinator.wait(for: round, atMost: .seconds(30)) }
        try? await Task.sleep(for: .milliseconds(50))
        let cancelledAt = ContinuousClock.now
        waiting.cancel(); await waiting.value
        #expect(cancelledAt.duration(to: .now) < .seconds(5), "cancelling ends the wait")
        #expect(!round.isCancelled)
        let quick = Task {}
        await SyncCoordinator.wait(for: quick, atMost: .seconds(30))
        await SyncCoordinator.wait(for: round, atMost: .zero)
    }

    @Test func theBackgroundRoundLeavesTimeForTheFlush() {
        let now: Int64 = 1_900_000_000_000
        #expect(SyncCoordinator.meguriBackgroundBudget(deadlineMilliseconds: now + 30_000, now: now) == .seconds(20))
        #expect(SyncCoordinator.meguriBackgroundBudget(deadlineMilliseconds: now + 12_000, now: now) == .seconds(7))
        #expect(SyncCoordinator.meguriBackgroundBudget(deadlineMilliseconds: now + 3_000, now: now) == .zero)
        #if os(macOS)
        #expect(SyncCoordinator.meguriBackgroundBudget(deadlineMilliseconds: nil, now: now) == .seconds(20))
        #else
        #expect(SyncCoordinator.meguriBackgroundBudget(deadlineMilliseconds: nil, now: now) == .zero)
        #endif
    }
}
