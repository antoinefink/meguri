import Foundation
import Testing
@testable import CoreBridge

extension EventTests {
    @Test func cloudFixturesSupplyTheirOwnPlatformAcrossHostLifetimes() async throws {
        try await withCloudConnection(status: .temporarilyUnavailable, identity: nil) { f in
            await #expect(throws: BridgeError.cloudConnection(.accountUnavailable)) { try await f.connection.beginConnectionPreview() }
        }
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            try await f.core.validateSDK(engine.instanceID)
            #expect(await f.adapter.lastFailure == nil)
            try await f.adapter.accountChanged(status: .temporarilyUnavailable, identity: nil)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
        }
        try await withCloudConnection { f in
            _ = try await f.ready()
        }
        try await withCloudKitFixture { f in
            let engine = try await f.engine()
            try await f.core.validateSDK(engine.instanceID)
            #expect(await f.adapter.lastFailure == nil)
        }
    }

    @Test(arguments: [CloudAccountStatus.noAccount, .temporarilyUnavailable, .restricted, .couldNotDetermine, .available])
    func connectionPreviewRejectsUnavailableOrUnverifiedPlatform(status: CloudAccountStatus) async throws {
        try await withCloudConnection(status: status, identity: nil) { f in
            await #expect(throws: BridgeError.cloudConnection(.accountUnavailable)) { try await f.connection.beginConnectionPreview() }
            let recovery = try await f.connection.recoveryStatus()
            #expect(recovery == nil)
        }
    }

    @Test func connectionPreviewAndConfirmationUseTheActualHostAndConsumeTheRetainedPreview() async throws {
        try await withCloudConnection { f in
            let preview = try await f.ready()
            let operation = try await f.connection.confirmPublication(preview)
            let status = try #require(try await f.connection.recoveryStatus())
            #expect(status.step == .confirmed && !status.switchSent)
            do { _ = try await f.connection.confirmPublication(preview); Issue.record("consumed preview accepted") }
            catch { #expect(error as? BridgeError == .cloudConnection(.consumed)) }
            try await f.connection.cancelOperation(operation)
            #expect(try await f.connection.recoveryStatus() == nil)
        }
    }
    @Test func connectionStaleConfirmationAndWrongOwnerKeepNewPreviewAuthority() async throws {
        try await withCloudConnection { f in
            let first = try await f.ready()
            let second = try await f.ready()
            let other = try await f.workflow.connection()
            do { _ = try await other.confirmPublication(second); Issue.record("wrong owner accepted") }
            catch { #expect(error as? BridgeError == .cloudConnection(.wrongOwner)) }
            do { try await f.connection.cancelPreview(first); Issue.record("old preview cancelled new one") }
            catch { #expect(error as? BridgeError == .cloudConnection(.authorityChanged)) }
            _ = try await f.connection.confirmPublication(second)
        }
    }
    @Test func retainedOwnRequestRechecksPauseAtEnqueueAndRejectsHeldReplyAfterClose() async throws {
        try await withCloudConnection { f in
            let preview = try await f.ready()
            _ = try await f.connection.confirmPublication(preview)
            let (request, command, _) = try await f.own()
            guard case .createZones(let zones) = command else { throw CloudOwnTransportFailure.unexpectedReply }
            #expect(zones.count == 3 && zones.contains("meguri-root") && zones.contains("meguri-media"))
            let queue = ConnectionQueue()
            try await f.workflow.setPaused(true)
            do { _ = try await f.connection.submitPublicationRequest(request, queue: queue); Issue.record("paused request enqueued") } catch {}
            #expect(queue.count.withLock { $0 } == 0)
            try await f.connection.publicationResult(request, result: .notStarted)
            try await f.workflow.setPaused(false)
            try await f.connection.trigger()
            let (next, _, _) = try await f.own()
            #expect(try await f.connection.submitPublicationRequest(next, queue: queue))
            #expect(queue.count.withLock { $0 } == 1)
            try await f.collection.close()
            do { try await f.connection.publicationResult(next, result: .failed(retryAfterMilliseconds: nil)); Issue.record("closed host accepted reply") }
            catch { #expect(error as? BridgeError == .cloudConnection(.closed)) }
        }
    }
    @Test func connectionReadsFoundWithoutEAndMissingAsDifferentActualCoreDecisions() async throws {
        try await withCloudConnection { f in
            guard case .readActive(let preview, _) = try await f.connection.beginConnectionPreview() else { throw CloudOwnTransportFailure.unexpectedReply }
            guard case .ready(_, let summary) = try await f.connection.connectionActiveRead(preview, read: .found(.init(encrypted: nil, systemFields: Data([8])))) else { throw CloudOwnTransportFailure.unexpectedReply }
            #expect(summary.decision == .cloudUnreadable)
            _ = try await f.ready()
        }
    }
    @Test func executorUsesGuardedAdmissionAndCloseDrainsTheOriginalQueuedReply() async throws {
        try await withCloudConnection { f in
            let preview = try await f.ready()
            _ = try await f.connection.confirmPublication(preview)
            let held = HeldOwnOperation()
            let executor = CloudOwnExecutor(core: f.connection, transport: HeldOwnTransport(operation: held))
            // Drive however many real local transitions Rust requires. A
            // matching existing sidecar decision may skip WriteDecision.
            let operation = Task {
                defer { held.finishAdmissionWait() }
                for _ in 0..<8 {
                    let progress = try await executor.runNext()
                    if held.count.withLock({ $0 }) == 1 { return progress }
                    if case .idle = progress { throw CloudOwnTransportFailure.unexpectedReply }
                }
                throw CloudOwnTransportFailure.unexpectedReply
            }
            do { try await held.waitUntilQueued() }
            catch { _ = try await operation.value; throw error }
            #expect(held.count.withLock { $0 } == 1)
            let closing = Task { await executor.close() }
            try await held.waitUntilCancelled()
            held.finish(.publication(.failed(retryAfterMilliseconds: nil)))
            _ = try await operation.value
            await closing.value
            do { _ = try await executor.runNext(); Issue.record("closed executor resumed") }
            catch { #expect(error is CloudOwnTransportFailure) }
        }
    }
}
