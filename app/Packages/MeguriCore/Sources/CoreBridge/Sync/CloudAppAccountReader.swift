import Foundation
import Synchronization
#if canImport(CloudKit)
import CloudKit
#endif

public struct CloudAppAccountReadResult: Sendable {
    public let outputs: [CloudPolicyOutput]
    public let error: (any Error)?
    public let verified: Bool
}

// Providers may ignore Task cancellation. Only this completion fence belongs
// to the read owner; the detached provider can deliver to this fence alone.
final class CloudAccountResolution: Sendable {
    enum Outcome: Sendable {
        case provider(Result<CloudResolvedAccount?, any Error>)
        case fence(taskCancellation: Bool)
        var result: Result<CloudResolvedAccount?, any Error> {
            switch self {
            case .provider(let result): result
            case .fence: .failure(CancellationError())
            }
        }
    }
    private struct State {
        var outcome: Outcome?
        var waiter: CheckedContinuation<Outcome, Never>?
    }
    private let state = Mutex(State())
    private let provider = Mutex<Task<Void, Never>?>(nil)
    func finish(_ result: Result<CloudResolvedAccount?, any Error>) {
        finishOutcome(.provider(result))
    }
    private func finishOutcome(_ outcome: Outcome) {
        let waiter = state.withLock { value in
            guard value.outcome == nil else { return Optional<CheckedContinuation<Outcome, Never>>.none }
            value.outcome = outcome
            let waiter = value.waiter; value.waiter = nil
            return waiter
        }
        waiter?.resume(returning: outcome)
    }
    func cancel() {
        cancel(taskCancellation: false)
    }
    private func cancel(taskCancellation: Bool) {
        finishOutcome(.fence(taskCancellation: taskCancellation))
        provider.withLock { $0 }?.cancel()
    }
    func resolve(using factory: any CloudKitEngineFactory) async throws -> CloudResolvedAccount? {
        try await resolveOutcome(using: factory).result.get()
    }
    func resolveOutcome(using factory: any CloudKitEngineFactory,
                        afterProviderLaunch: (@Sendable (Task<Void, Never>) async -> Void)? = nil) async -> Outcome {
        return await withTaskCancellationHandler {
            let task = Task.detached(priority: Task.currentPriority) { [self] in
                do { finish(.success(try await factory.resolveAccount())) }
                catch { finish(.failure(error)) }
            }
            // Internal scheduling seam; tests can hold the owner before install.
            if let afterProviderLaunch { await afterProviderLaunch(task) }
            provider.withLock { $0 = task }
            if state.withLock({ $0.outcome != nil }) { task.cancel() }
            defer { provider.withLock { $0 = nil } }
            // onCancel participates in the same first-result latch. Checking
            // Task cancellation here would overwrite an earlier provider error.
            return await withCheckedContinuation { continuation in
                let outcome = state.withLock { value -> Outcome? in
                    if let outcome = value.outcome { return outcome }
                    value.waiter = continuation
                    return nil
                }
                if let outcome { continuation.resume(returning: outcome) }
            }
        } onCancel: { self.cancel(taskCancellation: true) }
    }
}

/// A nonadmission return, issued only by its original prepared read. It cannot
/// complete Core work. Provider failures retain their exact original error.
public struct CloudAppAccountReadRefusal: Sendable {
    public let error: any Error
    private let preparation: CloudAppPreparedAccountRead
    fileprivate let generatedCancellation: Bool
    fileprivate init(error: any Error, preparation: CloudAppPreparedAccountRead, generatedCancellation: Bool) {
        self.error = error; self.preparation = preparation; self.generatedCancellation = generatedCancellation
    }
    @MainActor public func isGeneratedCancellation(from preparation: CloudAppPreparedAccountRead,
                                        reader: CloudAppAccountReader, read: UInt64) -> Bool {
        generatedCancellation && self.preparation === preparation && preparation.reader === reader && preparation.read == read
    }
}

