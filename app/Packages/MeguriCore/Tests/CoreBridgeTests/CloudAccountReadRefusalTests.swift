import Foundation
import Synchronization
import Testing
@testable import CoreBridge

extension EventTests {
    @Test(arguments: [false, true], [false, true])
    func originalResolutionOrdersCancellationDuringProviderLaunch(cancellationFirst: Bool, providerCancellation: Bool) async throws {
        try await withNativePolicyFixture { f in
            let resolution = CloudAccountResolution()
            let launch = CloudCallbackGate(), provider = CloudCallbackGate()
            let launchedTask = Mutex<Task<Void, Never>?>(nil)
            let error: any Error = providerCancellation ? CancellationError() : BridgeError.internal("synthetic-launch-failure")
            await f.factory.appendAccountRead(.failure(error), gate: provider)
            let owner = Task {
                let outcome = await resolution.resolveOutcome(using: f.factory, afterProviderLaunch: { task in
                    launchedTask.withLock { $0 = task }
                    await launch.enter()
                })
                _ = await launch.operationSucceeded()
                return outcome
            }
            do {
                try await launch.waitForArrival()
                let task = try #require(launchedTask.withLock { $0 })
                // The original owner stays paused until detached delivery has
                // finished, so cancellation cannot rely on waiter installation.
                if cancellationFirst { owner.cancel() }
                await provider.open()
                await task.value
                if !cancellationFirst { owner.cancel() }
                await launch.open()
                let outcome = await owner.value
                if cancellationFirst {
                    guard case .fence(taskCancellation: true) = outcome else {
                        Issue.record("Owner cancellation during launch must win before the later provider error")
                        return
                    }
                } else {
                    guard case .provider(.failure(let failure)) = outcome else {
                        Issue.record("An already delivered provider failure must survive owner cancellation")
                        return
                    }
                    if providerCancellation { #expect(failure is CancellationError) }
                    else { #expect(failure as? BridgeError == .internal("synthetic-launch-failure")) }
                }
            } catch {
                owner.cancel()
                await provider.open(); await launch.open()
                _ = await owner.value
                throw error
            }
        }
    }

    @Test func rejectedFallbackAcceptanceRetainsOriginalResolverFailure() async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            await f.factory.appendAccountRead(.failure(BridgeError.internal("synthetic-fallback-refusal")))
            let id = try await reader.begin(transition: true)
            let prepared = await reader.prepareAccountRead(id)
            // Core advances independently while the reader's local admission
            // remains valid. The real fallback must reject this older read.
            let newer = try await f.core.beginIdentityRead(transition: true)
            #expect(await reader.isCurrentRead(id))
            await MainActor.run {
                reader.afterFallbackIdentityCompletion = { accepted in #expect(!accepted) }
            }
            let result = await prepared.complete()
            #expect(result.error as? BridgeError == .internal("synthetic-fallback-refusal"))
            #expect(result.outputs.isEmpty && !result.verified)
            #expect(await reader.isCurrentRead(id))
            await #expect(throws: (any Error).self) { try await policy.projectIdentityRead(id) }
            #expect(try await f.core.completeIdentityRead(id: newer, status: .couldNotDetermine, identity: nil))
            await reader.close()
        }
    }

    @Test(arguments: [false, true])
    func closeWhileFallbackCompletionReturnsRetainsResolverFailure(cancellation: Bool) async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let fallback = CloudCallbackGate()
            let error: any Error = cancellation ? CancellationError() : BridgeError.internal("synthetic-fallback-close")
            await f.factory.appendAccountRead(.failure(error))
            let id = try await reader.begin(transition: true)
            let prepared = await reader.prepareAccountRead(id)
            await MainActor.run {
                reader.afterFallbackIdentityCompletion = { accepted in
                    #expect(accepted)
                    await fallback.enter()
                }
            }
            let completion = Task {
                let result = await prepared.complete()
                _ = await fallback.operationSucceeded()
                return result
            }
            do {
                try await fallback.waitForArrival()
                await Task { @MainActor in
                    let release = Task { await fallback.open() }
                    await reader.close()
                    await release.value
                }.value
                let result = await completion.value
                if cancellation { #expect(result.error is CancellationError) }
                else { #expect(result.error as? BridgeError == .internal("synthetic-fallback-close")) }
                #expect(result.outputs.isEmpty && !result.verified)
                #expect(!(await reader.isCurrentRead(id)))
                let refusal = await prepared.refuse()
                if cancellation { #expect(refusal.error is CancellationError) }
                else { #expect(refusal.error as? BridgeError == .internal("synthetic-fallback-close")) }
                #expect(!(await refusal.isGeneratedCancellation(from: prepared, reader: reader, read: id)))
            } catch {
                await fallback.open(); _ = await completion.value
                await reader.close()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func preparedRefusalRetainsFirstProviderFailureAcrossOwnerClose(cancellation: Bool) async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let error: any Error = cancellation ? CancellationError() : BridgeError.internal("synthetic-original-resolver")
            await f.factory.appendAccountRead(.failure(error))
            let id = try await reader.begin(transition: true)
            let prepared = await reader.prepareAccountRead(id)
            await reader.close()
            let refusal = await prepared.refuse()
            if cancellation { #expect(refusal.error is CancellationError) }
            else { #expect(refusal.error as? BridgeError == .internal("synthetic-original-resolver")) }
            #expect(!(await refusal.isGeneratedCancellation(from: prepared, reader: reader, read: id)))
            // Refusal never consumes the original Core read or projects proof.
            await #expect(throws: (any Error).self) { try await policy.projectIdentityRead(id) }
            #expect(try await f.core.completeIdentityRead(id: id, status: .couldNotDetermine, identity: nil))
        }
    }

    @Test func generatedRefusalIsBoundToOriginalPreparationReaderAndRead() async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            let reader = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let unrelated = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let id = try await reader.begin(transition: true)
            await f.factory.appendAccountRead(.success(.init(status: .available, identity: Data("synthetic-account-A".utf8))))
            let prepared = await reader.prepareAccountRead(id)
            let otherPreparation = await reader.prepareAccountRead(id)
            let refusal = await prepared.refuse()
            #expect(await refusal.isGeneratedCancellation(from: prepared, reader: reader, read: id))
            #expect(!(await refusal.isGeneratedCancellation(from: otherPreparation, reader: reader, read: id)))
            #expect(!(await refusal.isGeneratedCancellation(from: prepared, reader: unrelated, read: id)))
            #expect(!(await refusal.isGeneratedCancellation(from: prepared, reader: reader, read: id + 1)))
            await #expect(throws: (any Error).self) { try await policy.projectIdentityRead(id) }
            #expect(try await f.core.completeIdentityRead(id: id, status: .couldNotDetermine, identity: nil))
            await reader.close(); await unrelated.close()
        }
    }

    @Test func originalResolutionKeepsFirstProviderFailureAndFirstFenceOrigin() async throws {
        try await withNativePolicyFixture { f in
            let failed = CloudAccountResolution()
            failed.finish(.failure(CancellationError()))
            failed.cancel()
            let first = await failed.resolveOutcome(using: f.factory)
            guard case .provider(.failure(let error)) = first else {
                Issue.record("An earlier provider cancellation must remain a provider failure")
                return
            }
            #expect(error is CancellationError)

            let fenced = CloudAccountResolution()
            fenced.cancel()
            // Even a subsequent task cancellation cannot relabel an earlier
            // reader fence as a cancellation owned by that task.
            let pending = Task { await fenced.resolveOutcome(using: f.factory) }
            pending.cancel()
            let outcome = await pending.value
            guard case .fence(taskCancellation: false) = outcome else {
                Issue.record("The first reader fence must retain its origin")
                return
            }
            fenced.finish(.success(.init(status: .available, identity: Data("synthetic-late-provider".utf8))))
            let afterLateProvider = await fenced.resolveOutcome(using: f.factory)
            guard case .fence(taskCancellation: false) = afterLateProvider else {
                Issue.record("A late provider must not replace the original fence")
                return
            }
        }
    }
}
