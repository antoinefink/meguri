import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

extension EventTests {
    @Test(arguments: [false, true])
    func mediaNextWorkCancelsAndDrainsQueuedJobBeforeReleasingServiceOrder(listenerFailure: Bool) async throws {
        try await withCloudConnection { _ in
            let workflow = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
            let raw = try await MaintenanceLane.shared.call(.cloudWorkflowMedia) { try workflow.media() }
            var pool: [CoreJob] = []
            let (arrivals, arrived) = AsyncStream.makeStream(of: Void.self)
            let witness = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let returned = Mutex(false)
            let (queued, didQueue) = AsyncStream.makeStream(of: Void.self)
            let service = CoreCloudMediaService(raw, onJobQueued: { didQueue.yield(()) }, track: { handle in
                witness.withLock { $0 = handle }; arrived.yield(())
                if listenerFailure { throw BridgeError.invalidArgument("media listener setup") }
                return try Jobs.track(handle)
            })
            var call: Task<CoreBridge.CloudMediaIssuedWork?, any Error>?
            var following: Task<Bool, any Error>?
            do {
                for _ in 0..<4 {
                    let job = try await Jobs.start("test.startJob") { try MeguriCoreBindings.testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil) }
                    pool.append(job)
                    var progress = job.progress.makeAsyncIterator()
                    try #require(await progress.next() != nil, "positive entry into each original pool worker")
                }
                let active = Task {
                    defer { returned.withLock { $0 = true } }
                    return try await service.nextWork(sdk: "synthetic-queued-sdk")
                }
                call = active
                var arrival = arrivals.makeAsyncIterator()
                _ = await arrival.next()
                let handle = try #require(witness.withLock { $0 })
                let next = Task { try await service.downloadsBlocked(sdk: "synthetic-queued-sdk") }
                following = next
                var waiting = queued.makeAsyncIterator()
                _ = await waiting.next()
                active.cancel()
                // Observe the native atomic cancellation before releasing the
                // occupied pool; completion must not win this test's setup.
                var cancelled = false
                for _ in 0..<500 {
                    cancelled = try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled()
                    if cancelled { break }
                    try await Task.sleep(for: .milliseconds(10))
                }
                #expect(cancelled, "Swift cancellation/setup failure did not explicitly cancel Core")
                // A real review start can still be admitted on Maintenance.
                let review = try await Core.startSessionJob(deckID: Int64.max)
                #expect(!returned.withLock { $0 }, "media call escaped before native terminal drain")
                for job in pool { try? await job.cancel() }
                for job in pool { _ = await job.outcome() }
                _ = await review.job.outcome()
                do { _ = try await active.value; Issue.record("queued native cancellation was lost") }
                catch { #expect(error as? BridgeError == .cancelled) }
                _ = try? await next.value
                #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
                #expect(returned.withLock { $0 })
            } catch {
                call?.cancel()
                for job in pool { try? await job.cancel() }
                for job in pool { _ = await job.outcome() }
                _ = try? await call?.value
                _ = try? await following?.value
                throw error
            }
            arrived.finish(); didQueue.finish()
        }
    }
}
