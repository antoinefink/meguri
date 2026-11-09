import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

// Session s-20261109T094715Z-c7c7c45e; integration base 8921d870.
// Original batch base 1416a520542a8c5daa38a23bfe398da704c3d6c9.
// Additive witnesses over the reviewed lifecycle seed; no result substitution.
private func assetResourceFailureRawPass(
    _ fixture: CloudKitBridgeFixture, policy: CoreCloudPolicyService, remote: AssetExecutorRemote
) async throws -> MeguriCoreBindings.CloudAssetReadPass {
    let execution = try await assetExecutorRetryExecution(fixture, policy: policy, records: [remote])
    return try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginAssetReadPass) {
        try execution.raw.beginAssetReadPass()
    }
}

private func assetResourceFailureRawPolicy() async throws -> MeguriCoreBindings.CloudPolicy {
    let workflow = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
    return try await MaintenanceLane.shared.call(.cloudWorkflowPolicy) { try workflow.policy() }
}

private func assetResourceFailureHoldPool() async throws -> [CoreJob] {
    var jobs: [CoreJob] = []
    do {
        for _ in 0..<4 {
            let job = try await Jobs.start("test.startJob") {
                try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil)
            }
            jobs.append(job)
            var progress = job.progress.makeAsyncIterator()
            try #require(await progress.next() != nil, "Each original pool worker must be occupied")
        }
        return jobs
    } catch {
        await assetResourceFailureReleasePool(jobs)
        throw error
    }
}

private func assetResourceFailureReleasePool(_ jobs: [CoreJob]) async {
    for job in jobs { try? await job.cancel() }
    for job in jobs { _ = await job.outcome() }
}

private struct AssetResourceFailureQueue: CoreBridge.CloudAssetReadQueue {
    let operation: AssetExecutorOperation
    func enqueue() -> Bool { operation.enqueue() }
}

private func assetResourceFailureMalformedResult(
    _ remote: AssetExecutorRemote, directory: URL
) throws -> CoreBridge.CloudAssetReadResult {
    let source = directory.appending(path: "sdk-\(remote.noteID)")
    try remote.payload.write(to: source)
    let staging = try CloudMediaStaging(directory: directory.appending(path: "owned-\(remote.noteID)"), available: { UInt64.max })
    let asset = try staging.copyPayload(source, cancelled: { false })
    try FileManager.default.removeItem(at: source)
    // Keep the real Rust codec payload and lease; reject only SDK metadata.
    let record = CoreBridge.CloudRecord(name: remote.record.name, envelope: remote.record.envelope,
        systemFields: remote.record.systemFields, recordType: "Unsupported", zone: remote.record.zone, assetURL: asset.url)
    return try .init(record: record, asset: asset)
}

