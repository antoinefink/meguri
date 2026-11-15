import Foundation
import Synchronization

/// Issued by the native owner after its original operation terminal and callback
/// bookkeeping, and after actual output/removal acknowledgements have drained.
/// It yields only account-proof admission; it completes no command or SDK work.
public final class CloudOriginalCommandProofYield: Sendable {
    @TaskLocal public static var admission: (@Sendable (CloudOriginalCommandProofYield) async throws -> Void)?
    public let id = UUID()
    private let completion = CloudKitNetworkCompletion()
    private let pending = Mutex(true)
    private let cancellationIntent: CloudPolicyCancellationContext?
    private let originalRevocation: (@Sendable (CloudPolicyCancellationContext.Snapshot) -> Void)?
    private let originalFailure: (@Sendable (any Error) -> Void)?
    init(cancellationIntent: CloudPolicyCancellationContext? = nil,
         originalRevocation: (@Sendable (CloudPolicyCancellationContext.Snapshot) -> Void)? = nil,
         originalFailure: (@Sendable (any Error) -> Void)? = nil) {
        self.cancellationIntent = cancellationIntent
        self.originalRevocation = originalRevocation
        self.originalFailure = originalFailure
    }
    public var isPending: Bool { pending.withLock { $0 } }
    /// Select the fenced read's exact failure before executing its policy outputs.
    /// This reaches the original native latch synchronously, without releasing
    /// admission or replacing an earlier cancellation/result at that latch.
    public func sealFailureBeforeOutputs(_ error: any Error) {
        pending.withLock { value in
            guard value else { return }
            originalFailure?(error)
        }
    }
    public func finish(_ error: (any Error)? = nil) {
        finish(error, beforeCompletion: {})
    }
    public func revoke(cancellation context: CloudPolicyCancellationContext) {
        finish(CancellationError()) {
            guard cancellationIntent === context,
                  let cause = context.captureRequestedCancellation() else { return }
            originalRevocation?(cause)
        }
    }
    private func finish(_ error: (any Error)?, beforeCompletion: () -> Void) {
        guard pending.withLock({ value in
            guard value else { return false }; value = false; return true
        }) else { return }
        beforeCompletion()
        completion.complete(error)
    }
    public func wait() async throws {
        try await withTaskCancellationHandler {
            try Task.checkCancellation()
            try await completion.drain()
            try Task.checkCancellation()
        } onCancel: { self.finish(CancellationError()) }
    }
}

/// One exact command is only joining a causal predecessor, with no unconsumed
/// Core/native/result/removal-ACK prefix of its own. It remains tracked. This
/// grants only account-proof admission, and revocation must join the ordered
/// lane before the command can enter Core or native work again.
public final class CloudQuiescentDependencyWait: Sendable {
    public enum Phase: Sendable { case park, revoke }
    @TaskLocal public static var transition: (@Sendable (CloudQuiescentDependencyWait, Phase) async throws -> Void)?
    public let id = UUID()
    private let orderedTransition: @Sendable (CloudQuiescentDependencyWait, Phase) async throws -> Void
    private struct State {
        var entered = false
        var revoked = false
        var tail: Task<Void, any Error>?
    }
    private let state = Mutex(State())
    private init(_ transition: @escaping @Sendable (CloudQuiescentDependencyWait, Phase) async throws -> Void) {
        orderedTransition = transition
    }

    // Retain the command transition before passing the wait to a separate task.
    // Cancellation and deadline tasks can then run without task-local context.
    static func retained() -> CloudQuiescentDependencyWait? {
        transition.map { CloudQuiescentDependencyWait($0) }
    }
    func enter() async throws {
        try Task.checkCancellation()
        let task = try state.withLock { state in
            guard !state.revoked else { throw CancellationError() }
            if let tail = state.tail { return tail }
            state.entered = true
            let task = Task { try await self.orderedTransition(self, .park) }
            state.tail = task
            return task
        }
        try await task.value
    }

    static func park() async throws -> CloudQuiescentDependencyWait? {
        let wait = retained()
        try await wait?.enter()
        return wait
    }
    func revoke() async throws {
        let task = state.withLock { state in
            if state.revoked { return state.tail }
            state.revoked = true
            guard state.entered else { return nil }
            let preceding = state.tail
            let task = Task {
                // Even a rejected/failed park must join its ordered removal.
                _ = try? await preceding?.value
                try await self.orderedTransition(self, .revoke)
            }
            state.tail = task
            return task
        }
        try await task?.value
    }
}
