import Dispatch
import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

extension EventTests {
    @Test func assetSubmitTrackingFailureDrainsCallbackAcceptedBeforeReturnAndOriginalTerminal() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let execution = try await assetExecutorRetryExecution(fixture, policy: policy, records: [remote])
            let raw = try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginAssetReadPass) {
                try execution.raw.beginAssetReadPass()
            }
            let releaseCallback = DispatchSemaphore(value: 0)
            let accepted = DispatchSemaphore(value: 0)
            let (callbacks, callbackEntered) = AsyncStream<Void>.makeStream()
            let (setups, setupFailed) = AsyncStream<Void>.makeStream()
            let watchdog = Mutex(false)
            let operation = AssetExecutorOperation(outcome: .success(.assetRead(try remote.ownedResult(in: fixture.directory))), acceptedBeforeReturn: {
                callbackEntered.yield(())
                accepted.signal()
                if releaseCallback.wait(timeout: .now() + 10) == .timedOut { watchdog.withLock { $0 = true } }
            })
            let tracked = Mutex(0)
            let pass = CoreCloudAssetReadPass(raw, track: { handle in
                let ordinal = tracked.withLock { $0 += 1; return $0 }
                if ordinal == 2 {
                    if accepted.wait(timeout: .now() + 10) == .timedOut { watchdog.withLock { $0 = true } }
                    setupFailed.yield(())
                    throw BridgeError.invalidArgument("asset submit tracking failed")
                }
                return try Jobs.track(handle)
            })
            let (closes, closeAdmitted) = AsyncStream<Void>.makeStream()
            let executor = CloudAssetReadExecutor(transport: AssetExecutorTransport([remote.record.name: operation]),
                onCloseAdmission: { closeAdmitted.yield(()) })
            let returned = Mutex(false), closed = Mutex(false)
            let task = Task {
                defer { returned.withLock { $0 = true } }
                return try await executor.run(pass)
            }
            do {
                var callback = callbacks.makeAsyncIterator(), setup = setups.makeAsyncIterator()
                try #require(await callback.next() != nil)
                try #require(await setup.next() != nil)
                #expect(!returned.withLock { $0 })
                #expect(operation.enqueueCalls.withLock { $0 } == 1)
                #expect(operation.replyCalls.withLock { $0 } == 0)
                // Native already accepted, while the receipt is still .calling.
                let close = Task { await executor.close(); closed.withLock { $0 = true } }
                var closeAdmission = closes.makeAsyncIterator()
                try #require(await closeAdmission.next() != nil)
                #expect(!closed.withLock { $0 })
                releaseCallback.signal()
                try await operation.terminal.waitForArrival()
                #expect(operation.replyCalls.withLock { $0 } == 1)
                #expect(!closed.withLock { $0 })
                let owned = fixture.directory.appending(path: "owned-\(remote.noteID)")
                #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).count == 1)
                await operation.terminal.open()
                _ = await task.result
                await close.value
                #expect(returned.withLock { $0 } && closed.withLock { $0 })
                #expect(operation.replyCalls.withLock { $0 } == 1)
                let final = try await remote.observe()
                #expect(final.ownedCount == 0 && final.retryCount == nil && final.rowFields == remote.fields)
                #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).isEmpty)
                #expect(!watchdog.withLock { $0 })
            } catch {
                releaseCallback.signal()
                await operation.terminal.open()
                _ = await task.result
                await executor.close()
                throw error
            }
        }
    }

    @Test(arguments: [false, true])
    func assetRequiredCompletionAndFinishBodiesKeepConcurrentCloseJoined(holdFinish: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [remote])
            let gate = try await MaintenanceLane.shared.call("test.cloudHoldAssetReadBody") {
                try testCloudHoldAssetReadBody(finish: holdFinish)
            }
            let operation = AssetExecutorOperation(outcome: .success(.assetRead(try remote.ownedResult(in: fixture.directory))))
            let (closes, closeAdmitted) = AsyncStream<Void>.makeStream()
            let executor = CloudAssetReadExecutor(transport: AssetExecutorTransport([remote.record.name: operation]),
                onCloseAdmission: { closeAdmitted.yield(()) })
            let closed = Mutex(false), returned = Mutex(false)
            let task = Task {
                defer { returned.withLock { $0 = true } }
                return try await executor.run(pass)
            }
            do {
                try await operation.terminal.waitForArrival()
                await operation.terminal.open()
                try await QueryLane.shared.call("test.assetReadBodyClaimed") { try gate.waitForClaim() }
                let close = Task { await executor.close(); closed.withLock { $0 = true } }
                var closeAdmission = closes.makeAsyncIterator()
                try #require(await closeAdmission.next() != nil)
                #expect(!returned.withLock { $0 } && !closed.withLock { $0 })
                #expect(operation.replyCalls.withLock { $0 } == 1)
                if !holdFinish {
                    let owned = fixture.directory.appending(path: "owned-\(remote.noteID)")
                    #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).count == 1)
                    #expect(try await remote.observe().ownedCount == 1)
                }
                try await QueryLane.shared.call("test.releaseAssetReadBody") { try gate.release() }
                _ = await task.result
                await close.value
                #expect(closed.withLock { $0 } && returned.withLock { $0 })
                let final = try await remote.observe()
                #expect(final.ownedCount == 0 && final.rowFields == remote.fields)
                let owned = fixture.directory.appending(path: "owned-\(remote.noteID)")
                #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).isEmpty)
            } catch {
                try? await QueryLane.shared.call("test.releaseAssetReadBody") { try gate.release() }
                await operation.terminal.open()
                _ = await task.result
                await executor.close()
                throw error
            }
        }
    }
}
