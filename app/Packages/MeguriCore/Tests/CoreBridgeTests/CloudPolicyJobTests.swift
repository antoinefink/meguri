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
            } catch { #expect(error == .invalidArgument("early terminal setup")) }
            #expect(try await CoreBridge.CancelToken(raw: started.handle.cancel).isCancelled())
            let value = try await MaintenanceLane.shared.call(.cloudPolicyStatusResult) { try started.result.result() }
            #expect(value != nil)
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
