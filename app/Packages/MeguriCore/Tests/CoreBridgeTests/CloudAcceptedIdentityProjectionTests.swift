import CloudKit
import Foundation
import Instrumentation
import MeguriCoreBindings
import Testing
@testable import CoreBridge

/// Holds the real policy actor call after Core accepted the actual read, before
/// its maintenance-lane projection. It supplies no identity, ACK or Core result.
private func projectionPolicy(_ gate: CloudCallbackGate) async throws -> CoreCloudPolicyService {
    let workflow = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
    let raw = try await MaintenanceLane.shared.call(.cloudWorkflowPolicy) { try workflow.policy() }
    return CoreCloudPolicyService(raw, beforeIdentityProjection: { _ in await gate.enter() })
}

enum ProjectionInvalidation: Sendable { case refreeze, appClose, close, generation }

extension EventTests {
    @Test func acceptedTemporaryAccountReadProjectsStopAndAllowsLaterMatchingRead() async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let original = try await f.engine()
            let before = try await f.core.durableSnapshot()
            #expect(try await policy.status().syncNow == .available)
            await f.factory.appendAccountRead(.success(.init(status: .temporarilyUnavailable, identity: nil)))
            let id = try await reader.begin(transition: true)
            let prepared = await reader.prepareAccountRead(id)
            let temporary = await prepared.complete()
            #expect(temporary.error as? BridgeError == .unavailable(reason: "Cloud account proof unavailable"))
            #expect(!temporary.verified)
            try #require(temporary.outputs.count == 1)
            let refusal = await prepared.refuse()
            #expect(refusal.error as? BridgeError == temporary.error as? BridgeError)
            let commands = temporary.outputs.flatMap { output -> [CoreBridge.CloudPolicyCommand] in
                if case .commands(let commands) = output { return commands }; return []
            }
            #expect(commands.contains { if case .stopEngine = $0.action { true } else { false } })
            for output in temporary.outputs { try await consumePolicySetup(output, executor: executor) }
            let stopped = try await policy.status()
            #expect(stopped.accountStatus == .temporary)
            if case .unavailable = stopped.syncNow {} else { Issue.record("Temporary account must disable Sync Now") }
            #expect(stopped.attention[.isolated, default: 0] == 0)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await original.recordedTrace().contains(.released))
            await #expect(throws: (any Error).self) { try await f.core.validateSDK(original.instanceID) }
            #expect(try await f.core.durableSnapshot().binding == before.binding)

