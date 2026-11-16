#if os(macOS)
import Foundation

/// Snapshot tests that observe or change NSApp lifecycle share this owner.
/// Keep each body's deferred restoration inside the exclusive scope.
@MainActor
final class SnapshotAppLifecycle {
    static let shared = SnapshotAppLifecycle()

    private struct Waiter {
        let id: UUID
        let continuation: CheckedContinuation<Void, any Error>
    }
    private var owned = false
    private var waiters: [Waiter] = []

    func withExclusiveAccess(_ body: @MainActor () async throws -> Void) async throws {
        try await acquire()
        defer { release() }
        // Cancellation can race with a successful admission. We then own the
        // guard and must release it, without entering the cancelled body.
        try Task.checkCancellation()
        try await body()
    }

    private func acquire() async throws {
        try Task.checkCancellation()
        if !owned {
            owned = true
            return
        }
        let id = UUID()
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, any Error>) in
                // The cancellation callback may have reached this actor before
                // registration. Check again in the same turn as the enqueue.
                if Task.isCancelled {
                    continuation.resume(throwing: CancellationError())
                } else {
                    waiters.append(Waiter(id: id, continuation: continuation))
                }
            }
        } onCancel: {
            Task { @MainActor in self.cancel(id) }
        }
    }

    private func cancel(_ id: UUID) {
        guard let index = waiters.firstIndex(where: { $0.id == id }) else { return }
        waiters.remove(at: index).continuation.resume(throwing: CancellationError())
    }

    private func release() {
        if waiters.isEmpty {
            owned = false
        } else {
            // Transfer ownership before resuming; no new caller can overtake.
            waiters.removeFirst().continuation.resume()
        }
    }
}
#endif
