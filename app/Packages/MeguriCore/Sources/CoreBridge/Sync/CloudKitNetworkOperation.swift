import Dispatch
import Foundation
import Instrumentation
import Synchronization

/// A single deadline for P12.3 step 1; never a retry or polling clock.
public protocol CloudPolicyReadinessClock: Sendable { func waitForDeadline() async throws }
public struct ContinuousCloudPolicyReadinessClock: CloudPolicyReadinessClock {
    public init() {}
    public func waitForDeadline() async throws { try await ContinuousClock().sleep(for: .seconds(30)) }
}

/// Synchronous, instrumented FFI on the existing maintenance queue. This seam
/// is called outside SDK callbacks, from the owned operation task, never from that lane.
func cloudFinalCoreCall<T: Sendable>(_ name: BridgeCall, _ body: @Sendable () throws -> T) throws -> T {
    let queue = MaintenanceLane.shared.queue
    dispatchPrecondition(condition: .notOnQueue(queue))
    return try queue.sync {
        do { return try Signposts.bridge(name, lane: Lane.maintenance.rawValue, body) }
        catch { throw BridgeError(error) }
    }
}

/// Application preparation is complete. enqueue admits ONE retained operation
/// task; admission is neither an SDK invocation nor network acceptance/success.
/// The task must validate its pinned authority immediately before the public
/// async SDK call, without an intervening application await or owner lookup.
public struct CloudKitNetworkEnqueue: Sendable {
    private let begin: @Sendable (@escaping @Sendable () throws -> Void) throws -> CloudKitNetworkOperation
    public init(_ begin: @escaping @Sendable (@escaping @Sendable () throws -> Void) throws -> CloudKitNetworkOperation) { self.begin = begin }
    public func enqueue(validate: @escaping @Sendable () throws -> Void) throws -> CloudKitNetworkOperation { try begin(validate) }
}
public struct CloudKitNetworkOperation: Sendable {
    private let completion: @Sendable () async throws -> Void
    private let cancellation: @Sendable () -> Void
    public init(completion: @escaping @Sendable () async throws -> Void) {
        self.completion = completion; cancellation = {}
    }
    fileprivate init(task: Task<Void, any Error>, willCancel: @escaping @Sendable () -> Void,
                     cancelSDK: @escaping @Sendable () async -> Void, finished: @escaping @Sendable () -> Void) {
        let owner = CloudKitOwnedOperation(task: task, willCancel: willCancel, cancelSDK: cancelSDK)
        completion = {
            defer { finished() }
            try await owner.drain()
        }
        cancellation = { owner.cancel() }
    }
    public func cancel() { cancellation() }
    /// Cancellation joins the same task and original SDK, including a task that
    /// has not begun. Delegate/staging completion is never discarded.
    public func drain() async throws { try await completion() }
    public func cancelAndDrain() async { cancel(); _ = try? await drain() }
}

private final class CloudKitOwnedOperation: Sendable {
    private let task: Task<Void, any Error>
    private let willCancel: @Sendable () -> Void
    private let cancelSDK: @Sendable () async -> Void
    private struct State { var cancellation: Task<Void, Never>?; var finished = false }
    private let state = Mutex(State())
    init(task: Task<Void, any Error>, willCancel: @escaping @Sendable () -> Void,
         cancelSDK: @escaping @Sendable () async -> Void) {
        self.task = task; self.willCancel = willCancel; self.cancelSDK = cancelSDK
    }
    func cancel() {
        state.withLock { state in
            guard !state.finished else { return }
            task.cancel()
            guard state.cancellation == nil else { return }
            willCancel()
            let cancelSDK = self.cancelSDK
            state.cancellation = Task { await cancelSDK() }
        }
    }
    func drain() async throws {
        try await withTaskCancellationHandler {
            let result = await task.result
            let cancellation = state.withLock { state in
                if state.cancellation == nil { state.finished = true }
                return state.cancellation
            }
            if let cancellation { await cancellation.value }
            state.withLock { $0.finished = true }
            try result.get()
        } onCancel: { self.cancel() }
    }
}

