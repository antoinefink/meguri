import Foundation
#if canImport(CloudKit)
import CloudKit
#endif
import Instrumentation
import func MeguriCoreBindings.testCloudZoneDeletionFixture
import func MeguriCoreBindings.testCloudZoneDeletionRows
import Synchronization
import Testing
@testable import CoreBridge

private func deletionRows() async throws -> [String: Bool] {
    try await MaintenanceLane.shared.call(.testCloudZoneDeletionRows) {
        Dictionary(uniqueKeysWithValues: try MeguriCoreBindings.testCloudZoneDeletionRows().map { ($0.zone, $0.due) })
    }
}
private func deletionFixture(_ fixture: CloudKitBridgeFixture) async throws -> [String] {
    let sdk = try await fixture.engine().instanceID
    return try await MaintenanceLane.shared.call(.testCloudZoneDeletionFixture) {
        try MeguriCoreBindings.testCloudZoneDeletionFixture(sdk: sdk)
    }
}
private func deletionCommand(_ fixture: CloudKitBridgeFixture, policy: CoreCloudPolicyService) async throws -> CloudPolicyCommand {
    let sdk = try await fixture.engine().instanceID
    let fetch = try await fixture.core.beginFetch(sdk: sdk, zones: [fixture.zone])
    let group = "zone-deletion-\(fetch)"
    try await fixture.core.willFetch(sdk: sdk, group: group, manual: true)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: true, failed: false)
    try await fixture.core.fetchZone(sdk: sdk, zone: fixture.zone, will: false, failed: false)
    try await fixture.core.didFetch(sdk: sdk, group: group, manual: true)
    let drained = try await fixture.core.finishFetch(sdk: sdk, id: fetch)
    try #require(drained)
    guard case .commands(let commands) = try await policy.input(.refresh(.cloud)) else { throw BridgeError.internal("Expected actual drain") }
    return try #require(commands.first { if case .deleteZones = $0.action { true } else { false } })
}
private func deletionPass(_ fixture: CloudKitBridgeFixture, policy: CoreCloudPolicyService) async throws -> CoreCloudZoneDeletionPass {
    let command = try await deletionCommand(fixture, policy: policy)
    let prepared = try await policy.prepareExecution(command)
    let execution = try #require(prepared)
    let pass = try await execution.beginZoneDeletionPass()
    return try #require(pass)
}
private func activeRead(_ fixture: CloudKitBridgeFixture) -> CloudZoneDeletionRead {
    .init(read: .found(.init(encrypted: fixture.root.envelope, systemFields: fixture.root.systemFields ?? Data())))
}
private func deletionFollowupCommands(_ result: CloudPolicyNativeResult) throws -> [CloudPolicyCommand] {
    try #require(result.outputs.count == 1)
    guard case .commands(let commands) = result.outputs[0] else { throw BridgeError.internal("Expected deletion refresh projection") }
    return commands
}
private func requireDeletionChangesFollowup(_ result: CloudPolicyNativeResult) throws {
    let commands = try deletionFollowupCommands(result)
    try #require(commands.count == 1)
    guard case .updatePending(.changes) = commands[0].action else {
        throw BridgeError.internal("Expected only the pending changes update")
    }
}
private final class DeletionTransport: CloudOwnTransport, Sendable {
    let read: AssetExecutorOperation
    let deletion: AssetExecutorOperation
    let selections = Mutex<[[String]]>([])
    let failPreparation: Bool
    init(read: AssetExecutorOperation, deletion: AssetExecutorOperation, failPreparation: Bool = false) {
        self.read = read; self.deletion = deletion; self.failPreparation = failPreparation
    }
    func prepare(_ command: CloudOwnTransportCommand) throws -> any CloudPreparedOwnOperation {
        if failPreparation { throw CloudOwnTransportFailure.unexpectedReply }
        switch command {
        case .zoneDeletionActiveRead(let target):
            #expect(target == .init(zone: "meguri-root", name: "active", recordType: "MGActive")); return read
        case .deleteZones(let batch): selections.withLock { $0.append(batch.zones) }; return deletion
        default: throw CloudOwnTransportFailure.unexpectedReply
        }
    }
}
extension EventTests {
    @Test func deletionWithZeroRealDueRowsDoesNoNetworkWork() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
            ])
            try await fixture.adapter.requestFetch(zones: ["meguri-root", fixture.zone])
            guard case .commands(let commands) = try await policy.input(.refresh(.cloud)) else {
                throw BridgeError.internal("Expected actual first drain")
            }
            let issued = try #require(commands.first { if case .deleteZones = $0.action { true } else { false } })
            let before = try await deletionRows()
            #expect(before.isEmpty)
            let unused = AssetExecutorOperation(outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let transport = DeletionTransport(read: unused, deletion: unused)
            let native = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter, ownTransport: transport)
            let result = await native.execute(issued)
            #expect(result.error == nil && result.zoneDeletionError == nil && !result.deferred)
            try requireDeletionChangesFollowup(result)
            #expect(unused.enqueueCalls.withLock { $0 } == 0)
            let after = try await deletionRows()
            #expect(after.isEmpty)
        }
    }
    @Test func deletionUsesRetainedOriginalReadAndOneBatchWithRealDurableEffects() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let zones = try await deletionFixture(fixture)
            let initialRows = try await deletionRows()
            #expect(initialRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
            let before = try await fixture.core.durableSnapshot()
            let read = AssetExecutorOperation(outcome: .success(.zoneDeletionActiveRead(activeRead(fixture))))
            let results = [CloudZoneDeletionResult(zone: zones[0], failure: nil),
                .init(zone: zones[1], failure: .init(code: 26)), .init(zone: zones[2], failure: .init(code: 6, retryAfterMilliseconds: 123_000))]
            let deletion = AssetExecutorOperation(outcome: .success(.deletedZones(.init(results: results, terminal: .init(code: 2, retryAfterMilliseconds: 123_000)))))
            let transport = DeletionTransport(read: read, deletion: deletion)
            let native = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter, ownTransport: transport)
            let issued = try await deletionCommand(fixture, policy: policy)
            try await withCloudTask(gates: [read.terminal, deletion.terminal], operation: {
                let result = await native.execute(issued)
                #expect(result.error == nil && result.zoneDeletionError == nil)
                try requireDeletionChangesFollowup(result)
            }, whileRunning: {
                try await read.terminal.waitForArrival()
                #expect(transport.selections.withLock { $0.isEmpty })
                let duplicate = await native.execute(issued)
                #expect(duplicate.error == nil && duplicate.outputs.isEmpty && !duplicate.deferred)
                await read.terminal.open()
                try await deletion.terminal.waitForArrival()
                #expect(transport.selections.withLock { $0 } == [zones])
                let heldRows = try await deletionRows()
                #expect(heldRows.count == zones.count)
                await deletion.terminal.open()
            })
            let remainingRows = try await deletionRows()
            #expect(remainingRows == [zones[2]: true])
            // Collection/root state is untouched by deletion of retained zones.
            let after = try await fixture.core.durableSnapshot()
            #expect(after.binding == before.binding && after.rootEnvelope == before.rootEnvelope && after.rootSystemFields == before.rootSystemFields)
            #expect(after.entities == before.entities && after.inboxNames == before.inboxNames)
            let diagnostics = try await fixture.core.diagnostics().zoneDeletions
            #expect(diagnostics.last == .deleted(results: results, terminal: .init(code: 2, retryAfterMilliseconds: 123_000)))
        }
    }
    @Test(arguments: [CloudZoneDeletionLocalFailure.preparation, .queueRejected, .transport])
    func deletionLocalFailuresRetainEvidenceWithoutAppErrorOrDurableSuccess(category: CloudZoneDeletionLocalFailure) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let zones = try await deletionFixture(fixture)
            let before = try await policy.status()
            let read = AssetExecutorOperation(accept: category != .queueRejected, outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let unused = AssetExecutorOperation(outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let transport = DeletionTransport(read: read, deletion: unused, failPreparation: category == .preparation)
            let native = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter, ownTransport: transport)
            let issued = try await deletionCommand(fixture, policy: policy)
            await read.terminal.open()
            let result = await native.execute(issued)
            #expect(result.error == nil && result.zoneDeletionError != nil && !result.deferred)
            try requireDeletionChangesFollowup(result)
            #expect(CloudZoneDeletionDiagnostics.localFailures().contains(.localFailure(category: category, failure: .init(code: nil))))
            let remainingRows = try await deletionRows()
            #expect(remainingRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
            #expect(transport.selections.withLock { $0.isEmpty } && unused.enqueueCalls.withLock { $0 } == 0)
            let after = try await policy.status()
            #expect(after.state == before.state && after.asOfMilliseconds == before.asOfMilliseconds)
            #expect(after.lastAttempt?.exchange == before.lastAttempt?.exchange && after.lastCompleted?.exchange == before.lastCompleted?.exchange)
            if category == .transport {
                let diagnostics = try await fixture.core.diagnostics().zoneDeletions
                #expect(diagnostics.last == .read(found: false, allowed: 0, failure: nil, terminal: .init(code: nil)))
            }
        }
    }
    @Test func deletionReadFailureKeepsOriginalCodesAndRetryFactsInCore() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let zones = try await deletionFixture(fixture)
            let failure = CloudZoneDeletionFailure(code: 6, retryAfterMilliseconds: 123_000)
            let terminal = CloudZoneDeletionFailure(code: 2, retryAfterMilliseconds: 456_000)
            let read = AssetExecutorOperation(outcome: .success(.zoneDeletionActiveRead(.init(read: .failed, failure: failure, terminal: terminal))))
            let unused = AssetExecutorOperation(outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let native = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                ownTransport: DeletionTransport(read: read, deletion: unused))
            await read.terminal.open()
            let issued = try await deletionCommand(fixture, policy: policy)
            let result = await native.execute(issued)
            #expect(result.error == nil && result.zoneDeletionError == nil)
            let diagnostics = try await fixture.core.diagnostics().zoneDeletions
            #expect(diagnostics.last == .read(found: false, allowed: 0, failure: failure, terminal: terminal))
            let remainingRows = try await deletionRows()
            #expect(remainingRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
            #expect(unused.enqueueCalls.withLock { $0 } == 0)
        }
    }
    @Test func deletionEncryptedResetReturnsActualPolicyStopFollowup() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let zones = try await deletionFixture(fixture)
            let reset = CloudZoneDeletionFailure(code: 26, encryptedDataReset: true)
            let read = AssetExecutorOperation(outcome: .success(.zoneDeletionActiveRead(.init(read: .zoneMissing, failure: reset))))
            let unused = AssetExecutorOperation(outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let native = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                ownTransport: DeletionTransport(read: read, deletion: unused))
            await read.terminal.open()
            let issued = try await deletionCommand(fixture, policy: policy)
            let result = await native.execute(issued)
            #expect(result.error == nil && result.zoneDeletionError == nil)
            let commands = try deletionFollowupCommands(result)
            #expect(commands.contains { if case .stopEngine = $0.action { true } else { false } })
            let diagnostics = try await fixture.core.diagnostics().zoneDeletions
            #expect(diagnostics.last == .read(found: false, allowed: 0, failure: reset, terminal: nil))
            let remainingRows = try await deletionRows()
            #expect(remainingRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
            #expect(unused.enqueueCalls.withLock { $0 } == 0)
        }
    }
    @Test(arguments: [false, true], [false, true])
    func deletionCancellationAndCloseJoinOriginalTerminal(deleting: Bool, closing: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let zones = try await deletionFixture(fixture)
            let pass = try await deletionPass(fixture, policy: policy)
            let read = AssetExecutorOperation(outcome: .success(.zoneDeletionActiveRead(activeRead(fixture))))
            let deletion = AssetExecutorOperation(outcome: .success(.deletedZones(.init(results: zones.map { .init(zone: $0, failure: .init(code: 20)) }))))
            let executor = CloudZoneDeletionExecutor(transport: DeletionTransport(read: read, deletion: deletion))
            let finished = Mutex(false)
            let run = Task {
                defer { finished.withLock { $0 = true } }
                try await executor.run(pass)
            }
            try await withCloudTask(gates: [read.terminal, deletion.terminal], expectedArrivals: deleting ? [read.terminal, deletion.terminal] : [read.terminal], operation: {
                let result = await run.result
                if closing {
                    expectAssetExecutorFailure(result, .closed)
                } else {
                    guard case .failure(let error) = result else {
                        Issue.record("Expected caller cancellation after the original reply joined"); return
                    }
                    #expect(error is CancellationError)
                }
            }, whileRunning: {
                try await read.terminal.waitForArrival()
                let original: AssetExecutorOperation
                if deleting { await read.terminal.open(); try await deletion.terminal.waitForArrival(); original = deletion }
                else { original = read }
                let close: Task<Void, Never>?
                if closing { close = Task { await executor.close() } }
                else { close = nil; run.cancel() }
                do {
                    try await original.waitUntilCancelled()
                    #expect(!finished.withLock { $0 })
                    let heldRows = try await deletionRows()
                    #expect(heldRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
                    await original.terminal.open()
                    if let close { await close.value }
                } catch {
                    await original.terminal.open()
                    if let close { await close.value }
                    throw error
                }
            })
            #expect(read.replyCalls.withLock { $0 } == 1)
            #expect(deletion.replyCalls.withLock { $0 } == (deleting ? 1 : 0))
            let remainingRows = try await deletionRows()
            #expect(remainingRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
        }
    }
    @Test func deletionPauseAfterOriginalReadAdmissionPreventsBatchAndDrainsRead() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let zones = try await deletionFixture(fixture)
            let pass = try await deletionPass(fixture, policy: policy)
            let read = AssetExecutorOperation(outcome: .success(.zoneDeletionActiveRead(activeRead(fixture))))
            let deletion = AssetExecutorOperation(outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let transport = DeletionTransport(read: read, deletion: deletion)
            let executor = CloudZoneDeletionExecutor(transport: transport)
            try await withCloudTask(gates: [read.terminal], operation: {
                let result = await Task { try await executor.run(pass) }.result
                if case .success = result { Issue.record("Expected paused deletion pass to fail") }
            }, whileRunning: {
                try await read.terminal.waitForArrival()
                _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
                await read.terminal.open()
            })
            #expect(transport.selections.withLock { $0.isEmpty })
            #expect(read.replyCalls.withLock { $0 } == 1)
            let remainingRows = try await deletionRows()
            #expect(remainingRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
            await executor.close()
        }
    }
    @Test func deletionDeniedBatchQueueKeepsDueRowsWithoutAReplacement() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let zones = try await deletionFixture(fixture)
            let pass = try await deletionPass(fixture, policy: policy)
            let read = AssetExecutorOperation(outcome: .success(.zoneDeletionActiveRead(activeRead(fixture))))
            let deletion = AssetExecutorOperation(accept: false, outcome: .failure(CloudOwnTransportFailure.unexpectedReply))
            let executor = CloudZoneDeletionExecutor(transport: DeletionTransport(read: read, deletion: deletion))
            await read.terminal.open()
            let result = await Task { try await executor.run(pass) }.result
            expectAssetExecutorFailure(result, .queueRejected)
            #expect(deletion.replyCalls.withLock { $0 } == 0)
            let remainingRows = try await deletionRows()
            #expect(remainingRows == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
            await executor.close()
        }
    }
}

