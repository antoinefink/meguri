import Foundation
import Synchronization
import Testing
@testable import CoreBridge

private final class BufferedNativeFailure: Error {}

private enum NetworkAdmissionTaskLocals {
    // Synthetic ancestry only; this does not model a private SDK callback context.
    @TaskLocal static var callbackMarker: String?
}

private struct NetworkSDKEntryObservation: Sendable {
    let callbackMarker = NetworkAdmissionTaskLocals.callbackMarker
    let mediaContext = CloudMediaCancellationContext.current
    let priority = Task.currentPriority
    #if DEBUG
    let selectedAction = CloudMediaCancellationContext.selectedAction
    #endif
}

private final class NetworkSDKEntryObservationBox: Sendable {
    private let storage = Mutex<NetworkSDKEntryObservation?>(nil)

    func record() {
        let observation = NetworkSDKEntryObservation()
        storage.withLock { $0 = observation }
    }

    var value: NetworkSDKEntryObservation? { storage.withLock { $0 } }
}

private enum NetworkEntryObservationError: Error { case deadline }

private func drainNetworkEntryObservation(_ operation: CloudKitNetworkOperation,
                                          queued: CloudCallbackGate, observed: CloudCallbackGate) async throws {
    let watchdog = Task {
        let failure: any Error
        do {
            try await Task.sleep(for: .seconds(10))
            failure = NetworkEntryObservationError.deadline
        } catch { failure = error }
        operation.cancel()
        await queued.abort(failure)
        await observed.abort(failure)
    }
    try await withTaskCancellationHandler {
        do {
            try await queued.waitForArrival()
            await queued.open()
            try await observed.waitForArrival()
            await observed.open()
            try await operation.drain()
        } catch {
            await queued.abort(error)
            await observed.abort(error)
            watchdog.cancel()
            await watchdog.value
            await operation.cancelAndDrain()
            #expect(await queued.pendingWaiterCount() == 0)
            #expect(await observed.pendingWaiterCount() == 0)
            throw error
        }
        watchdog.cancel()
        await watchdog.value
        #expect(await queued.pendingWaiterCount() == 0)
        #expect(await observed.pendingWaiterCount() == 0)
    } onCancel: {
        operation.cancel()
        watchdog.cancel()
    }
}

struct CloudKitNetworkOperationTests {
    @Test(arguments: [false, true])
    func networkEntryObservationDeadlineDrainsEarlyCompletedOriginal(queuedArrived: Bool) async throws {
        let lifetime = CloudKitNetworkLifetime()
        let queued = CloudCallbackGate(), observed = CloudCallbackGate()
        let completed = CloudAcceptedNetworkCalls()
        await queued.open()
        let operation = try lifetime.admit(cancelSDK: {}) {
            if queuedArrived { await queued.enter() }
            completed.record()
        }
        try await operation.drain()
        #expect(completed.value == 1)
        do {
            try await drainNetworkEntryObservation(operation, queued: queued, observed: observed)
            Issue.record("missing arrival did not fail at the observation deadline")
        } catch { #expect(error is NetworkEntryObservationError) }
        #expect(await queued.pendingWaiterCount() == 0)
        #expect(await observed.pendingWaiterCount() == 0)
        try await operation.drain()
        #expect(completed.value == 1)
    }

    @Test func admittedTaskDropsArbitraryCallerMarkerAtSDKEntry() async throws {
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        let queued = CloudCallbackGate(), observed = CloudCallbackGate()
        let observation = NetworkSDKEntryObservationBox()
        let callerPriority = Task.currentPriority
        let operation = try NetworkAdmissionTaskLocals.$callbackMarker.withValue("synthetic-callback-marker") {
            try lifetime.admit(cancelSDK: {
                await queued.abort(CancellationError())
                await observed.abort(CancellationError())
            }) {
                await queued.enter()
                do {
                    try lifetime.enqueue(generation: generation, validate: {}, begin: {})
                    // Stand-in SDK entry in this original admitted body. The
                    // gate precedes final validation; admission is not acceptance.
                    observation.record()
                    await observed.enter()
                } catch {
                    await queued.abort(error)
                    await observed.abort(error)
                    throw error
                }
            }
        }
        // The caller's synthetic binding has ended before the body is released.
        try await drainNetworkEntryObservation(operation, queued: queued, observed: observed)
        let entry = try #require(observation.value)
        #expect(entry.callbackMarker == nil)
        #expect(entry.priority >= callerPriority,
                "caller priority must survive admission; escalation is allowed")
    }