            await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            let restoredID = try await reader.begin(transition: true)
            #expect(restoredID == id + 1)
            let restored = await reader.read(restoredID)
            #expect(restored.error == nil)
            try #require(restored.outputs.count == 1)
            for output in restored.outputs { try await consumePolicySetup(output, executor: executor) }
            await reader.didProjectRead(restoredID)
            let available = try await policy.status()
            #expect(available.accountStatus == .available && available.accountMatchesBinding)
            #expect(available.syncNow == .available)
            let retained = await f.adapter.retainedInstanceIDs()
            let current = try #require(retained.first)
            #expect(retained.count == 1)
            try await f.core.validateSDK(current)
            #expect(try await f.core.durableSnapshot().binding == before.binding)
            await reader.close()
        }
    }

    @Test func undeterminedAndLaterFailedReadsDoNotProjectTemporaryStatus() async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let original = try await f.engine()
            // Each read follows an already verified account in this process.
            for status in [CoreBridge.CloudAccountStatus.couldNotDetermine, .available] {
                await f.factory.appendAccountRead(.success(.init(status: status, identity: nil)))
                let id = try await reader.begin(transition: false)
                let result = await reader.read(id)
                #expect(result.error as? BridgeError == .unavailable(reason: "Cloud account proof unavailable"))
                #expect(result.outputs.isEmpty && !result.verified)
                #expect(try await policy.status().accountStatus == .available)
                try await f.core.validateSDK(original.instanceID)
            }
            let error = BridgeError.internal("synthetic-later-read-failure")
            await f.factory.appendAccountRead(.failure(error))
            let failedID = try await reader.begin(transition: false)
            let failed = await reader.read(failedID)
            #expect(failed.error as? BridgeError == error)
            #expect(failed.outputs.isEmpty && !failed.verified)
            #expect(try await policy.status().accountStatus == .available)
            try await f.core.validateSDK(original.instanceID)
            await reader.close()
        }
    }

    @Test(arguments: ["stale", "cancelled", "frozen"])
    func temporaryAccountReadRefusesInvalidPreparation(invalidation: String) async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            await f.factory.appendAccountRead(.success(.init(status: .temporarilyUnavailable, identity: nil)))
            let id = try await reader.begin(transition: false)
            let prepared = await reader.prepareAccountRead(id)
            if invalidation == "stale" { _ = try await reader.begin(transition: false) }
            if invalidation == "frozen" { await reader.freeze() }
            let completion = Task {
                if invalidation == "cancelled" { withUnsafeCurrentTask { $0?.cancel() } }
                return await prepared.complete()
            }
            let refused = await completion.value
            #expect(refused.error is CancellationError)
            #expect(refused.outputs.isEmpty && !refused.verified)
            #expect(try await policy.status().accountStatus == .available)
            await #expect(throws: (any Error).self) { try await policy.projectIdentityRead(id) }
            await reader.close()
        }
    }

    @Test func closeDuringSetupFallbackProjectionRetainsResolverFailure() async throws {
        try await withNativePolicyFixture { f in
            let projection = CloudCallbackGate()
            let policy = try await projectionPolicy(projection)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            await f.factory.appendAccountRead(.failure(CKError(.permissionFailure)))
            let id = try await reader.begin(transition: true)
            let prepared = await reader.prepareAccountRead(id)
            let completion = Task {
                let result = await prepared.complete()
                _ = await projection.operationSucceeded()
                return result
            }
            do {
                try await projection.waitForArrival()
                await Task { @MainActor in
                    let release = Task { await projection.open() }
                    await reader.close()
                    await release.value
                }.value
                let result = await completion.value
                #expect((result.error as? CKError)?.code == .permissionFailure)
                #expect(result.outputs.isEmpty && !result.verified)
                #expect(!(await reader.isCurrentRead(id)))
                let refusal = await prepared.refuse()
                #expect((refusal.error as? CKError)?.code == .permissionFailure)
                #expect(!(await refusal.isGeneratedCancellation(from: prepared, reader: reader, read: id)))
            } catch {
                await projection.open(); _ = await completion.value
                await reader.close()
                throw error
            }
        }
    }

    @Test func staleSetupFallbackProjectionAfterCloseRetainsResolverFailure() async throws {
        try await withNativePolicyFixture { f in
            let projection = CloudCallbackGate()
            let policy = try await projectionPolicy(projection)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            await f.factory.appendAccountRead(.failure(CKError(.permissionFailure)))
            let id = try await reader.begin(transition: true)
            let prepared = await reader.prepareAccountRead(id)
            let completion = Task {
                let result = await prepared.complete()
                _ = await projection.operationSucceeded()
                return result
            }
            do {
                try await projection.waitForArrival()
                // Invalidate the real fallback projection in Core while the
                // original reader still admits this read, then close it.
                let newer = try await f.core.beginIdentityRead(transition: true)
                #expect(await reader.isCurrentRead(id))
                await Task { @MainActor in
                    let release = Task { @MainActor in await projection.open() }
                    await reader.close()
                    await release.value
                }.value
                let result = await completion.value
                #expect((result.error as? CKError)?.code == .permissionFailure)
                #expect(result.outputs.isEmpty && !result.verified)
                #expect(!(await reader.isCurrentRead(id)))
                let refusal = await prepared.refuse()
                #expect((refusal.error as? CKError)?.code == .permissionFailure)
                #expect(!(await refusal.isGeneratedCancellation(from: prepared, reader: reader, read: id)))
                #expect(try await f.core.completeIdentityRead(id: newer, status: .couldNotDetermine, identity: nil))
            } catch {
                await projection.open(); _ = await completion.value
                await reader.close()
                throw error
            }
        }
    }

    @Test func genuineOriginalProjectionErrorIsRetainedWhenReaderCloseFollows() async throws {
        try await withNativePolicyFixture { f in
            let projection = CloudCallbackGate()
            let policy = try await projectionPolicy(projection)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            let id = try await reader.begin(transition: true)
            let prepared = await reader.prepareAccountRead(id)
            let completion = Task {
                let result = await prepared.complete()
                _ = await projection.operationSucceeded()
                return result
            }
            do {
                try await projection.waitForArrival()
                // Advance actual Core read authority without changing this
                // reader's local fence. The original projection must fail.
                _ = try await f.core.beginIdentityRead(transition: true)
                await projection.open()
                let result = await completion.value
                let originalError = try #require(result.error as? BridgeError)
                await reader.close()
                #expect(result.error as? BridgeError == originalError)
                #expect(!result.verified && result.outputs.isEmpty)
                // A later refusal retains the completed projection's error
                // and remains ineligible for shutdown qualification.
                let refusal = await prepared.refuse()
                #expect(refusal.error as? BridgeError == originalError)
                #expect(!(await refusal.isGeneratedCancellation(from: prepared, reader: reader, read: id)))
                #expect(!(result.error is CancellationError))
            } catch {
                await projection.open(); _ = await completion.value
                await reader.close()
                throw error
            }
        }
    }

    @Test func acceptedAppIdentityProjectionCannotReplaceANewerActualRead() async throws {
        try await withNativePolicyFixture { f in
            let (_, executor) = try await nativePolicy(f)
            let projection = CloudCallbackGate(), actualB = CloudCallbackGate()
            let policy = try await projectionPolicy(projection)
            let accounts = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let engine = try await f.engine()
            let original = engine.instanceID
            let made = await f.factory.engines().count
            let readCount = await f.factory.resolvedAccountReadCount()
            let before = try await f.core.durableSnapshot()
            try await f.collection.flushForBackground()
            await accounts.freeze()
            try await f.adapter.freezeForSuspend()
            #expect(try await f.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
            let delivered = Task { await engine.emitRetained(.stateUpdate(Data([0x6f, 0x6c, 0x64])), label: "acceptedProjectionState") }
            var readA: Task<CloudAppAccountReadResult, Never>?
            var readB: Task<CloudAppAccountReadResult, Never>?
            do {
                try await f.adapter.waitForSuspendedDelivery()
                _ = try await f.collection.resume()
                await accounts.resumeAfterCoreResume()
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
                let a = try await accounts.begin(transition: false)
                let pendingA = Task {
                    let result = await accounts.read(a)
                    _ = await projection.operationSucceeded()
                    return result
                }; readA = pendingA
                try await projection.waitForArrival()
                // The actual completion, not a test flag, reverified A. Its SDK
                // delivery still cannot pass the suspended replay gate.
                try await f.core.validateSDK(original)
                #expect(try await f.core.durableSnapshot().serialization == before.serialization)
                let b = try await accounts.begin(transition: true)
                #expect(b == a + 1)
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-B".utf8))), gate: actualB)
                let pendingB = Task {
                    let result = await accounts.read(b)
                    _ = await actualB.operationSucceeded()
                    return result
                }; readB = pendingB
                try await actualB.waitForArrival()
                await projection.open()
                let stale = await pendingA.value
                #expect(stale.error is CancellationError && !stale.verified && stale.outputs.isEmpty)
                #expect(try await f.core.durableSnapshot().serialization == before.serialization)
                #expect(!(await engine.recordedTrace()).contains(.eventReturned("acceptedProjectionState")))
                await actualB.open()
                let switched = await pendingB.value
                #expect(switched.error == nil && !switched.verified)
                try #require(switched.error == nil)
                let status = try await policy.status()
                #expect(status.accountStatus == .available && !status.accountMatchesBinding)
                #expect(status.attention[.isolated] == 1)
                await #expect(throws: (any Error).self) { try await f.core.validateSDK(original) }
                let after = try await f.core.durableSnapshot()
                #expect(after.binding == before.binding)
                #expect(after.serialization == before.serialization)
                let commands = switched.outputs.flatMap { output -> [CoreBridge.CloudPolicyCommand] in
                    if case .commands(let commands) = output { return commands }; return []
                }
                #expect(commands.contains { if case .stopEngine = $0.action { true } else { false } })
                #expect(!(await f.adapter.resumeDeliveries()))
                #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
                #expect(!(await engine.recordedTrace()).contains(.eventReturned("acceptedProjectionState")))
                #expect(await f.factory.resolvedAccountReadCount() == readCount + 2)
                let next = try await f.core.beginIdentityRead()
                #expect(next == b + 1)
                #expect(try await f.core.completeIdentityRead(id: next, status: .couldNotDetermine, identity: nil))
                for output in switched.outputs { try await consumePolicySetup(output, executor: executor) }
                await delivered.value
                #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                #expect(await engine.recordedTrace().contains(.released))
                #expect(await f.factory.engines().count == made)
                #expect(try await f.core.durableSnapshot().serialization == before.serialization)
            } catch {
                await projection.open(); await actualB.open()
                readA?.cancel(); readB?.cancel()
                _ = await readA?.value; _ = await readB?.value
                await accounts.close()
                try? await f.adapter.stop()
                await delivered.value
                throw error
            }
        }
    }

    @Test(arguments: [ProjectionInvalidation.refreeze, .appClose, .close, .generation])
    func acceptedProjectionCannotReleaseDeliveryAfterOwnerInvalidation(change: ProjectionInvalidation) async throws {
        try await withNativePolicyFixture { f in
            _ = try await nativePolicy(f)
            let projection = CloudCallbackGate()
            let policy = try await projectionPolicy(projection)
            let accounts = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot().serialization
            try await f.collection.flushForBackground()
            await accounts.freeze()
            try await f.adapter.freezeForSuspend()
            #expect(try await f.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
            let delivered = Task { await engine.emitRetained(.stateUpdate(Data([0x6c, 0x61, 0x74, 0x65])), label: "lateProjectionState") }
            var read: Task<CloudAppAccountReadResult, Never>?
            do {
                try await f.adapter.waitForSuspendedDelivery()
                _ = try await f.collection.resume()
                await accounts.resumeAfterCoreResume()
                await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
                let id = try await accounts.begin(transition: false)
                let pending = Task {
                    let result = await accounts.read(id)
                    _ = await projection.operationSucceeded()
                    return result
                }; read = pending
                try await projection.waitForArrival()
                switch change {
                case .refreeze:
                    // Same-actor execution revokes the app permit before the
                    // release task runs when freeze waits for this real read.
                    await Task { @MainActor in
                        let release = Task { @MainActor in await projection.open() }
                        await accounts.freeze()
                        await release.value
                    }.value
                case .appClose:
                    await Task { @MainActor in
                        let release = Task { @MainActor in await projection.open() }
                        await accounts.close()
                        await release.value
                    }.value
                case .close:
                    try await f.collection.close()
                    await projection.open()
                case .generation:
                    try await f.collection.close()
                    let location = CollectionLocation(collectionDirectory: f.directory.appending(path: "collection"),
                        backupsDirectory: f.directory.appending(path: "backups"),
                        appGroupSnapshot: f.directory.appending(path: "widget.snapshot"),
                        manualTransferDirectory: f.directory.appending(path: "synthetic-provider-installation"))
                    try await f.collection.open(location, randomSeed: 115)
                    await projection.open()
                }
                let stale = await pending.value
                #expect(stale.error != nil && !stale.verified && stale.outputs.isEmpty)
                #expect(try await f.core.durableSnapshot().serialization == before)
                #expect(!(await engine.recordedTrace()).contains(.eventReturned("lateProjectionState")))
                await accounts.close()
                try await f.adapter.stop()
                await delivered.value
                #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                #expect(await engine.recordedTrace().contains(.released))
                #expect(try await f.core.durableSnapshot().serialization == before)
            } catch {
                await projection.open()
                read?.cancel(); _ = await read?.value
                await accounts.close()
                try? await f.adapter.stop()
                await delivered.value
                throw error
            }
        }
    }
}
