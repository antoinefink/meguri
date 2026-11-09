import Dispatch
import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

private func rawJobPolicy() async throws -> MeguriCoreBindings.CloudPolicy {
    let workflow = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
    return try await MaintenanceLane.shared.call(.cloudWorkflowPolicy) { try workflow.policy() }
}

private func retainCompletedPolicyResource<Value: Sendable>(
    _ startName: BridgeCall, result resultName: BridgeCall,
    attempted: @escaping @Sendable (UInt64) -> Void, message: String,
    _ started: CloudPolicyJobValue<Value>
) async throws -> Value {
    try await CoreCloudPolicyService.runOrdered(
        startName, result: resultName,
        track: { handle in
            attempted(handle.id)
            throw BridgeError.invalidArgument(message)
        }, retainResourceResultOnTrackingFailure: true
    ) { started }
}

private func policyJobActions(_ value: MeguriCoreBindings.CloudPolicyOutput) throws -> [String] {
    guard case .commands(let commands) = value else { throw BridgeError.internal("Expected policy outputs") }
    return commands.map { String(describing: $0.action) }
}

extension EventTests {
    @Test func policyOriginalOwnerMetadataSurvivesSuspendAndKeepsOrdinaryJobsCancelled() async throws {
        try await withCloudConnection { fixture in
            let policy = try await fixture.workflow.policy()
            try #require(try await fixture.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
            try await policy.validateOriginalOwner()
            await #expect(throws: BridgeError.cancelled) { _ = try await policy.status() }
            await #expect(throws: BridgeError.cancelled) { _ = try await policy.input(.foreground) }
            try await policy.validateOriginalOwner()
            try await fixture.collection.close()
            await #expect(throws: BridgeError.cloudPolicy(.closed)) {
                try await policy.validateOriginalOwner()
            }
        }
    }

    @Test func policyFacadeOrdersDelayedInputsAndKeepsCancelledTaskEffectsAndReadiness() async throws {
        try await withCloudConnection { _ in
            let hold = CloudCallbackGate()
            let (queued, didQueue) = AsyncStream.makeStream(of: Void.self)
            let first = Mutex(true)
            let raw = try await rawJobPolicy()
            let policy = CoreCloudPolicyService(raw, beforeJobStart: { name in
                let pause = first.withLock { value in let old = value; value = false; return old }
                if name == .cloudPolicyStartInput && pause { await hold.enter() }
            }, onJobQueued: { didQueue.yield(()) })
            let notifications = await policy.readinessChanges()
            let a = Task { try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false))) }
            var second: Task<CoreBridge.CloudPolicyOutput, any Error>?
            do {
                try await hold.waitForArrival()
                let b = Task {
                    defer { didQueue.finish() }
                    return try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false)))
                }
                second = b
                var waiting = queued.makeAsyncIterator()
                try #require(await waiting.next() != nil, "second input bypassed the ordering permit")
                a.cancel(); b.cancel()
                await hold.open()
                _ = try await a.value
                _ = try await b.value
                let status = try await policy.status()
                #expect(status.controls.userPaused == false, "second input must follow the delayed first input")
                var readiness = notifications.makeAsyncIterator()
                #expect(await readiness.next() != nil, "cancelled Swift tasks still notify readiness")
            } catch {
                await hold.open()
                _ = try? await a.value
                _ = try? await second?.value
                throw error
            }
            didQueue.finish()
        }
    }

    @Test func policyFacadeListenerFailureCancelsAndDrainsStartedWork() async throws {
        try await withCloudConnection { _ in
            let witness = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let raw = try await rawJobPolicy()
            let policy = CoreCloudPolicyService(raw, track: { handle in
                witness.withLock { $0 = handle }
                throw BridgeError.invalidArgument("policy listener setup")
            })
            do { _ = try await policy.input(.foreground); Issue.record("setup failure lost") }
            catch { #expect(error as? BridgeError == .invalidArgument("policy listener setup")) }
            let handle = try #require(witness.withLock { $0 })
            #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
            // A following facade call must acquire the permit after the drain.
            _ = try await CoreCloudPolicyService(raw).status()
        }
    }

    @Test func policyListenerFailureDrainsAnAlreadyDeliveredTerminal() async throws {
        try await withCloudConnection { _ in
            try await CoreEventStream.shared.start()
            let raw = try await rawJobPolicy()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyStartStatus) { try raw.startStatus() }
            // Observe real terminal delivery before trying listener setup. Its
            // unclaimed outcome must be consumed by the fallback drain waiter.
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            do {
                _ = try await Jobs.trackDraining(started.handle, setup: { _ in throw BridgeError.invalidArgument("early terminal setup") })
                Issue.record("setup failure lost")
            } catch { #expect(error as? BridgeError == BridgeError.invalidArgument("early terminal setup")) }
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            let value = try await MaintenanceLane.shared.call(.cloudPolicyStatusResult) { try started.result.result() }
            #expect(value != nil)
        }
    }

    @Test func policyRegistrationListenerFailureRetainsInstalledIdentity() async throws {
        try await withNativePolicyFixture { fixture in
            try await fixture.requireNativePolicyBaseline()
            let policy = try await fixture.core.policy()
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter)
            _ = try await consumePolicySetup(policy.input(.settings(.init(enabled: false, paused: false, automatic: false, media: false))), executor: executor)
            _ = try await consumePolicySetup(policy.refreshAccount(), executor: executor)
            _ = try await consumePolicySetup(policy.input(.network(reachable: true)), executor: executor)
            let output = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false)))
            guard case .commands(let commands) = output else {
                throw BridgeError.internal("Expected policy startup commands")
            }
            let command = try #require(commands.first { if case .startEngine = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartRegisterSdk) {
                try raw.startRegisterSdk()
            }
            // Real terminal delivery fixes the race at the already-installed
            // registration, before the injected listener setup error.
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let installed = try #require(try await MaintenanceLane.shared.call(.cloudPolicyRegistrationResult) {
                try started.result.result()
            })
            try await fixture.core.validateSDK(installed.instanceId)
            let attempted = Mutex<UInt64?>(nil)
            let retained: MeguriCoreBindings.CloudEngineRegistration = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartRegisterSdk, result: .cloudPolicyRegistrationResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "registration listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            #expect(retained.instanceId == installed.instanceId)
            #expect(retained.serialization == installed.serialization)
            #expect(retained.collectionZone == installed.collectionZone)
            #expect(retained.needsRootFetch == installed.needsRootFetch)
            try await fixture.core.validateSDK(retained.instanceId)
            try await execution.validate()
            // A following ordered call proves the drained registration released
            // its permit. Cleanup names only the recovered original identity.
            _ = try await policy.status()
            try await fixture.core.retireSDK(retained.instanceId, policyStop: false)
            try await fixture.core.releaseSDK(retained.instanceId)
            let successor = try await fixture.core.registerSDK(automatic: false)
            #expect(successor.instanceID != retained.instanceId)
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudPolicyRegistrationResult) {
                try started.result.result()
            })
            #expect(reread.instanceId == retained.instanceId)
            try await fixture.core.validateSDK(successor.instanceID)
            try await fixture.core.retireSDK(successor.instanceID, policyStop: false)
            try await fixture.core.releaseSDK(successor.instanceID)
        }
    }

    @Test func policyFetchAndSendListenerFailureRetainsCallIdentityAndCompletedNoCall() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected exchange commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let sdk = try #require(try await execution.originalSDK())
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let fetch = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginFetch) {
                try raw.startBeginFetch(zones: ["meguri-root", fixture.zone])
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == fetch.handle.id { break }
            }
            let fetchID = try #require(try await MaintenanceLane.shared.call(.cloudPolicyFetchResult) { try fetch.result.result() })
            let attemptedFetch = Mutex<UInt64?>(nil)
            let retainedFetch: UInt64 = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartBeginFetch, result: .cloudPolicyFetchResult,
                attempted: { id in attemptedFetch.withLock { $0 = id } }, message: "fetch listener setup",
                CloudPolicyJobValue(handle: fetch.handle, read: { try fetch.result.result() })
            )
            #expect(attemptedFetch.withLock { $0 } == fetch.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: fetch.handle.cancel).isCancelled())
            #expect(retainedFetch == fetchID && fetchID > 0)
            try await fixture.core.willFetch(sdk: sdk, group: "meguri-\(retainedFetch)", manual: true)
            try await fixture.core.didFetch(sdk: sdk, group: "meguri-\(retainedFetch)", manual: true)
            _ = try await fixture.core.finishFetch(sdk: sdk, id: retainedFetch)

            let send = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginSend) { try raw.startBeginSend() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == send.handle.id { break }
            }
            let sent = try #require(try await MaintenanceLane.shared.call(.cloudPolicySendResult) { try send.result.result() })
            let sendID = try #require(sent.call)
            let attemptedSend = Mutex<UInt64?>(nil)
            let retainedSend: MeguriCoreBindings.CloudPolicySendCall = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartBeginSend, result: .cloudPolicySendResult,
                attempted: { id in attemptedSend.withLock { $0 = id } }, message: "send listener setup",
                CloudPolicyJobValue(handle: send.handle, read: { try send.result.result() })
            )
            #expect(attemptedSend.withLock { $0 } == send.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: send.handle.cancel).isCancelled())
            #expect(retainedSend.call == sendID && sendID > 0)
            try await fixture.core.finishSend(sdk: sdk, id: sendID, pending: [:])

            _ = try await policy.captureExchange(command)
            let noCall = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartBeginSend) { try raw.startBeginSend() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == noCall.handle.id { break }
            }
            let attemptedNoCall = Mutex<UInt64?>(nil)
            let retainedNoCall: MeguriCoreBindings.CloudPolicySendCall = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartBeginSend, result: .cloudPolicySendResult,
                attempted: { id in attemptedNoCall.withLock { $0 = id } }, message: "no-call listener setup",
                CloudPolicyJobValue(handle: noCall.handle, read: { try noCall.result.result() })
            )
            #expect(attemptedNoCall.withLock { $0 } == noCall.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: noCall.handle.cancel).isCancelled())
            #expect(retainedNoCall.call == nil)
            #expect(try await execution.beginSend() == nil)
            _ = try await policy.status()
            try await execution.validate()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyFetchResult) { try fetch.result.result() } == retainedFetch)
            let rereadSend = try #require(try await MaintenanceLane.shared.call(.cloudPolicySendResult) { try send.result.result() })
            let rereadNoCall = try #require(try await MaintenanceLane.shared.call(.cloudPolicySendResult) { try noCall.result.result() })
            #expect(rereadSend.call == retainedSend.call && rereadNoCall.call == nil)
        }
    }

    @Test func policyStartedListenerFailureRetainsActualMultizoneOutputsAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected exchange commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let pendingBefore = try await policy.status().pending
            let added = try await CoreDeckService().addDeck(name: "Started listener pending deck")
            try await fixture.core.awaitTracking(generation: added.summary.generation, seq: added.summary.seq)
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartReportInvocation) {
                try raw.startReportInvocation(zones: [.root, .collection], kind: .fetch)
            }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            guard case .commands(let startedCommands) = actual else {
                throw BridgeError.internal("Expected Started outputs")
            }
            #expect(startedCommands.contains { command in
                if case .statusChanged(let status) = command.action { status.pending == pendingBefore + 1 } else { false }
            })
            let attempted = Mutex<UInt64?>(nil)
            let retained: MeguriCoreBindings.CloudPolicyOutput = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartReportInvocation, result: .cloudPolicyOutputResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "Started listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            // Compare full action payloads (not just command counts) from the
            // exact retained cell. No new projection is sampled as expected data.
            let expected = try policyJobActions(actual)
            let recovered = try policyJobActions(retained)
            #expect(!expected.isEmpty && recovered == expected)
            _ = try await policy.status() // The listener-failure drain released ordering.
            _ = try await execution.report(zones: [.root, .collection], kind: .fetch, started: false, error: CancellationError())
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            #expect(try policyJobActions(reread) == expected)
        }
    }

    @Test func policyFacadeKeepsTypedFailureBeforeGenericJobFailure() async throws {
        try await withCloudConnection { _ in
            let policy = CoreCloudPolicyService(try await rawJobPolicy())
            await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                _ = try await policy.projectIdentityRead(UInt64.max)
            }
        }
    }
}


