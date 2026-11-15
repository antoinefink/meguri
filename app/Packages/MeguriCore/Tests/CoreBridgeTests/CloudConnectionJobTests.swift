import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

private func rawJobConnection() async throws -> MeguriCoreBindings.CloudConnection {
    let workflow = try await MaintenanceLane.shared.call(.cloudWorkflowNew) { try MeguriCoreBindings.CloudWorkflow() }
    return try await MaintenanceLane.shared.call(.cloudWorkflowConnection) { try workflow.connection() }
}
private func holdConnectionPool() async throws -> [CoreJob] {
    var jobs: [CoreJob] = []
    do {
        for _ in 0..<4 {
            let job = try await Jobs.start("test.startJob") { try testStartJob(durationMs: 60_000, reportsPerSecond: 100, failWith: nil) }
            jobs.append(job)
            var progress = job.progress.makeAsyncIterator()
            try #require(await progress.next() != nil, "positive entry into each original pool worker")
        }
        return jobs
    } catch {
        await releaseConnectionPool(jobs)
        throw error
    }
}
private func releaseConnectionPool(_ jobs: [CoreJob]) async {
    for job in jobs { try? await job.cancel() }
    for job in jobs { _ = await job.outcome() }
}
private func connectionAdoption(_ connection: CoreCloudConnectionService) async throws -> CoreBridge.CloudAdoptionWork {
    // Canonical K5.2 control records with synthetic ids, zero default times,
    // complete schema 18 epoch and no entity counts. No transport is involved.
    let id = Data(repeating: 8, count: 16)
    let active = CoreBridge.CloudControlRecord(encrypted: Data([8, 1, 18, 16]) + id, systemFields: Data([1]))
    let epoch = CoreBridge.CloudControlRecord(encrypted: Data([8, 1, 18, 16]) + id + Data([26, 32]) + Data(repeating: 19, count: 32) + Data([40, 18, 48, 2, 56, 1]), systemFields: Data([2]))
    guard case .readActive(let activePreview, _) = try await connection.beginConnectionPreview(),
          case .readEpoch(let epochPreview, _) = try await connection.connectionActiveRead(activePreview, read: .found(active)),
          case .ready(let preview, _) = try await connection.connectionEpochRead(epochPreview, read: .found(epoch)) else {
        throw CloudOwnTransportFailure.unexpectedReply
    }
    _ = try await connection.confirmAdoption(preview)
    let status = try #require(try await connection.recoveryStatus())
    #expect(!status.adoptionRecoveredCancelOnly)
    #expect(!status.adoptionAfterReview)
    guard case .adoption(let handle, _, _) = try await connection.nextPublicationWork() else { throw CloudOwnTransportFailure.unexpectedReply }
    return handle
}

extension EventTests {
    @Test func connectionTypedNextRetainsResultAcrossOriginalClose() async throws {
        try await withCloudConnection { fixture in
            let raw = try await rawJobConnection()
            try await CoreEventStream.shared.start()
            let started = try await MaintenanceLane.shared.call(.cloudConnectionStartNextPublicationWork) { try raw.startNextPublicationWork() }
            let job = try await Jobs.trackDraining(started.handle)
            try await job.value()
            try await fixture.collection.close()
            let value = try await MaintenanceLane.shared.call(.cloudPublicationWorkResult) { try started.result.result() }
            guard case .idle? = value else { Issue.record("typed Idle must survive original close"); return }
            await #expect(throws: BridgeError.cloudConnection(.closed)) { _ = try await fixture.connection.nextPublicationWork() }
        }
    }

    @Test func connectionNextListenerFailureCancelsAndDrainsExactQueuedJob() async throws {
        try await withCloudConnection { _ in
            let raw = try await rawJobConnection()
            let pool = try await holdConnectionPool()
            let (arrivals, arrived) = AsyncStream.makeStream(of: Void.self)
            let witness = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let returned = Mutex(false)
            let service = CoreCloudConnectionService(raw, track: { handle in
                witness.withLock { $0 = handle }; arrived.yield(())
                throw BridgeError.invalidArgument("connection listener")
            })
            let call = Task {
                defer { returned.withLock { $0 = true } }
                return try await service.nextPublicationWork()
            }
            var iterator = arrivals.makeAsyncIterator()
            _ = await iterator.next()
            let handle = try #require(witness.withLock { $0 })
            do {
                // Admission and fallback cancellation leave Maintenance free
                // while all workers are positively occupied by the test jobs.
                let review = try await Core.startSessionJob(deckID: Int64.max)
                #expect(!returned.withLock { $0 }, "listener failure escaped before terminal drain")
                await releaseConnectionPool(pool)
                _ = await review.job.outcome()
                _ = try await call.value
                Issue.record("listener failure lost")
            } catch {
                await releaseConnectionPool(pool)
                _ = try? await call.value
                #expect(error as? BridgeError == .invalidArgument("connection listener"))
            }
            #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled())
            #expect(returned.withLock { $0 })
        }
    }