/// Buffers even an inline SDK completion. Cancellation still drains that result.
final class CloudKitNetworkCompletion: Sendable {
    private struct State {
        var result: Result<Void, any Error>?
        var waiter: CheckedContinuation<Void, any Error>?
    }
    private let state = Mutex(State())
    func complete(_ error: (any Error)?) {
        let result: Result<Void, any Error> = error.map { .failure($0) } ?? .success(())
        let waiter = state.withLock { state in
            guard state.result == nil else { return Optional<CheckedContinuation<Void, any Error>>.none }
            state.result = result
            let waiter = state.waiter; state.waiter = nil
            return waiter
        }
        waiter?.resume(with: result)
    }
    func drain() async throws {
        try await withCheckedThrowingContinuation { continuation in
            let result: Result<Void, any Error>? = state.withLock { state in
                if let result = state.result { return result }
                state.waiter = continuation
                return nil
            }
            if let result { continuation.resume(with: result) }
        }
    }
}

/// Per-original-SDK authority and retained tasks. Core never calls native code
/// under its lock. The native lock is released before the async SDK invocation;
/// public CloudKit exposes no synchronous network linearization point.
final class CloudKitNetworkLifetime: Sendable {
    private struct State {
        var retired = false
        var cancelling = 0
        var eventActive = false
        var generation: UInt64 = 0
        var operations: [UUID: CloudKitNetworkOperation] = [:]
    }
    private let state = Mutex(State())
    func beginEvent() { state.withLock { $0.eventActive = true } }
    func endEvent() { state.withLock { $0.eventActive = false } }
    func authority() -> UInt64 { state.withLock { $0.generation } }
    func invalidate() { state.withLock { $0.generation &+= 1 } }
    func retire() { state.withLock { $0.retired = true; $0.generation &+= 1 } }
    /// Final task-body validation. begin is only synchronous instrumentation in
    /// fakes; it does not claim that CloudKit has accepted network work.
    func enqueue(generation: UInt64, validate: @Sendable () throws -> Void, begin: () -> Void) throws {
        try state.withLock { state in
            guard !state.retired, state.cancelling == 0, !state.eventActive, generation == state.generation else { throw CancellationError() }
            try Task.checkCancellation()
            try validate()
            try Task.checkCancellation()
            begin()
        }
    }
    func admit(cancelSDK: @escaping @Sendable () async -> Void,
               body: @escaping @Sendable () async throws -> Void) throws -> CloudKitNetworkOperation {
        try state.withLock { state in
            guard !state.retired, state.cancelling == 0 else { throw CancellationError() }
            try Task.checkCancellation()
            let id = UUID()
            let context = CloudMediaCancellationContext.current
            #if DEBUG
            let selectedAction = CloudMediaCancellationContext.selectedAction
            #endif
            let task = Task.detached(priority: Task.currentPriority) {
                try await CloudMediaCancellationContext.$current.withValue(context) {
                    #if DEBUG
                    try await CloudMediaCancellationContext.$selectedAction.withValue(selectedAction) {
                        try await body()
                    }
                    #else
                    try await body()
                    #endif
                }
            }
            let operation = CloudKitNetworkOperation(task: task,
                willCancel: { self.startCancellationFence() },
                cancelSDK: { await cancelSDK(); self.endCancellation() },
                finished: { self.state.withLock { _ = $0.operations.removeValue(forKey: id) } })
            state.operations[id] = operation
            return operation
        }
    }
    private func startCancellationFence() {
        state.withLock { $0.generation &+= 1; $0.cancelling += 1 }
    }
    func beginCancellation() -> [CloudKitNetworkOperation] {
        let operations = state.withLock { state in
            state.generation &+= 1; state.cancelling += 1
            return Array(state.operations.values)
        }
        // Never hold lifetime authority while entering a handle's own lock.
        for operation in operations { operation.cancel() }
        return operations
    }
    func endCancellation() { state.withLock { $0.cancelling -= 1 } }
}

/// Value facts from the actual SDK failure; classification remains in Core.
struct CloudKitSDKOperationFailure: Error {
    let underlying: any Error
    let code: Int64
    let retryAfterMilliseconds: Int64?
}

/// Preserve the real SDK error object/code while deriving cancellation semantics.
func cloudOperationWasCancelled(_ error: (any Error)?) -> Bool {
    error is CancellationError || (error as? CloudKitSDKOperationFailure)?.code == 20
}
