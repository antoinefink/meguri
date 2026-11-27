import Foundation

/// Deterministic rendezvous shared by Meguri provider ownership tests.
actor DeferredTestGate {
    enum ArrivalError: Error { case operationCompletedBeforeArrival }

    private var arrived = false
    private var opened = false
    private var terminalError: (any Error)?
    private var arrivals: [CheckedContinuation<Void, any Error>] = []
    private var releases: [CheckedContinuation<Void, Never>] = []

    func enter() async {
        arrived = true
        let waiting = arrivals
        arrivals.removeAll()
        for waiter in waiting { waiter.resume() }
        if !opened {
            await withCheckedContinuation { releases.append($0) }
        }
    }

    func waitForArrival() async throws {
        if arrived { return }
        if let terminalError { throw terminalError }
        try await withCheckedThrowingContinuation { arrivals.append($0) }
    }

    /// Completion belongs only to this operation's expected arrivals. An
    /// already reached gate remains valid while its body does deferred work.
    func operationSucceeded() -> Bool {
        if !arrived { failArrival(ArrivalError.operationCompletedBeforeArrival) }
        open()
        return arrived
    }

    func abort(_ error: any Error) {
        failArrival(error)
        open()
    }

    private func failArrival(_ error: any Error) {
        terminalError = error
        let waiting = arrivals
        arrivals.removeAll()
        for waiter in waiting { waiter.resume(throwing: error) }
    }

    func pendingWaiterCount() -> Int { arrivals.count + releases.count }

    func open() {
        opened = true
        let waiting = releases
        releases.removeAll()
        for waiter in waiting { waiter.resume() }
    }
}