@MainActor public final class CloudAppPreparedAccountRead {
    fileprivate let reader: CloudAppAccountReader
    fileprivate let read: UInt64
    private let outcome: CloudAccountResolution.Outcome
    private let completion: @MainActor () async -> CloudAppAccountReadResult
    private var completionStarted = false
    private var completionFailure: (any Error)?
    fileprivate init(reader: CloudAppAccountReader, read: UInt64, outcome: CloudAccountResolution.Outcome,
                     completion: @escaping @MainActor () async -> CloudAppAccountReadResult) {
        self.reader = reader; self.read = read; self.outcome = outcome; self.completion = completion
    }
    public func complete() async -> CloudAppAccountReadResult {
        completionStarted = true
        let result = await completion()
        if completionFailure == nil { completionFailure = result.error }
        return result
    }
    public func refuse() -> CloudAppAccountReadRefusal {
        // Once actual completion began, refusal cannot supply evidence that
        // there was no Core work. In particular it cannot hide its failure.
        if completionStarted {
            return .init(error: completionFailure ?? CancellationError(), preparation: self, generatedCancellation: false)
        }
        switch outcome {
        case .provider(.failure(let error)):
            return .init(error: error, preparation: self, generatedCancellation: false)
        case .fence(taskCancellation: false):
            return .init(error: CancellationError(), preparation: self, generatedCancellation: false)
        case .provider(.success), .fence(taskCancellation: true):
            return .init(error: CancellationError(), preparation: self, generatedCancellation: true)
        }
    }
}

private final class CloudAccountReplayPermit: Sendable {
    private let live = Mutex(true)
    func revoke() { live.withLock { $0 = false } }
    var isLive: Bool { live.withLock { $0 } }
}

/// The app's single account-read lane. Local resume only permits a fresh read;
/// original SDK delivery stays held until Core accepts that read's proof.
@MainActor
public final class CloudAppAccountReader {
    private let workflow: CoreCloudWorkflowService
    private let policy: CoreCloudPolicyService
    private let factory: any CloudKitEngineFactory
    private let adapter: CloudKitSyncAdapter
    private var frozen = false
    private var closed = false
    private var epoch: UInt64 = 0
    private var latest: UInt64?
    private var replayPermit = CloudAccountReplayPermit()
    private var committing = 0
    private var waiters: [CheckedContinuation<Void, Never>] = []
    private var resolutions: [UInt64: CloudAccountResolution] = [:]
    // Internal scheduling observation after the real fallback result, before admission.
    var afterFallbackIdentityCompletion: (@MainActor (Bool) async -> Void)?

