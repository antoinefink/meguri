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

    public init(workflow: CoreCloudWorkflowService, policy: CoreCloudPolicyService,
                factory: any CloudKitEngineFactory, adapter: CloudKitSyncAdapter) {
        self.workflow = workflow; self.policy = policy; self.factory = factory; self.adapter = adapter
    }
    public func begin(transition: Bool) async throws -> UInt64 {
        guard !closed, !frozen else { throw CancellationError() }
        replayPermit.revoke(); replayPermit = CloudAccountReplayPermit(); latest = nil
        let expected = epoch
        let id = try await workflow.beginIdentityRead(transition: transition)
        guard !closed, !frozen, expected == epoch else { throw CancellationError() }
        latest = id
        return id
    }
    public func read(_ id: UInt64) async -> CloudAppAccountReadResult {
        let complete = await prepareRead(id)
        return await complete()
    }
    /// Resolve outside the app input lane. The returned completion performs the
    /// actual Core proof and rechecks its original read/epoch at admission.
    public func prepareRead(_ id: UInt64) async -> @MainActor () async -> CloudAppAccountReadResult {
        let expected = epoch
        guard admits(id, epoch: expected) else { return { self.cancelled() } }
        let permit = replayPermit
        let resolved: Result<CloudResolvedAccount?, any Error>
        do { resolved = .success(try await factory.resolveAccount()) }
        catch { resolved = .failure(error) }
        return { [self] in await completeRead(id, epoch: expected, permit: permit, resolved: resolved) }
    }
    private func completeRead(_ id: UInt64, epoch expected: UInt64, permit: CloudAccountReplayPermit,
                              resolved: Result<CloudResolvedAccount?, any Error>) async -> CloudAppAccountReadResult {
        guard admits(id, epoch: expected) else { return cancelled() }
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
                return .init(outputs: [], error: BridgeError.unavailable(reason: "Cloud account proof unavailable"), verified: false)
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
        } catch {
            guard admits(id, epoch: expected) else { return cancelled() }
            // A projection/replay error must not complete the read again.
            guard !completionAttempted else { return .init(outputs: [], error: error, verified: false) }
            var setupFailure = false
            #if canImport(CloudKit)
            setupFailure = (error as? CKError).map {
                [.missingEntitlement, .badContainer, .permissionFailure, .managedAccountRestricted, .badDatabase].contains($0.code)
            } ?? false
            #endif
            let accepted = (try? await workflow.completeIdentityRead(id: id,
                status: setupFailure ? .configuration : .couldNotDetermine, identity: nil)) == true
            guard accepted, admits(id, epoch: expected) else { return cancelled() }
            // A failed resolver must not reverify from the cached provider.
            if setupFailure {
                do {
                    let output = try await policy.projectIdentityRead(id)
                    guard admits(id, epoch: expected), permit.isLive else { return cancelled() }
                    outputs.append(output)
                } catch {
                    guard admits(id, epoch: expected) else { return cancelled() }
                    return .init(outputs: [], error: error, verified: false)
                }
            }
            return .init(outputs: outputs, error: error, verified: false)
        }
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
        if committing > 0 { await withCheckedContinuation { waiters.append($0) } }
    }
    public func resumeAfterCoreResume() { if !closed { frozen = false } }
    public func close() async { closed = true; await freeze() }
}
