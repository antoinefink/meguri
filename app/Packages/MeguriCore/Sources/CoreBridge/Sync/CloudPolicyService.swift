import Foundation
import Instrumentation
internal import MeguriCoreBindings

/// A synchronous admission seam for already prepared coordinator work. It
/// never awaits, reads providers, performs UI work or calls back into core.
public protocol CloudPolicyQueue: Sendable { func enqueue() -> Bool }
private final class ForeignPolicyQueue: MeguriCoreBindings.CloudPolicyQueue {
    let queue: any CloudPolicyQueue
    init(_ queue: any CloudPolicyQueue) { self.queue = queue }
    func enqueue() -> Bool { queue.enqueue() }
}

// Preserve the ordering formerly supplied by Maintenance, even for separate
// facades over one retained policy. Only one facade job is outstanding; waiting
// callers suspend on continuations, with no worker waits or retained task chain.
actor CloudPolicyOrder {
    private var occupied = false
    private var waiting: [CheckedContinuation<Void, Never>] = []
    func acquire(queued: @Sendable () -> Void) async {
        if !occupied { occupied = true; return }
        await withCheckedContinuation { waiting.append($0); queued() }
    }
    func release() {
        if waiting.isEmpty { occupied = false }
        else { waiting.removeFirst().resume() }
    }
}

struct CloudPolicyJobValue<Value: Sendable>: Sendable {
    let handle: MeguriCoreBindings.JobHandle
    let read: @Sendable () throws -> Value?
}