    public init(workflow: CoreCloudWorkflowService, policy: CoreCloudPolicyService,
                factory: any CloudKitEngineFactory, adapter: CloudKitSyncAdapter) {
        self.workflow = workflow; self.policy = policy; self.factory = factory; self.adapter = adapter
    }
    public func begin(transition: Bool) async throws -> UInt64 {
        guard !closed, !frozen else { throw CancellationError() }
        for resolution in resolutions.values { resolution.cancel() }
        replayPermit.revoke(); replayPermit = CloudAccountReplayPermit(); latest = nil
        let expected = epoch
        let id = try await workflow.beginIdentityRead(transition: transition)
        guard !closed, !frozen, expected == epoch else { throw CancellationError() }
        latest = id
        await adapter.accountReadBegan(id: id)
        return id
    }
    public func read(_ id: UInt64) async -> CloudAppAccountReadResult {
        let complete = await prepareRead(id)
        return await complete()
    }
    /// Resolve outside the app input lane. The returned completion performs the
    /// actual Core proof and rechecks its original read/epoch at admission.
    public func prepareRead(_ id: UInt64) async -> @MainActor () async -> CloudAppAccountReadResult {
        let prepared = await prepareAccountRead(id)
        return { await prepared.complete() }
    }
    public func prepareAccountRead(_ id: UInt64) async -> CloudAppPreparedAccountRead {
        let expected = epoch
        guard admits(id, epoch: expected) else {
            return .init(reader: self, read: id, outcome: .fence(taskCancellation: false), completion: { self.cancelled() })
        }
        let permit = replayPermit
        let resolution = CloudAccountResolution()
        resolutions[id] = resolution
        defer { resolutions.removeValue(forKey: id) }
        let outcome = await resolution.resolveOutcome(using: factory)
        return .init(reader: self, read: id, outcome: outcome, completion: { [self] in
            await completeRead(id, epoch: expected, permit: permit, resolved: outcome.result)
        })
    }
    private func completeRead(_ id: UInt64, epoch expected: UInt64, permit: CloudAccountReplayPermit,
                              resolved: Result<CloudResolvedAccount?, any Error>) async -> CloudAppAccountReadResult {
        guard admits(id, epoch: expected) else {
            if case .failure(let error) = resolved { return .init(outputs: [], error: error, verified: false) }
            return cancelled()
        }
        committing += 1
        defer {
            committing -= 1
            if committing == 0 {
                let pending = waiters; waiters.removeAll()
                for waiter in pending { waiter.resume() }
            }
        }
        var outputs: [CloudPolicyOutput] = []
        var completionAttempted = false
        do {
            let account = try resolved.get()
            completionAttempted = true
            let accepted = try await workflow.completeIdentityRead(id: id,
                status: account?.status ?? .couldNotDetermine, identity: account?.identity)
            guard accepted, admits(id, epoch: expected) else { return cancelled() }
            // ReadFailed can be accepted while leaving the binding unverified.
            // Neither a cached status nor acceptance alone permits replay.
            guard let account, account.status != .couldNotDetermine,
                  account.status != .temporarilyUnavailable,
                  account.status != .available || account.identity != nil else {
                if account?.status == .temporarilyUnavailable {
                    // Accepted temporary status must stop policy as well as Core.
                    let output = try await policy.projectIdentityRead(id)
                    guard admits(id, epoch: expected), permit.isLive else { return cancelled() }
                    outputs.append(output)
                }
                return .init(outputs: outputs, error: BridgeError.unavailable(reason: "Cloud account proof unavailable"), verified: false)
            }
            let output = try await policy.projectIdentityRead(id)
            guard admits(id, epoch: expected), permit.isLive else { return cancelled() }
            let verified: Bool
            if account.status == .available, account.identity != nil {
                verified = await adapter.resumeDeliveries(ifPermitted: { permit.isLive })
            } else { verified = false }
            guard admits(id, epoch: expected), permit.isLive else { return cancelled() }
            outputs.append(output)
            return .init(outputs: outputs, error: nil, verified: verified)
        } catch let resolverError {
            guard admits(id, epoch: expected) else {
                // A newer read on this reader supersedes only successful resolutions.
                if case .success = resolved, let latest, latest != id { return cancelled() }
                return .init(outputs: outputs, error: resolverError, verified: false)
            }
            // A projection/replay error must not complete the read again.
            guard !completionAttempted else {
                await adapter.accountReadCompleted(id: id, available: false)
                return .init(outputs: [], error: resolverError, verified: false)
            }
            var setupFailure = false
            #if canImport(CloudKit)
            setupFailure = (resolverError as? CKError).map {
                [.missingEntitlement, .badContainer, .permissionFailure, .managedAccountRestricted, .badDatabase].contains($0.code)
            } ?? false
            #endif
            let accepted = (try? await workflow.completeIdentityRead(id: id,
                status: setupFailure ? .configuration : .couldNotDetermine, identity: nil)) == true
            if let afterFallbackIdentityCompletion { await afterFallbackIdentityCompletion(accepted) }
            guard accepted, admits(id, epoch: expected) else { return .init(outputs: outputs, error: resolverError, verified: false) }
            await adapter.accountReadCompleted(id: id, available: false)
            // A failed resolver must not reverify from the cached provider.
            if setupFailure {
                do {
                    let output = try await policy.projectIdentityRead(id)
                    guard admits(id, epoch: expected), permit.isLive else { return .init(outputs: outputs, error: resolverError, verified: false) }
                    outputs.append(output)
                } catch {
                    guard admits(id, epoch: expected) else { return .init(outputs: outputs, error: resolverError, verified: false) }
                    return .init(outputs: [], error: error, verified: false)
                }
            }
            return .init(outputs: outputs, error: resolverError, verified: false)
        }
    }
    /// The app calls this only after the read's policy outputs and follow-up
    /// projection have been reduced. Scheduling never joins an SDK on this lane.
    public func didProjectRead(_ id: UInt64) async {
        guard admits(id, epoch: epoch) else { return }
        await adapter.accountReadCompleted(id: id, available: true)
    }
    public func isCurrentRead(_ id: UInt64) -> Bool { admits(id, epoch: epoch) }
    private func admits(_ id: UInt64, epoch expected: UInt64) -> Bool {
        !frozen && !closed && !Task.isCancelled && epoch == expected && latest == id
    }
    private func cancelled() -> CloudAppAccountReadResult {
        #if DEBUG
        let error = CancellationError()
        let boundary: DebugCloudDiagnostics.Boundary
        if closed { boundary = .accountReaderClosed }
        else if frozen { boundary = .accountReaderFrozen }
        else if Task.isCancelled { boundary = .accountReaderTaskCancelled }
        else { boundary = .accountReaderInvalidated }
        DebugCloudDiagnostics.record(boundary, error: error)
        return .init(outputs: [], error: error, verified: false)
        #else
        .init(outputs: [], error: CancellationError(), verified: false)
        #endif
    }
    public func freeze() async {
        replayPermit.revoke()
        frozen = true; epoch &+= 1; latest = nil
        for resolution in resolutions.values { resolution.cancel() }
        if committing > 0 { await withCheckedContinuation { waiters.append($0) } }
    }
    public func resumeAfterCoreResume() { if !closed { frozen = false } }
    public func close() async { closed = true; await freeze() }
}