    @Test func admittedTaskPreservesMediaContextIdentityAndCallerPriorityAtSDKEntry() async throws {
        // A high-priority caller makes lost priority visible even when a naive
        // detached task would otherwise happen to use the test's default priority.
        let caller = Task(priority: .high) {
            let lifetime = CloudKitNetworkLifetime()
            let generation = lifetime.authority()
            let queued = CloudCallbackGate(), observed = CloudCallbackGate()
            let observation = NetworkSDKEntryObservationBox()
            let context = CloudMediaCancellationContext(sdk: "synthetic-admission-context")
            let callerPriority = Task.currentPriority
            let operation = try CloudMediaCancellationContext.$current.withValue(context) {
                try lifetime.admit(cancelSDK: {
                    await queued.abort(CancellationError())
                    await observed.abort(CancellationError())
                }) {
                    await queued.enter()
                    do {
                        try lifetime.enqueue(generation: generation, validate: {}, begin: {})
                        // Stand-in SDK entry, before any drain can escalate it.
                        observation.record()
                        await observed.enter()
                    } catch {
                        await queued.abort(error)
                        await observed.abort(error)
                        throw error
                    }
                }
            }
            // Rebinding in production must retain this exact application object
            // after the caller's TaskLocal scope has ended.
            try await drainNetworkEntryObservation(operation, queued: queued, observed: observed)
            return (callerPriority: callerPriority, context: context, observation: observation.value)
        }
        let result = try await withTaskCancellationHandler {
            try await caller.value
        } onCancel: { caller.cancel() }
        let entry = try #require(result.observation)
        #expect(result.callerPriority >= .high)
        #expect(entry.mediaContext === result.context)
        #expect(entry.priority >= result.callerPriority,
                "caller priority must survive admission; escalation is allowed")
    }

    #if DEBUG
    @Test func admittedTaskPreservesSelectedActionAtSDKEntry() async throws {
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        let queued = CloudCallbackGate(), observed = CloudCallbackGate()
        let observation = NetworkSDKEntryObservationBox()
        let action = CloudPolicyAction.requestFetch(zones: [.collection, .media])
        let operation = try CloudMediaCancellationContext.$selectedAction.withValue(action) {
            try lifetime.admit(cancelSDK: {
                await queued.abort(CancellationError())
                await observed.abort(CancellationError())
            }) {
                await queued.enter()
                do {
                    try lifetime.enqueue(generation: generation, validate: {}, begin: {})
                    observation.record()
                    await observed.enter()
                } catch {
                    await queued.abort(error)
                    await observed.abort(error)
                    throw error
                }
            }
        }
        try await drainNetworkEntryObservation(operation, queued: queued, observed: observed)
        let entry = try #require(observation.value)
        // Match the concrete case and its associated values without
        // requiring CloudPolicyAction to conform to Equatable.
        if case .some(.requestFetch(let zones)) = entry.selectedAction {
            #expect(zones == [.collection, .media])
        } else {
            Issue.record("selected requestFetch action was lost or changed at SDK entry")
        }
    }
    #endif

    @Test func ownedTaskInvokesSDKAndBuffersInlineFailure() async throws {
        let accepted = CloudAcceptedNetworkCalls()
        let completion = CloudKitNetworkCompletion()
        let failure = BufferedNativeFailure()
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        let prepared = CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: {}) {
                try await prepare()
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

    @Test(arguments: ["cancel", "invalidate", "retire", "failure"])
    func asyncPreparationHeldRejectsSDKEntryAndDrainsOriginal(transition: String) async throws {
        let lifetime = CloudKitNetworkLifetime()
        let generation = lifetime.authority()
        let held = CloudCallbackGate()
        let accepted = CloudAcceptedNetworkCalls(), cancelled = CloudAcceptedNetworkCalls()
        let failure = BufferedNativeFailure()
        let prepared = CloudKitNetworkEnqueue { prepare, validate in
            try lifetime.admit(cancelSDK: { cancelled.record(); await held.open() }) {
                try await prepare()
                try lifetime.enqueue(generation: generation, validate: validate, begin: {})
                accepted.record()
            }
        }
        let operation = try prepared.enqueue(prepare: {
            await held.enter()
            if transition == "failure" { throw failure }
        }, validate: {})
        let deadline = CloudPolicyReadinessDecision()
        let watchdog = Task {
            do { try await Task.sleep(for: .seconds(10)) } catch { return }
            if deadline.expire() { await held.abort(NetworkEntryObservationError.deadline); operation.cancel() }
        }
        var caught: (any Error)?
        do {
            try await held.waitForArrival()
            #expect(accepted.value == 0)
            switch transition {
            case "cancel": operation.cancel()
            case "invalidate": lifetime.invalidate()
            case "retire": lifetime.retire()
            default: break
            }
            await held.open()
            do { try await operation.drain(); Issue.record("held preparation reached SDK entry") }
            catch { caught = error }
        } catch { caught = error }
        await held.open()
        await operation.cancelAndDrain()
        let inTime = deadline.observeReady()
        watchdog.cancel(); await watchdog.value
        #expect(inTime && accepted.value == 0)
        #expect(await held.pendingWaiterCount() == 0)
        if transition == "failure" { #expect((caught as? BufferedNativeFailure) === failure) }
        else { #expect(caught is CancellationError) }
        if transition == "cancel" { #expect(cancelled.value == 1) }
        if transition != "retire" {
            let later = try lifetime.admit(cancelSDK: {}) {
                try lifetime.enqueue(generation: lifetime.authority(), validate: {}, begin: {})
                accepted.record()
            }
            try await later.drain()
            #expect(accepted.value == 1)
            operation.cancel()
            #expect(accepted.value == 1, "the drained UUID cannot cancel later work")
        }
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