    @Test(arguments: [false, true])
    func connectionAdoptionPayloadLeaseOutlivesQueuedWorkAndListenerFailureDrain(listenerFailure: Bool) async throws {
        try await withCloudConnection { fixture in
            let raw = try await rawJobConnection()
            let (arrivals, arrived) = AsyncStream.makeStream(of: Void.self)
            let witness = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let returned = Mutex(false)
            let armed = Mutex(false)
            let original = CoreCloudConnectionService(raw, track: { admitted in
                if armed.withLock({ $0 }) {
                    witness.withLock { $0 = admitted }; arrived.yield(())
                    if listenerFailure { throw BridgeError.invalidArgument("adoption listener") }
                }
                return try Jobs.track(admitted)
            })
            let handle = try await connectionAdoption(original)
            armed.withLock { $0 = true }
            let pool = try await holdConnectionPool()
            let source = fixture.directory.appending(path: "native-payload")
            try Data([1, 2, 3]).write(to: source)
            let call = Task {
                defer { returned.withLock { $0 = true } }
                let lease = CloudMediaStagedAsset(url: source, release: { try? FileManager.default.removeItem(at: source) })
                let record = CoreBridge.CloudStagingRecord(name: "synthetic", recordType: "MGEntity", record: .init(encrypted: Data([1]), systemFields: Data([2])), payloadLease: lease)
                // A page at ReadActive is malformed. When actually claimed,
                // its original mandatory abort must outrank Swift cancellation.
                try await original.adoptionResult(handle, result: .page(records: [record], deleted: [], next: nil))
            }
            var iterator = arrivals.makeAsyncIterator(); _ = await iterator.next()
            do {
                #expect(FileManager.default.fileExists(atPath: source.path))
                #expect(!returned.withLock { $0 })
                call.cancel()
                _ = try await MaintenanceLane.shared.call("test.leaseWitness") { FileManager.default.fileExists(atPath: source.path) }
                #expect(FileManager.default.fileExists(atPath: source.path))
                await releaseConnectionPool(pool)
                try await call.value
                Issue.record("expected original reply or listener failure")
            } catch {
                await releaseConnectionPool(pool)
                _ = try? await call.value
                #expect(error as? BridgeError == (listenerFailure ? .invalidArgument("adoption listener") : .cloudConnection(.tracking)))
            }
            #expect(!FileManager.default.fileExists(atPath: source.path), "native lease survived terminal drain scope")
            if listenerFailure {
                let admitted = try #require(witness.withLock { $0 })
                #expect(try await CoreBridge.CancelToken(raw: admitted.cancel).isCancelled())
            }
        }
    }
}

extension EventTests {
    @Test func connectionPostClaimReplyOrdersNextWorkAndRetainsFollowupAfterTaskCancellation() async throws {
        try await withCloudConnection { _ in
            let raw = try await rawJobConnection()
            let (queued, didQueue) = AsyncStream.makeStream(of: Void.self)
            let connection = CoreCloudConnectionService(raw, onJobQueued: { didQueue.yield(()) })
            let handle = try await connectionAdoption(connection)
            let gate = try await MaintenanceLane.shared.call("test.holdAdoptionReply") { try testCloudHoldAdoptionReply() }
            let id = Data(repeating: 8, count: 16)
            let active = CoreBridge.CloudControlRecord(encrypted: Data([8, 1, 18, 16]) + id, systemFields: Data([1]))
            let reply = Task { try await connection.adoptionResult(handle, result: .read(.found(active))) }
            var following: Task<CoreBridge.CloudPublicationWork, any Error>?
            do {
                // The actual opaque task has been consumed; an overtaking next
                // call would return Idle while this reply still owns next=None.
                try await QueryLane.shared.call("test.waitAdoptionClaim") { try gate.waitForClaim() }
                let next = Task {
                    // A defective overtaking call can finish without queuing.
                    defer { didQueue.finish() }
                    return try await connection.nextPublicationWork()
                }
                following = next
                // Use the existing project signal/watchdog race: cancelling
                // AsyncStream iteration joins promptly, unlike held core work.
                try await withThrowingTaskGroup(of: Void.self) { group in
                    group.addTask {
                        for await _ in queued { return }
                        throw CloudOwnTransportFailure.missingReply
                    }
                    group.addTask {
                        try await Task.sleep(for: .seconds(10))
                        throw CloudOwnTransportFailure.missingReply
                    }
                    defer { group.cancelAll() }
                    _ = try await group.next()
                }
                reply.cancel(); next.cancel()
                try await MaintenanceLane.shared.call("test.releaseAdoptionReply") { try gate.release() }
                try await reply.value
                guard case .adoption(_, .read(let target), _) = try await next.value else {
                    Issue.record("original reply follow-up was lost or overtaken"); return
                }
                #expect(target.name == "epoch")
            } catch {
                reply.cancel(); following?.cancel()
                try? await MaintenanceLane.shared.call("test.releaseAdoptionReply") { try gate.release() }
                _ = try? await reply.value
                _ = try? await following?.value
                throw error
            }
            didQueue.finish()
        }
    }
}