extension EventTests {
    /// K12.5/K13.1/P24: actual native classification -> event -> lane -> Rust.
    /// Exact names are proved by one due row leaving while the other rows stay.
    @Test func fetchedOtherEpochDeletionUsesOriginalSDKAndExactNames() async throws {
        try await withCloudKitFixture(keepAlive: true) { f in
            let old = try await f.engine()
            let zones = try await deletionFixture(f)
            let before = try await f.core.durableSnapshot()
            #expect(try await deletionRows() == Dictionary(uniqueKeysWithValues: zones.map { ($0, true) }))
            #if canImport(CloudKit)
            let event = try NativeCloudKitIdentityValidation.databaseDeletionEvent(modified: [],
                deleted: [.init(zoneName: zones[0], ownerName: CKCurrentUserDefaultName)], collectionZone: f.zone)
            #else
            let event = CloudEngineEvent.fetchedCollectionZoneDeletions([zones[0]])
            #endif
            await old.emitRetained(event, label: "otherEpochDeleted")
            await old.emitRetained(event, label: "otherEpochReplay")
            #expect(await f.adapter.lastFailure == nil)
            #expect(try await deletionRows() == Dictionary(uniqueKeysWithValues: zones.dropFirst().map { ($0, true) }))
            #expect(try await f.core.durableSnapshot() == before)
            let cursor = Data([0x12, 0x05])
            await old.emitRetained(.stateUpdate(cursor), label: "afterOtherEpoch")
            #expect(try await f.core.durableSnapshot().serialization == cursor)
            try await f.adapter.configureAutomatic(true)
            try await f.adapter.waitForIdle()
            let replacement = try await f.engine()
            #expect(replacement.instanceID != old.instanceID)
            await old.emitRetained(.fetchedCollectionZoneDeletions([zones[1]]), label: "oldOtherEpoch")
            #expect(try await deletionRows()[zones[1]] == true)
            try await f.core.validateSDK(replacement.instanceID)
            await replacement.emitRetained(.fetchedCollectionZoneDeletions([zones[1]]), label: "currentOtherEpoch")
            #expect(try await deletionRows() == [zones[2]: true])
        }
    }

