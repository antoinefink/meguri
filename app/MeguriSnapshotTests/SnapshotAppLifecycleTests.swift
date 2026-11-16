#if os(macOS)
import Testing

@MainActor @Suite(.timeLimit(.minutes(1)))
struct SnapshotAppLifecycleTests {
    private enum Failure: Error { case expected }

    @MainActor private final class Signal {
        private let event = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingOldest(1))
        func send() { event.continuation.yield(()); event.continuation.finish() }
        func wait() async { for await _ in event.stream { return } }
    }

    @Test
    func restoresBeforeNextAdmissionAfterFailureOrCancellation() async throws {
        for cancelOwner in [false, true] {
            try await exerciseRestorationBeforeNextAdmission(cancelOwner: cancelOwner)
        }
    }

    private func exerciseRestorationBeforeNextAdmission(cancelOwner: Bool) async throws {
        // Each case owns a synthetic guard; no window or application state.
        let guardOwner = SnapshotAppLifecycle()
        let entered = Signal(), release = Signal()
        let cancelledRequested = Signal(), survivorRequested = Signal()
        var state = "original"
        var order: [String] = []
        var cancelledWaiter: Task<Void, any Error>?
        let owner = Task { @MainActor in
            try await guardOwner.withExclusiveAccess {
                state = "owned"
                order.append("owner")
                defer {
                    state = "original"
                    order.append("restored")
                    // No suspension between cancellation and handoff: cover
                    // cancellation racing with successful queued admission.
                    if cancelOwner { cancelledWaiter?.cancel() }
                }
                entered.send()
                await release.wait()
                try Task.checkCancellation()
                throw Failure.expected
            }
        }
        await entered.wait()
        let cancelled = Task { @MainActor in
            cancelledRequested.send()
            try await guardOwner.withExclusiveAccess {
                order.append("cancelled body")
                Issue.record("A cancelled waiter entered its body")
            }
        }
        cancelledWaiter = cancelled
        await cancelledRequested.wait()
        let nextTask = Task { @MainActor in
            survivorRequested.send()
            try await guardOwner.withExclusiveAccess {
                #expect(state == "original")
                order.append("next")
                state = "next"
                defer { state = "original"; order.append("next restored") }
            }
        }
        await survivorRequested.wait()
        if cancelOwner {
            owner.cancel()
        } else {
            // A queued cancellation must finish while the owner is suspended,
            // without needing a release or admitting any other body.
            cancelled.cancel()
            switch await cancelled.result {
            case .success: Issue.record("Expected queued cancellation")
            case .failure(let error): #expect(error is CancellationError)
            }
        }
        #expect(state == "owned" && order == ["owner"])
        release.send()
        switch await owner.result {
        case .success: Issue.record("Expected owner failure or cancellation")
        case .failure(let error):
            if cancelOwner { #expect(error is CancellationError) }
            else { #expect(error is Failure) }
        }
        switch await cancelled.result {
        case .success: Issue.record("Expected cancelled admission")
        case .failure(let error): #expect(error is CancellationError)
        }
        try await nextTask.value
        #expect(state == "original")
        #expect(order == ["owner", "restored", "next", "next restored"])
        // Cancellation and a drained queue must also permit a later caller.
        try await guardOwner.withExclusiveAccess {
            #expect(state == "original")
            order.append("later")
        }
        #expect(order.last == "later")
    }
}
#endif