/// Retains the original Host policy. Native control work must run outside SDK
/// callbacks, in the emitted order. No Swift policy or completion inference.
public actor CoreCloudPolicyService {
    private static let order = CloudPolicyOrder()
    private let raw: MeguriCoreBindings.CloudPolicy
    private let track: @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob
    private let onJobQueued: @Sendable () -> Void
    private let beforeJobStart: @Sendable (BridgeCall) async -> Void
    // Scheduling observation only; never supplies a Core result or authority.
    private let beforeIdentityProjection: @Sendable (UInt64) async -> Void
    private var readinessObservers: [UUID: AsyncStream<Void>.Continuation] = [:]
    init(_ raw: MeguriCoreBindings.CloudPolicy,
         beforeIdentityProjection: @escaping @Sendable (UInt64) async -> Void = { _ in },
         beforeJobStart: @escaping @Sendable (BridgeCall) async -> Void = { _ in },
         onJobQueued: @escaping @Sendable () -> Void = {},
         track: @escaping @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob = { try Jobs.track($0) }) {
        self.raw = raw; self.beforeIdentityProjection = beforeIdentityProjection
        self.beforeJobStart = beforeJobStart; self.track = track; self.onJobQueued = onJobQueued
    }

    nonisolated static func call<T: Sendable>(_ name: BridgeCall, _ body: @Sendable @escaping () throws -> T) async throws -> T {
        try await MaintenanceLane.shared.call(name, body)
    }

    private func run<Value: Sendable>(
        _ startName: BridgeCall, result resultName: BridgeCall,
        _ start: @escaping @Sendable () throws -> CloudPolicyJobValue<Value>
    ) async throws -> Value {
        try await Self.runOrdered(startName, result: resultName, beforeJobStart: beforeJobStart,
                                  onJobQueued: onJobQueued, track: track, start)
    }

    nonisolated static func runOrdered<Value: Sendable>(
        _ startName: BridgeCall, result resultName: BridgeCall,
        beforeJobStart: @Sendable (BridgeCall) async -> Void = { _ in },
        onJobQueued: @Sendable () -> Void = {},
        track: @escaping @Sendable (MeguriCoreBindings.JobHandle) throws -> CoreJob = { try Jobs.track($0) },
        retainResourceResultOnTrackingFailure: Bool = false,
        _ start: @escaping @Sendable () throws -> CloudPolicyJobValue<Value>
    ) async throws -> Value {
        await order.acquire(queued: onJobQueued)
        // Release on errors only after any admitted job has drained. Swift
        // cancellation does not remove a predecessor or drop its effects.
        do {
            await beforeJobStart(startName)
            try await CoreEventStream.shared.start()
            let started = try await Self.call(startName, start)
            let job: CoreJob
            do {
                job = try await Jobs.trackDraining(started.handle, setup: track)
            } catch {
                guard retainResourceResultOnTrackingFailure else { throw error }
                let setupError = error
                // Keep the actual resource result after drain.
                do {
                    if let value = try await Self.call(resultName, started.read) {
                        await Self.order.release()
                        return value
                    }
                } catch BridgeError.cancelled {
                    throw setupError
                }
                throw setupError
            }
            let outcome = await job.outcome()
            // Inspect the exact FFI error before lossy JobFinished conversion.
            // Result reads never consult the current collection owner.
            let value = try await Self.call(resultName, started.read)
            if let value { await Self.order.release(); return value }
            switch outcome {
            case .cancelled: throw BridgeError.cancelled
            case .failed(let error): throw error
            case .succeeded: throw BridgeError.internal("Policy job completed without its typed result")
            }
        } catch {
            await Self.order.release()
            throw error
        }
    }

    /// Original Host metadata only; available after Core suspend without
    /// admitting a policy job or waiting for the policy ordering permit.
    public func validateOriginalOwner() async throws {
        let raw = self.raw
        try await Self.call(.cloudPolicyValidateOriginalOwner) { [raw] in
            try raw.validateOriginalOwner()
        }
    }

    public func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        defer { notifyReadinessChanged() }
        let raw = self.raw
        let event = event.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await run(.cloudPolicyStartInput, result: .cloudPolicyOutputResult) { [raw] in
            let started = try raw.startInput(event: event)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    public func refreshAccount() async throws -> CloudPolicyOutput {
        defer { notifyReadinessChanged() }
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await run(.cloudPolicyStartRefreshAccount, result: .cloudPolicyOutputResult) { [raw] in
            let started = try raw.startRefreshAccount()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    func projectIdentityRead(_ request: UInt64) async throws -> CloudPolicyOutput {
        defer { notifyReadinessChanged() }
        await beforeIdentityProjection(request)
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await run(.cloudPolicyStartProjectIdentityRead, result: .cloudPolicyOutputResult) { [raw] in
            let started = try raw.startProjectIdentityRead(request: request)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    /// Latest applied Driver snapshot. Feed Refresh after core events and
    /// consume its commands before reading status; this read emits no work.
    public func status() async throws -> CloudPolicyStatus {
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPolicyStatus = try await run(.cloudPolicyStartStatus, result: .cloudPolicyStatusResult) { [raw] in
            let started = try raw.startStatus()
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyStatus(value)
    }
    public func executeCore(_ command: CloudPolicyCommand) async throws -> CloudPolicyOutput {
        defer { notifyReadinessChanged() }
        let raw = self.raw
        let command = command.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await run(.cloudPolicyStartExecuteCore, result: .cloudPolicyOutputResult) { [raw] in
            let started = try raw.startExecuteCore(command: command)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    /// Prepare before calling; enqueue synchronously during the callback;
    /// await completion afterwards. An accepted result means queue admission only.
    public func submitNative(_ command: CloudPolicyCommand, queue: any CloudPolicyQueue) async throws -> CloudPolicyAdmission {
        let raw = self.raw
        let command = command.raw
        let queue = ForeignPolicyQueue(queue)
        let value: MeguriCoreBindings.CloudPolicyAdmission = try await run(.cloudPolicyStartSubmitNative, result: .cloudPolicyAdmissionResult) { [raw] in
            let started = try raw.startSubmitNative(command: command, queue: queue)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyAdmission(value)
    }
    public func captureExchange(_ command: CloudPolicyCommand) async throws -> CloudPolicyCapture {
        let raw = self.raw
        let command = command.raw
        let value: MeguriCoreBindings.CloudPolicyCapture = try await run(.cloudPolicyStartCaptureExchange, result: .cloudPolicyCaptureResult) { [raw] in
            let started = try raw.startCaptureExchange(command: command)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyCapture(value)
    }
    public func completeExchange(_ exchange: UInt64) async throws -> CloudPolicyOutput {
        defer { notifyReadinessChanged() }
        let raw = self.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await run(.cloudPolicyStartCompleteExchange, result: .cloudPolicyOutputResult) { [raw] in
            let started = try raw.startCompleteExchange(exchange: exchange)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    public func endBeforeCapture(_ exchange: UInt64, outcome: CloudPolicyEarlyEnd) async throws -> CloudPolicyOutput {
        defer { notifyReadinessChanged() }
        let raw = self.raw
        let outcome = outcome.raw
        let value: MeguriCoreBindings.CloudPolicyOutput = try await run(.cloudPolicyStartEndBeforeCapture, result: .cloudPolicyOutputResult) { [raw] in
            let started = try raw.startEndBeforeCapture(exchange: exchange, outcome: outcome)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return CloudPolicyOutput(value)
    }
    /// Subscribe before querying to avoid losing a StartEngine/probe completion.
    func readinessChanges() -> AsyncStream<Void> {
        let id = UUID()
        return AsyncStream(bufferingPolicy: .bufferingNewest(1)) { continuation in
            readinessObservers[id] = continuation
            continuation.onTermination = { [weak self] _ in
                Task { await self?.removeReadinessObserver(id) }
            }
        }
    }
    private func removeReadinessObserver(_ id: UUID) { readinessObservers.removeValue(forKey: id) }
    public func notifyReadinessChanged() {
        for observer in readinessObservers.values { observer.yield(()) }
    }
    func exchangeReadiness(_ command: CloudPolicyCommand, nativeReady: Bool, expired: Bool = false) async throws -> MeguriCoreBindings.CloudPolicyReadiness {
        let raw = self.raw; let command = command.raw
        let value: MeguriCoreBindings.CloudPolicyReadiness = try await run(.cloudPolicyStartExchangeReadiness, result: .cloudPolicyReadinessResult) { [raw] in
            let started = try raw.startExchangeReadiness(command: command, expired: expired, nativeReady: nativeReady)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return value
    }
    func prepareExecution(_ command: CloudPolicyCommand) async throws -> CoreCloudPolicyExecution? {
        let raw = self.raw
        let command = command.raw
        let value: MeguriCoreBindings.CloudPolicyPreparedExecution = try await run(.cloudPolicyStartPrepareExecution, result: .cloudPolicyExecutionResult) { [raw] in
            let started = try raw.startPrepareExecution(command: command)
            return CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
        }
        return value.execution.map(CoreCloudPolicyExecution.init)
    }
}
