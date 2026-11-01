import Foundation
import Synchronization
import Testing
@testable import CoreBridge

private final class CleanupTransport: CloudOwnTransport, Sendable {
    let operation: AssetExecutorOperation
    init(_ operation: AssetExecutorOperation) { self.operation = operation }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        guard case .zoneInventory = command else { throw CloudOwnTransportFailure.unexpectedReply }
        return operation
    }
}
private func cleanupCommand(_ fixture: CloudKitBridgeFixture, policy: CoreCloudPolicyService) async throws -> CloudPolicyCommand {
    let sdk = try await fixture.engine().instanceID
    let fetch = try await fixture.core.beginFetch(sdk: sdk, zones: [fixture.zone])
    try await fixture.core.willFetch(sdk: sdk, group: "cleanup", manual: true)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: true, failed: false)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: false, failed: false)
    try await fixture.core.didFetch(sdk: sdk, group: "cleanup", manual: true)
    try #require(try await fixture.core.finishFetch(sdk: sdk, id: fetch))
    guard case .commands(let commands) = try await policy.input(.refresh(.cloud)) else { throw BridgeError.internal("Expected drain") }
    return try #require(commands.first { if case .runCleanup = $0.action { true } else { false } })
}
extension EventTests {
    @Test func cleanupRetainsCommandAndConsumesOriginalInventoryOnce() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let command = try await cleanupCommand(fixture, policy: policy)
            let execution = try #require(try await policy.prepareExecution(command))
            let pass = try await execution.beginCleanupPass()
            let operation = AssetExecutorOperation(outcome: .success(.zoneInventory(zones: [])))
            let worker = CloudCleanupExecutor(transport: CleanupTransport(operation))
            await operation.terminal.open()
            try await worker.run(pass)
            #expect(operation.enqueueCalls.withLock { $0 } == 1)
            #expect(operation.replyCalls.withLock { $0 } == 1)
            await #expect(throws: (any Error).self) { _ = try await execution.beginCleanupPass() }
            await #expect(throws: (any Error).self) { _ = try await pass.completeInventory(zones: []) }
        }
    }
    @Test(arguments: [false, true])
    func cleanupCancellationAndCloseWaitForOriginalTerminal(closing: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let command = try await cleanupCommand(fixture, policy: policy)
            let execution = try #require(try await policy.prepareExecution(command))
            let pass = try await execution.beginCleanupPass()
            let operation = AssetExecutorOperation(outcome: .failure(CancellationError()))
            let worker = CloudCleanupExecutor(transport: CleanupTransport(operation))
            let finished = Mutex(false)
            let running = Task { defer { finished.withLock { $0 = true } }; try await worker.run(pass) }
            try await withCloudTask(gates: [operation.terminal], operation: {
                if case .success = await running.result { Issue.record("Cancelled cleanup succeeded") }
            }, whileRunning: {
                try await operation.terminal.waitForArrival()
                let close = closing ? Task { await worker.close() } : nil
                if !closing { running.cancel() }
                do {
                    try await operation.waitUntilCancelled()
                    #expect(!finished.withLock { $0 })
                    await operation.terminal.open()
                    await close?.value
                } catch {
                    await operation.terminal.open(); await close?.value; throw error
                }
            })
            #expect(operation.replyCalls.withLock { $0 } == 1)
            try await pass.finish()
        }
    }
    @Test func cleanupPauseBeforeEnqueueRefusesOriginalOperation() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let command = try await cleanupCommand(fixture, policy: policy)
            let execution = try #require(try await policy.prepareExecution(command))
            let pass = try await execution.beginCleanupPass()
            _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
            let operation = AssetExecutorOperation(outcome: .success(.zoneInventory(zones: [])))
            let worker = CloudCleanupExecutor(transport: CleanupTransport(operation))
            await #expect(throws: (any Error).self) { try await worker.run(pass) }
            #expect(operation.enqueueCalls.withLock { $0 } == 0)
            #expect(operation.replyCalls.withLock { $0 } == 0)
        }
    }
}

extension EventTests {
    @Test func cleanupInventoryFailureCommitsNoObservationAndDoesNotRetry() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let command = try await cleanupCommand(fixture, policy: policy)
            let operation = AssetExecutorOperation(outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let native = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter, ownTransport: CleanupTransport(operation))
            await operation.terminal.open()
            let result = await native.execute(command)
            #expect(result.error == nil && result.cleanupInventoryError != nil && !result.deferred)
            #expect(!CloudCleanupDiagnostics.localFailures().isEmpty)
            #expect(operation.enqueueCalls.withLock { $0 } == 1)
            #expect(operation.replyCalls.withLock { $0 } == 1)
            let duplicate = await native.execute(command)
            #expect(duplicate.error != nil)
            #expect(operation.enqueueCalls.withLock { $0 } == 1)
            await native.drainCleanups()
        }
    }
}