extension EventTests {
    @Test func explicitAdoptionResumeWaitsForOriginalResultJobDrain() async throws {
        try await withCloudConnection { _ in
            let raw = try await rawJobConnection()
            let queued = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let connection = CoreCloudConnectionService(raw, onJobQueued: { queued.continuation.yield(()) })
            var handle = try await connectionAdoption(connection)
            let id = Data(repeating: 8, count: 16)
            let reads: [CoreBridge.CloudControlRecord] = [
                .init(encrypted: Data([8, 1, 18, 16]) + id, systemFields: Data([1])),
                .init(encrypted: Data([8, 1, 18, 16]) + id + Data([26, 32]) + Data(repeating: 19, count: 32) + Data([40, 18, 48, 2, 56, 1]), systemFields: Data([2])),
                .init(encrypted: Data([8, 1, 18, 32]) + Data(repeating: 23, count: 32), systemFields: Data([3]))
            ]
            for record in reads {
                try await connection.adoptionResult(handle, result: .read(.found(record)))
                guard case .adoption(let next, _, _) = try await connection.nextPublicationWork() else {
                    throw CloudOwnTransportFailure.unexpectedReply
                }
                handle = next
            }
            let status = try #require(try await connection.recoveryStatus())
            #expect(!status.adoptionRecoveredCancelOnly)
            #expect(!status.adoptionAfterReview)
            let gate = try await MaintenanceLane.shared.call("test.holdAdoptionReply") { try testCloudHoldAdoptionReply() }
            let pendingHandle = handle
            let original = Task { try await connection.adoptionResult(pendingHandle, result: .transportFailed(code: 4, retryAfterDurationMilliseconds: nil)) }
            var following: Task<Bool, any Error>?
            do {
                try await QueryLane.shared.call("test.waitAdoptionClaim") { try gate.waitForClaim() }
                let resumed = Task { try await connection.resumeAdoption(status.operation) }
                following = resumed
                try await withThrowingTaskGroup(of: Void.self) { group in
                    group.addTask {
                        for await _ in queued.stream { return }
                        throw CloudOwnTransportFailure.missingReply
                    }
                    group.addTask {
                        try await Task.sleep(for: .seconds(10))
                        throw CloudOwnTransportFailure.missingReply
                    }
                    defer { group.cancelAll() }
                    _ = try await group.next()
                }
                #expect(try await connection.recoveryStatus()?.adoptionTransportWait == false)
                #expect(try await connection.recoveryStatus()?.adoptionRecoveredCancelOnly == false)
                #expect(try await connection.recoveryStatus()?.adoptionAfterReview == false)
                try await MaintenanceLane.shared.call("test.releaseAdoptionReply") { try gate.release() }
                try await original.value
                #expect(try await resumed.value)
                guard case .adoption(_, .fetch(_, let cursor, _), _) = try await connection.nextPublicationWork() else {
                    throw CloudOwnTransportFailure.unexpectedReply
                }
                #expect(cursor == nil)
                try await connection.cancelOperation(status.operation)
            } catch {
                try? await MaintenanceLane.shared.call("test.releaseAdoptionReply") { try gate.release() }
                _ = try? await original.value
                _ = try? await following?.value
                throw error
            }
            queued.continuation.finish()
        }
    }
}