extension EventTests {
    @Test func policyControlListenerFailureRetainsValidationAndExactMediaBooleansAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected exchange commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let validated = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartValidateControl) { try raw.startValidateControl() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == validated.handle.id { break }
            }
            let validationAttempt = Mutex<UInt64?>(nil)
            let validation: Bool = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartValidateControl, result: .cloudPolicyControlResult,
                attempted: { id in validationAttempt.withLock { $0 = id } }, message: "control listener setup",
                CloudPolicyJobValue(handle: validated.handle, read: { try validated.result.result() })
            )
            #expect(validation && validationAttempt.withLock { $0 } == validated.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: validated.handle.cancel).isCancelled())
            _ = try await policy.status()
            let stopped = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartContinueMediaAfterSend) { try raw.startContinueMediaAfterSend(code: 25) }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == stopped.handle.id { break }
            }
            let mediaAttempt = Mutex<UInt64?>(nil)
            let continued: Bool = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartContinueMediaAfterSend, result: .cloudPolicyMediaContinuationResult,
                attempted: { id in mediaAttempt.withLock { $0 = id } }, message: "media continuation listener setup",
                CloudPolicyJobValue(handle: stopped.handle, read: { try stopped.result.result() })
            )
            #expect(!continued && mediaAttempt.withLock { $0 } == stopped.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: stopped.handle.cancel).isCancelled())
            _ = try await policy.status()
            // The same issued validation retains its precise stale error,
            // including recovery after a listener setup failure.
            _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
            let stale = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartValidateControl) { try raw.startValidateControl() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == stale.handle.id { break }
            }
            let staleAttempt = Mutex<UInt64?>(nil)
            await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                let _: Bool = try await retainCompletedPolicyResource(
                    .cloudPolicyExecutionStartValidateControl, result: .cloudPolicyControlResult,
                    attempted: { id in staleAttempt.withLock { $0 = id } }, message: "stale control listener setup",
                    CloudPolicyJobValue(handle: stale.handle, read: { try stale.result.result() })
                )
            }
            #expect(staleAttempt.withLock { $0 } == stale.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: stale.handle.cancel).isCancelled())
            _ = try await policy.status()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyControlResult) { try validated.result.result() } == true)
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyMediaContinuationResult) { try stopped.result.result() } == false)
            await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                _ = try await CoreCloudPolicyService.call(.cloudPolicyControlResult) { try stale.result.result() }
            }
        }
    }

    @Test func policyMediaContinuationListenerFailureRetainsActualTrueAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            guard case .commands(let commands) = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true))) else {
                throw BridgeError.internal("Expected media commands")
            }
            let command = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let sdk = try #require(try await execution.originalSDK())
            _ = try await policy.input(.operationError(instance: sdk, operation: .init(zone: .media, kind: .send),
                error: .quotaFull, code: 25, retryAfterMilliseconds: nil))
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartContinueMediaAfterSend) { try raw.startContinueMediaAfterSend(code: 25) }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let attempted = Mutex<UInt64?>(nil)
            let continued: Bool = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartContinueMediaAfterSend, result: .cloudPolicyMediaContinuationResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "quota continuation listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(continued && attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            _ = try await policy.status()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            #expect(try await MaintenanceLane.shared.call(.cloudPolicyMediaContinuationResult) { try started.result.result() } == true)
        }
    }

    @Test func policyFlushListenerFailureRetainsActualRefreshOutputsAfterClose() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            _ = try await policy.input(.syncNowRequested(.command))
            guard case .commands(let commands) = try await policy.input(.background(deadlineMilliseconds: 1_900_000_000_000)) else {
                throw BridgeError.internal("Expected flush commands")
            }
            let command = try #require(commands.first { if case .backgroundFlush = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let pendingBefore = try await policy.status().pending
            let added = try await CoreDeckService().addDeck(name: "Flush listener pending deck")
            try await fixture.core.awaitTracking(generation: added.summary.generation, seq: added.summary.seq)
            let raw = execution.raw
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudPolicyExecutionStartPrepareFlush) { try raw.startPrepareFlush() }
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, _) = event.kind, id.rawValue == started.handle.id { break }
            }
            let actual = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            guard case .commands(let refreshed) = actual else { throw BridgeError.internal("Expected flush Refresh outputs") }
            #expect(refreshed.contains { command in
                if case .statusChanged(let status) = command.action { status.pending == pendingBefore + 1 } else { false }
            })
            let attempted = Mutex<UInt64?>(nil)
            let retained: MeguriCoreBindings.CloudPolicyOutput = try await retainCompletedPolicyResource(
                .cloudPolicyExecutionStartPrepareFlush, result: .cloudPolicyOutputResult,
                attempted: { id in attempted.withLock { $0 = id } }, message: "flush listener setup",
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            )
            #expect(attempted.withLock { $0 } == started.handle.id)
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            let expected = try policyJobActions(actual)
            let recovered = try policyJobActions(retained)
            #expect(!expected.isEmpty && recovered == expected)
            _ = try await policy.status()
            try await fixture.adapter.stop()
            try await fixture.collection.close()
            let reread = try #require(try await MaintenanceLane.shared.call(.cloudPolicyOutputResult) { try started.result.result() })
            #expect(try policyJobActions(reread) == expected)
        }
    }
}