extension EventTests {
    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func assetRequiredExecutorTrackingFailureKeepsOrderUntilActualBodyReturns(holdFinish: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let raw = try await assetResourceFailureRawPass(fixture, policy: policy, remote: remote)
            let rawPolicy = try await assetResourceFailureRawPolicy()
            let gate = try await MaintenanceLane.shared.call("test.cloudHoldAssetReadBody") {
                try testCloudHoldAssetReadBody(finish: holdFinish)
            }
            let target = holdFinish ? 5 : 3 // next, submit, complete, exhausted next, finish
            let calls = Mutex(0), admitted = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let (setups, setupFailed) = AsyncStream<Void>.makeStream()
            let pass = CoreCloudAssetReadPass(raw, track: { handle in
                let ordinal = calls.withLock { $0 += 1; return $0 }
                if ordinal == target {
                    admitted.withLock { $0 = handle }
                    setupFailed.yield(())
                    throw BridgeError.invalidArgument("required asset resource tracking setup")
                }
                return try Jobs.track(handle)
            })
            let operation = AssetExecutorOperation(outcome: .success(.assetRead(try remote.ownedResult(in: fixture.directory))))
            let executor = CloudAssetReadExecutor(transport: AssetExecutorTransport([remote.record.name: operation]))
            let returned = Mutex(false), followingReturned = Mutex(false)
            let running = Task {
                defer { returned.withLock { $0 = true }; setupFailed.finish() }
                return try await executor.run(pass)
            }
            var following: Task<CoreBridge.CloudPolicyStatus, any Error>?
            do {
                try await operation.terminal.waitForArrival()
                #expect(operation.enqueueCalls.withLock { $0 } == 1)
                #expect(operation.replyCalls.withLock { $0 } == 1)
                await operation.terminal.open()
                try await QueryLane.shared.call("test.assetReadBodyClaimed") { try gate.waitForClaim() }
                var setup = setups.makeAsyncIterator()
                try #require(await setup.next() != nil)
                let (queued, didQueue) = AsyncStream<Void>.makeStream()
                let successor = CoreCloudPolicyService(rawPolicy, onJobQueued: { didQueue.yield(()) })
                let next = Task {
                    defer { followingReturned.withLock { $0 = true }; didQueue.finish() }
                    return try await successor.status()
                }
                following = next
                var waiting = queued.makeAsyncIterator()
                try #require(await waiting.next() != nil, "Following policy work must wait for the original resource drain")
                #expect(!returned.withLock { $0 } && !followingReturned.withLock { $0 })
                let owned = fixture.directory.appending(path: "owned-\(remote.noteID)")
                let beforeRelease = try await remote.observe()
                if holdFinish {
                    #expect(beforeRelease.ownedCount == 0 && beforeRelease.rowFields == remote.fields)
                    #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).isEmpty)
                } else {
                    #expect(beforeRelease.ownedCount == 1 && beforeRelease.rowFields == nil)
                    #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).count == 1)
                }
                try await QueryLane.shared.call("test.releaseAssetReadBody") { try gate.release() }
                let result = try await running.value
                _ = try await next.value
                #expect(result.readCount == 1 && result.outcome == .succeeded && result.failures.isEmpty)
                #expect(calls.withLock { $0 } == 5)
                let handle = try #require(admitted.withLock { $0 })
                #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
                let final = try await remote.observe()
                #expect(final.ownedCount == 0 && final.retryCount == nil && final.rowFields == remote.fields)
                #expect(final.changedByIncoming)
                #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).isEmpty)
                _ = try await successor.status() // The successor also released the ordering permit.
                await executor.close()
            } catch {
                try? await QueryLane.shared.call("test.releaseAssetReadBody") { try gate.release() }
                await operation.terminal.open()
                _ = await running.result
                _ = await following?.result
                await executor.close()
                throw error
            }
        }
    }

    @Test(.timeLimit(.minutes(1)))
    func assetCompletionTrackingFailureRetainsStagedFileThroughActualNativeFailure() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let raw = try await assetResourceFailureRawPass(fixture, policy: policy, remote: remote)
            let gate = try await MaintenanceLane.shared.call("test.cloudHoldAssetReadBody") {
                try testCloudHoldAssetReadBody(finish: false)
            }
            let calls = Mutex(0), admitted = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let (setups, setupFailed) = AsyncStream<Void>.makeStream()
            let pass = CoreCloudAssetReadPass(raw, track: { handle in
                let ordinal = calls.withLock { $0 += 1; return $0 }
                if ordinal == 3 {
                    admitted.withLock { $0 = handle }
                    setupFailed.yield(())
                    throw BridgeError.invalidArgument("failing completion tracking setup")
                }
                return try Jobs.track(handle)
            })
            let operation = AssetExecutorOperation(outcome: .success(.assetRead(
                try assetResourceFailureMalformedResult(remote, directory: fixture.directory))))
            let executor = CloudAssetReadExecutor(transport: AssetExecutorTransport([remote.record.name: operation]))
            let returned = Mutex(false)
            let running = Task {
                defer { returned.withLock { $0 = true }; setupFailed.finish() }
                return try await executor.run(pass)
            }
            do {
                try await operation.terminal.waitForArrival()
                await operation.terminal.open()
                try await QueryLane.shared.call("test.assetReadBodyClaimed") { try gate.waitForClaim() }
                var setup = setups.makeAsyncIterator()
                try #require(await setup.next() != nil)
                #expect(!returned.withLock { $0 })
                let owned = fixture.directory.appending(path: "owned-\(remote.noteID)")
                let runs = try FileManager.default.contentsOfDirectory(at: owned, includingPropertiesForKeys: nil)
                let copied = try #require(runs.first).appending(path: "asset")
                #expect(runs.count == 1)
                #expect(try Data(contentsOf: copied) == remote.payload)
                #expect(!FileManager.default.fileExists(atPath: fixture.directory.appending(path: "sdk-\(remote.noteID)").path))
                #expect(try await remote.observe().ownedCount == 1)
                try await QueryLane.shared.call("test.releaseAssetReadBody") { try gate.release() }
                let result = await running.result
                guard case .failure(let error) = result else {
                    Issue.record("Actual metadata rejection became a successful cleanup")
                    await executor.close()
                    return
                }
                #expect(error as? BridgeError == .unavailable(reason: "unsupported asset read record metadata"))
                #expect(calls.withLock { $0 } == 4, "Finish must still run after the failing completion")
                let handle = try #require(admitted.withLock { $0 })
                #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
                let final = try await remote.observe()
                #expect(final.ownedCount == 0 && final.retryCount == 1 && final.rowFields == nil)
                #expect(!final.changedByIncoming)
                #expect(!FileManager.default.fileExists(atPath: copied.path))
                #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).isEmpty)
                #expect(operation.enqueueCalls.withLock { $0 } == 1 && operation.replyCalls.withLock { $0 } == 1)
                _ = try await policy.status()
                await executor.close()
            } catch {
                try? await QueryLane.shared.call("test.releaseAssetReadBody") { try gate.release() }
                await operation.terminal.open()
                _ = await running.result
                await executor.close()
                throw error
            }
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func assetCancelTrackingFailureKeepsActualSelectedOrAcceptedOutcome(accepted: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let raw = try await assetResourceFailureRawPass(fixture, policy: policy, remote: remote)
            let original = CoreCloudAssetReadPass(raw)
            let work = try #require(try await original.next())
            let operation = AssetExecutorOperation(outcome: .success(.assetRead(.failed)))
            if accepted {
                #expect(try await original.submit(work.handle, queue: AssetResourceFailureQueue(operation: operation)))
            }
            let rawPolicy = try await assetResourceFailureRawPolicy()
            let pool = try await assetResourceFailureHoldPool()
            let admitted = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let (setups, setupFailed) = AsyncStream<Void>.makeStream()
            let pass = CoreCloudAssetReadPass(raw, track: { handle in
                admitted.withLock { $0 = handle }
                setupFailed.yield(())
                throw BridgeError.invalidArgument("cancel tracking setup")
            })
            let returned = Mutex(false), followingReturned = Mutex(false)
            let cancelling = Task {
                defer { returned.withLock { $0 = true }; setupFailed.finish() }
                return try await pass.cancel(work.handle)
            }
            var following: Task<CoreBridge.CloudPolicyStatus, any Error>?
            do {
                var setup = setups.makeAsyncIterator()
                try #require(await setup.next() != nil)
                let (queued, didQueue) = AsyncStream<Void>.makeStream()
                let successor = CoreCloudPolicyService(rawPolicy, onJobQueued: { didQueue.yield(()) })
                let next = Task {
                    defer { followingReturned.withLock { $0 = true }; didQueue.finish() }
                    return try await successor.status()
                }
                following = next
                var waiting = queued.makeAsyncIterator()
                try #require(await waiting.next() != nil)
                cancelling.cancel()
                #expect(!returned.withLock { $0 } && !followingReturned.withLock { $0 })
                #expect(try await remote.observe().ownedCount == (accepted ? 1 : 0))
                await assetResourceFailureReleasePool(pool)
                #expect(try await cancelling.value == !accepted)
                _ = try await next.value
                let handle = try #require(admitted.withLock { $0 })
                #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
                if accepted {
                    #expect(try await remote.observe().ownedCount == 1, "False cancellation must retain the accepted original")
                    let reply = Task { try await operation.reply() }
                    await operation.terminal.open()
                    guard case .assetRead(let result) = try await reply.value else {
                        throw CloudOwnTransportFailure.unexpectedReply
                    }
                    #expect(try await original.complete(work.handle, result: result).intakeAccepted == false)
                } else {
                    #expect(try await original.cancel(work.handle) == false)
                }
                try await original.finish()
                let final = try await remote.observe()
                #expect(final.ownedCount == 0 && final.retryCount == (accepted ? 1 : 0) && final.rowFields == nil)
                #expect(operation.enqueueCalls.withLock { $0 } == (accepted ? 1 : 0))
                #expect(operation.replyCalls.withLock { $0 } == (accepted ? 1 : 0))
                _ = try await successor.status()
            } catch {
                await assetResourceFailureReleasePool(pool)
                _ = await cancelling.result
                _ = await following?.result
                await operation.terminal.open()
                if accepted { _ = try? await original.complete(work.handle, result: .failed) }
                try? await original.finish()
                throw error
            }
        }
    }
}
