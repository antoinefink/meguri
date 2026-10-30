import Foundation
import Testing
@testable import CoreBridge

private final class BufferedNativeFailure: Error {}

struct CloudKitNetworkOperationTests {
    @Test func ownedTaskInvokesSDKAndBuffersInlineFailure() async throws {
        let accepted = CloudAcceptedNetworkCalls()
        let completion = CloudKitNetworkCompletion()
        let failure = BufferedNativeFailure()
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        let prepared = CloudKitNetworkEnqueue { validate in
            try lifetime.admit(cancelSDK: {}) {
                try lifetime.enqueue(generation: generation, validate: validate, begin: {})
                // Stand-in for an SDK invocation completing immediately.
                accepted.record()
                completion.complete(failure)
                try await completion.drain()
            }
        }
        let operation = try prepared.enqueue(validate: {})
        do { try await operation.drain(); Issue.record("native failure was bypassed") }
        catch { #expect((error as? BufferedNativeFailure) === failure) }
        #expect(accepted.value == 1)
    }

    @Test func cancellationRevokesPreparedWorkButAllowsLaterOriginalSDKOperation() throws {
        let lifetime = CloudKitNetworkLifetime()
        let original = lifetime.authority()
        let accepted = CloudAcceptedNetworkCalls()
        lifetime.invalidate()
        #expect(throws: CancellationError.self) {
            try lifetime.enqueue(generation: original, validate: {}) { accepted.record() }
        }
        #expect(accepted.value == 0)
        try lifetime.enqueue(generation: lifetime.authority(), validate: {}) { accepted.record() }
        #expect(accepted.value == 1)
        lifetime.retire()
        #expect(throws: CancellationError.self) {
            try lifetime.enqueue(generation: lifetime.authority(), validate: {}) { accepted.record() }
        }
        #expect(accepted.value == 1)
    }

    @Test func queuedTaskRevokedBeforeSDKEntryCancelsAndDrainsOriginal() async throws {
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        let queued = CloudCallbackGate()
        let accepted = CloudAcceptedNetworkCalls(), originalCancelled = CloudAcceptedNetworkCalls()
        let operation = try lifetime.admit(cancelSDK: {
            originalCancelled.record()
            await queued.open()
        }) {
            await queued.enter()
            try lifetime.enqueue(generation: generation, validate: {}, begin: {})
            accepted.record()
        }
        try await queued.waitForArrival()
        #expect(accepted.value == 0, "task admission cannot count as SDK invocation")
        await operation.cancelAndDrain()
        #expect(accepted.value == 0)
        #expect(originalCancelled.value == 1)
        #expect(await queued.pendingWaiterCount() == 0)
        operation.cancel()
        #expect(originalCancelled.value == 1, "completed handles cannot cancel later SDK work")
    }

    @Test func cancellationDuringInlineFailureStillDrainsOriginalSDKAndRetainsError() async throws {
        let lifetime = CloudKitNetworkLifetime()
        let completion = CloudKitNetworkCompletion(), failed = CloudCallbackGate(), cancelReturned = CloudCallbackGate()
        let failure = BufferedNativeFailure()
        let accepted = CloudAcceptedNetworkCalls(), originalCancelled = CloudAcceptedNetworkCalls()
        let operation = try lifetime.admit(cancelSDK: {
            originalCancelled.record()
            await cancelReturned.enter()
        }) {
            accepted.record()
            completion.complete(failure)
            await failed.enter()
            try await completion.drain()
        }
        try await failed.waitForArrival()
        operation.cancel()
        try await cancelReturned.waitForArrival()
        #expect(accepted.value == 1 && originalCancelled.value == 1)
        await failed.open()
        await cancelReturned.open()
        do { try await operation.drain(); Issue.record("inline SDK error was discarded by cancel") }
        catch { #expect((error as? BufferedNativeFailure) === failure) }
        #expect(await failed.pendingWaiterCount() == 0)
        #expect(await cancelReturned.pendingWaiterCount() == 0)
    }

    @Test func deadlineWinsBeforeQueuedQueryReturnsReadyAndPriorReadySurvivesDeadline() async throws {
        let deadlineWins = CloudPolicyReadinessDecision()
        let queuedQuery = CloudCallbackGate()
        let reply = Task {
            await queuedQuery.enter()
            return deadlineWins.observeReady()
        }
        try await queuedQuery.waitForArrival()
        #expect(deadlineWins.expire())
        await queuedQuery.open()
        #expect(!(await reply.value), "Ready only obtained after expiry cannot capture")
        #expect(!deadlineWins.observeReady(), "deadline-winning qualification stays terminal")
        let readyWins = CloudPolicyReadinessDecision()
        #expect(readyWins.observeReady())
        #expect(!readyWins.expire(), "genuinely observed readiness survives the later deadline")
        #expect(readyWins.observeReady())
    }

    @Test func actualSDKCancellationCode20KeepsErrorIdentityAnd14IsNotCancellation() {
        let underlying = BufferedNativeFailure()
        let cancelled = CloudKitSDKOperationFailure(underlying: underlying, code: 20, retryAfterMilliseconds: nil)
        let conflict = CloudKitSDKOperationFailure(underlying: underlying, code: 14, retryAfterMilliseconds: nil)
        #expect(cloudOperationWasCancelled(cancelled))
        #expect((cancelled.underlying as? BufferedNativeFailure) === underlying && cancelled.code == 20)
        #expect(!cloudOperationWasCancelled(conflict))
        #expect(cloudOperationWasCancelled(CancellationError()))
    }

}
