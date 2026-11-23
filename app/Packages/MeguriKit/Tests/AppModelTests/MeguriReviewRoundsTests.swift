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