    /// Same real intake fault as fetched records; failed metadata closes intake
    /// before a later opaque state can pass it, with original cancel/drain order.
    @Test func fetchedOtherEpochDeletionFailureKeepsRowsAndCursor() async throws {
        try await withCloudKitFixture(keepAlive: true, failInbox: true) { f in
            let engine = try await f.engine()
            let zones = try await deletionFixture(f)
            let rows = try await deletionRows()
            let before = try await f.core.durableSnapshot()
            await engine.emitRetained(.fetchedCollectionZoneDeletions(zones), label: "failedOtherEpoch")
            await engine.emitRetained(.stateUpdate(Data([0xff])), label: "afterFailedOtherEpoch")
            try await f.adapter.waitForIdle()
            #expect(await f.adapter.lastFailure != nil)
            #expect(try await deletionRows() == rows)
            let after = try await f.core.durableSnapshot()
            #expect(after.serialization == before.serialization)
            #expect(after.entities == before.entities && after.inboxNames == before.inboxNames)
            #expect(after.trackingStopped)
            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
            #expect(await f.factory.engines().count == 1)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("failedOtherEpoch")))
            let cancel = try #require(trace.firstIndex(of: .cancelEntered))
            let drained = try #require(trace.firstIndex(of: .cancelReturned))
            let released = try #require(trace.firstIndex(of: .released))
            #expect(returned < cancel && cancel < drained && drained < released)
            await engine.emitRetained(.stateUpdate(Data([0xfe])), label: "lateOtherEpochState")
            #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
            #expect(try await f.core.durableSnapshot() == after)
        }
    }
}
