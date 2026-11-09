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
            let retained: MeguriCoreBindings.CloudEngineRegistration = try await CoreCloudPolicyService.runOrdered(
                .cloudPolicyExecutionStartRegisterSdk, result: .cloudPolicyRegistrationResult,
                track: { handle in
                    attempted.withLock { $0 = handle.id }
                    throw BridgeError.invalidArgument("registration listener setup")
                }, retainResourceResultOnTrackingFailure: true
            ) {
                CloudPolicyJobValue(handle: started.handle, read: { try started.result.result() })
            }
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

    @Test func policyFacadeKeepsTypedFailureBeforeGenericJobFailure() async throws {
        try await withCloudConnection { _ in
            let policy = CoreCloudPolicyService(try await rawJobPolicy())
            await #expect(throws: BridgeError.cloudPolicy(.staleCommand)) {
                _ = try await policy.projectIdentityRead(UInt64.max)
            }
        }
    }
}
