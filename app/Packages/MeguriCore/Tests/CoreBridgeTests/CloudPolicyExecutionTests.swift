import Foundation
import Instrumentation
import func MeguriCoreBindings.testCloudPolicyExchangeObservation
import Synchronization
import Testing
@testable import CoreBridge

private struct PolicyExchangeObservation: Sendable, Equatable {
    let policyExchange: UInt64
    let mappedRuntimeID: UInt64?
    let captures: [UInt64: UInt64]
    let completedIDs: Set<UInt64>
    let activeID: UInt64?
}

private func observePolicyExchange(_ command: CoreBridge.CloudPolicyCommand) async throws -> PolicyExchangeObservation {
    let raw = command.raw
    return try await MaintenanceLane.shared.call(.testCloudPolicyExchangeObservation) {
        let value = try MeguriCoreBindings.testCloudPolicyExchangeObservation(command: raw)
        return PolicyExchangeObservation(policyExchange: value.policyExchange, mappedRuntimeID: value.mappedRuntimeId,
            captures: value.captures, completedIDs: Set(value.completedIds), activeID: value.activeId)
    }
}

/// Assert the original policy request's one actual capture and qualified result.
@discardableResult
private func qualifiedRecordExchange(_ command: CoreBridge.CloudPolicyCommand, result: CloudPolicyNativeResult,
    fixture: CloudKitBridgeFixture, before: PolicyExchangeObservation, outcome: CloudCollectionOutcome,
    alsoCompleted: Set<UInt64> = []) async throws -> CloudExchangeResult {
    guard case .runExchange(let policyID, let source) = command.action else {
        throw BridgeError.internal("Expected original RunExchange")
    }
    let observed = try await observePolicyExchange(command)
    #expect(observed.policyExchange == policyID)
    let runtimeID = try #require(observed.mappedRuntimeID)
    #expect(before.mappedRuntimeID == nil && before.captures[policyID] == nil)
    #expect(observed.captures == before.captures.merging([policyID: runtimeID]) { _, new in new })
    #expect(!before.completedIDs.contains(runtimeID) && !alsoCompleted.contains(runtimeID))
    #expect(observed.completedIDs == before.completedIDs.union(alsoCompleted).union([runtimeID]))
    #expect(observed.activeID == nil)
    let stored = try #require(try await fixture.core.exchangeResult(id: runtimeID))
    #expect(stored.id == runtimeID && stored.collection == outcome)
    let records = try result.outputs.flatMap(policyCommands).compactMap { command -> CoreBridge.CloudPolicyExchangeRecord? in
        if case .recordExchange(let record) = command.action { record } else { nil }
    }
    try #require(records.count == 1)
    let record = try #require(records.first)
    #expect(record.exchange == policyID && record.source == source)
    switch (record.outcome, stored.collection) {
    case (.succeeded(let selected, _, _, _, _), .succeeded):
        #expect(selected == UInt64(stored.selectedNames.count))
    case (.cancelled, .cancelled): break
    case (.failed(let error), .failed): #expect(error == .other)
    default: Issue.record("RecordExchange outcome differs from the actual stored Runtime result")
    }
    #expect(try await fixture.core.exchangeResult(id: runtimeID) == stored)
    #expect(try await observePolicyExchange(command) == observed)
    return stored
}

private func policyCommands(_ output: CloudPolicyOutput) throws -> [CloudPolicyCommand] {
    guard case .commands(let commands) = output else { throw BridgeError.internal("Unexpected policy output") }
    return commands
}
@discardableResult
func consumePolicySetup(_ output: CloudPolicyOutput, executor: NativeCloudPolicyExecutor) async throws -> Int {
    var pending = try policyCommands(output)
    var next = 0
    var starts = 0
    while next < pending.count {
        try #require(next < 64, "Policy fixture setup exceeded its bounded command budget")
        let command = pending[next]
        next += 1
        if case .startEngine = command.action { starts += 1 }
        switch command.action {
        case .startEngine, .stopEngine, .configureAutomatic, .updatePending, .applyReady, .recordExchange, .recordQuietPoint, .probeTracking, .statusChanged:
            let result = await executor.execute(command)
            if let error = result.error { throw error }
            for followup in result.outputs { pending.append(contentsOf: try policyCommands(followup)) }
        default: break // This setup starts with automatic/media off; no native read is simulated.
        }
    }
    return starts
}
func nativePolicy(_ fixture: CloudKitBridgeFixture, gate: (any CloudPolicyExchangeGate)? = nil) async throws -> (CoreCloudPolicyService, NativeCloudPolicyExecutor) {
    try #require(await fixture.factory.engines().isEmpty, "Policy fixture must not bypass Start")
    try await fixture.requireNativePolicyBaseline()
    let policy = try await fixture.core.policy()
    let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter, exchangeGate: gate)
    // Keep the actual settings gate closed until every setup input is projected.
    let disabledStarts = try await consumePolicySetup(policy.input(.settings(.init(enabled: false, paused: false, automatic: false, media: false))), executor: executor)
    let accountStarts = try await consumePolicySetup(policy.refreshAccount(), executor: executor)
    let networkStarts = try await consumePolicySetup(policy.input(.network(reachable: true)), executor: executor)
    let enginesBeforeEnable = await fixture.factory.engines()
    try #require(disabledStarts == 0 && accountStarts == 0 && networkStarts == 0 && enginesBeforeEnable.isEmpty,
                 "Policy fixture must wait for settings to open the actual Start gate")
    let starts = try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false))), executor: executor)
    try #require(starts == 1, "Policy fixture must issue exactly one gate-opening Start")
    let engines = await fixture.factory.engines()
    try #require(engines.count == 1, "Policy Start must create exactly one actual SDK")
    let engine = try #require(engines.first)
    try await fixture.core.validateSDK(engine.instanceID)
    try #require(await fixture.adapter.retainedInstanceIDs() == [engine.instanceID])
    let status = try await policy.status()
    try #require(status.accountStatus == .available && status.accountMatchesBinding)
    return (policy, executor)
}

private struct CancelledPolicyGate: CloudPolicyExchangeGate {
    func waitBeforeCapture(_ command: CloudPolicyCommand) async throws -> CloudPolicyEarlyEnd? { .cancelled }
}

private func pendingCommand(_ policy: CoreCloudPolicyService, cause: CloudPolicyChangeCause = .local) async throws -> CloudPolicyCommand {
    let commands = try policyCommands(try await policy.input(.refresh(cause)))
    let updates = commands.filter { if case .updatePending = $0.action { true } else { false } }
    try #require(updates.count == 1)
    return try #require(updates.first)
}

private func addPendingDeck(_ fixture: CloudKitBridgeFixture, name: String) async throws {
    let added = try await CoreDeckService().addDeck(name: name)
    try await fixture.core.awaitTracking(generation: added.summary.generation, seq: added.summary.seq)
}

/// Faults and pauses only SDK operations. The original Core command, pending
/// take, update application and acknowledgement all use production code.
private actor PolicyPendingEngine: CloudKitEngine {
    nonisolated let instanceID: String
    private let sdk: CloudKitEngineFake
    private var applyError: (any Error)?
    private var snapshotPause: CloudCallbackGate?
    private var applyPause: CloudCallbackGate?
    private var received: [CloudPendingUpdate] = []
    init(_ sdk: CloudKitEngineFake) { self.sdk = sdk; instanceID = sdk.instanceID }
    func failNextApply(_ error: any Error) { applyError = error }
    func pauseNextSnapshot(_ gate: CloudCallbackGate) { snapshotPause = gate }
    func pauseNextApply(_ gate: CloudCallbackGate) { applyPause = gate }
    func receivedUpdates() -> [CloudPendingUpdate] { received }
    func pendingRecordZoneChanges() async -> [String: CloudPendingChange] {
        if let gate = snapshotPause { snapshotPause = nil; await gate.enter() }
        return await sdk.pendingRecordZoneChanges()
    }
    func pendingDatabaseChanges() async -> Set<String> { await sdk.pendingDatabaseChanges() }
    func applyPendingUpdate(_ update: CloudPendingUpdate) async throws -> CloudPendingUpdate {
        received.append(update)
        if let gate = applyPause { applyPause = nil; await gate.enter(); try Task.checkCancellation() }
        if let error = applyError { applyError = nil; throw error }
        return await sdk.applyPendingUpdate(update)
    }
    func removePendingDatabaseChanges() async { await sdk.removePendingDatabaseChanges() }
    func waitForEventBookkeeping() async throws { try await sdk.waitForEventBookkeeping() }
    func prepareSend(zones: [String]) async throws -> CloudKitNetworkEnqueue { try await sdk.prepareSend(zones: zones) }
    func prepareFetch(options: CloudFetchOptions) async throws -> CloudKitNetworkEnqueue { try await sdk.prepareFetch(options: options) }
    func sendChanges(zones: [String]) async throws { try await sdk.sendChanges(zones: zones) }
    func fetchChanges(options: CloudFetchOptions) async throws { try await sdk.fetchChanges(options: options) }
    func cancelOperations() async { await sdk.cancelOperations() }
    func release() async { await sdk.release() }
}

/// Expected lists are derived at each producer boundary, before SDK delivery.
private func expectPendingDelivery(_ expected: [CloudPendingUpdate], fixture: CloudKitBridgeFixture,
    engine: CloudKitEngineFake) async throws {
    let ids = expected.map(\.id)
    let observations = try await fixture.core.observations()
    let emissions = observations.filter {
        if case .corePendingEmitted(_, let id) = $0 { ids.contains(id) } else { false }
    }
    #expect(emissions == expected.map { .corePendingEmitted(kind: $0.kind, actionID: $0.id) })
    let applied = await engine.recordedTrace().filter {
        if case .pendingApplied(let id, _, _) = $0 { ids.contains(id) } else { false }
    }
    #expect(applied == expected.map { .pendingApplied($0.id, $0.removes.sorted(), $0.adds.keys.sorted()) })
    for update in expected {
        let acknowledged = observations.compactMap { observation -> String? in
            if case .pendingAdded(let name, let id) = observation, id == update.id { name } else { nil }
        }
        #expect(acknowledged.sorted() == update.adds.keys.sorted())
    }
}

private struct PolicyPendingApplyFailure: Error {}

extension EventTests {
    @Test(arguments: [false, true], [CloudPolicyChangeCause.local, .cloud])
    func nativeUpdatePendingDeliversCoreAdditionsWithoutNetworkCall(automatic: Bool, cause: CloudPolicyChangeCause) async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            if automatic {
                try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            }
            let engine = try await f.engine()
            let before = await engine.pendingRecordZoneChanges()
            let emitted = try await f.core.emittedUpdateIDs()
            let calls = engine.acceptedNetworkCalls.value
            try await addPendingDeck(f, name: "Ordinary pending addition")
            let command = try await pendingCommand(policy, cause: cause)
            // Tracking has already emitted Full; isolate the explicit command.
            let commandBoundary = try await f.core.emittedUpdateIDs()
            let trackingIDs = commandBoundary.filter { !emitted.contains($0) }
            try #require(trackingIDs.count == 1)
            let trackingID = try #require(trackingIDs.first)
            let sendables = try await f.core.trackingSendables()
            let kind: CloudUpdateKind = cause == .local ? .full : .changes
            let expectedUpdate: CloudPolicyUpdate = cause == .local ? .full : .changes
            guard case .updatePending(let update) = command.action else { Issue.record("Expected UpdatePending"); return }
            #expect(update == expectedUpdate)
            let delivered = Mutex<[CloudPolicyOutput]>([])
            let result = await executor.execute(command, corePrepared: {}, acknowledgeOutputs: { values in
                delivered.withLock { $0.append(contentsOf: values) }
            })
            #expect(result.error == nil && !result.deferred && result.afterOutputs == nil)
            // Local Core output still belongs to the original coordinator ACK.
            #expect(result.outputs.count == 1 && result.acknowledgedOutputCount == 0)
            #expect(delivered.withLock { $0.isEmpty })
            _ = try policyCommands(try #require(result.outputs.first))
            let allEmitted = try await f.core.emittedUpdateIDs()
            let ids = allEmitted.filter { !commandBoundary.contains($0) }
            try #require(ids.count == 1, "The explicit command must emit exactly one list")
            let id = try #require(ids.first)
            #expect(allEmitted.filter { !emitted.contains($0) } == [trackingID, id])
            let after = await engine.pendingRecordZoneChanges()
            let added = Set(after.keys).subtracting(before.keys)
            try #require(added.count == 1, "The real local deck must reach the idle SDK")
            let currentSendables = try await f.core.trackingSendables()
            #expect(after == sendables && after == currentSendables)
            for name in added { #expect(after[name] == .save) }
            // Changes compares with desired, already updated by tracking Full.
            try await expectPendingDelivery([
                CloudPendingUpdate(id: trackingID, kind: .full, adds: sendables, removes: []),
                CloudPendingUpdate(id: id, kind: kind, adds: kind == .full ? sendables : [:], removes: [])
            ], fixture: f, engine: engine)
            #expect(engine.acceptedNetworkCalls.value == calls && engine.automaticallySync == automatic)
        }
    }

    @Test func nativeUpdatePendingApplyFailureRetainsCoreOutputBeforeRefresh() async throws {
        try await withNativePolicyFixture { f in
            let wrapped = Mutex<PolicyPendingEngine?>(nil)
            await f.factory.wrapNextEngine { sdk in
                let engine = PolicyPendingEngine(sdk); wrapped.withLock { $0 = engine }; return engine
            }
            let (policy, executor) = try await nativePolicy(f)
            let engine = try #require(wrapped.withLock { $0 })
            let sdk = try await f.engine()
            let before = await sdk.pendingRecordZoneChanges()
            let received = await engine.receivedUpdates().count
            try await addPendingDeck(f, name: "Pending apply failure")
            let command = try await pendingCommand(policy)
            await engine.failNextApply(PolicyPendingApplyFailure())
            let deliveries = Mutex(0)
            let inputs = Mutex(0)
            let result = await CloudPolicyOutputAcknowledgement.$current.withValue({ outputs in
                deliveries.withLock { $0 += outputs.count }
            }) {
                await CloudPolicyOutputAcknowledgement.$input.withValue({ work in
                    inputs.withLock { $0 += 1 }; return try await work()
                }) { await executor.execute(command) }
            }
            #expect(result.error is PolicyPendingApplyFailure)
            #expect(result.outputs.count == 1 && result.acknowledgedOutputCount == 0 && !result.deferred)
            _ = try policyCommands(try #require(result.outputs.first))
            #expect(deliveries.withLock { $0 } == 0 && inputs.withLock { $0 } == 0)
            let update = try #require(await engine.receivedUpdates().dropFirst(received).first)
            let additions = Set(update.adds.keys).subtracting(before.keys)
            try #require(additions.count == 1)
            #expect(await sdk.pendingRecordZoneChanges() == before)
            let observations = try await f.core.observations()
            for name in additions { #expect(!observations.contains(.pendingAdded(name: name, actionID: update.id))) }
            let followup = try #require(result.afterOutputs)
            let refreshed = await followup()
            #expect(refreshed.error == nil && refreshed.outputs.count == 1 && refreshed.acknowledgedOutputCount == 1)
            #expect(inputs.withLock { $0 } == 1 && deliveries.withLock { $0 } == 0)
            #expect(sdk.acceptedNetworkCalls.value == 0)
        }
    }

    @Test func nativeUpdatePendingCancellationDoesNotAcknowledgeUnappliedAdditions() async throws {
        try await withNativePolicyFixture { f in
            let wrapped = Mutex<PolicyPendingEngine?>(nil)
            await f.factory.wrapNextEngine { sdk in
                let engine = PolicyPendingEngine(sdk); wrapped.withLock { $0 = engine }; return engine
            }
            let (policy, executor) = try await nativePolicy(f)
            let engine = try #require(wrapped.withLock { $0 })
            let sdk = try await f.engine()
            let before = await sdk.pendingRecordZoneChanges()
            let received = await engine.receivedUpdates().count
            try await addPendingDeck(f, name: "Cancelled pending apply")
            let command = try await pendingCommand(policy)
            let gate = CloudCallbackGate()
            await engine.pauseNextApply(gate)
            let running = Task {
                let result = await executor.execute(command)
                _ = await gate.operationSucceeded()
                return result
            }
            do {
                try await gate.waitForArrival()
                running.cancel()
                await gate.open()
                let result = await running.value
                #expect(result.error is CancellationError)
                #expect(result.outputs.count >= 1 && !result.deferred)
                _ = try policyCommands(try #require(result.outputs.first))
                let update = try #require(await engine.receivedUpdates().dropFirst(received).first)
                let observations = try await f.core.observations()
                for name in Set(update.adds.keys).subtracting(before.keys) {
                    #expect(!observations.contains(.pendingAdded(name: name, actionID: update.id)))
                }
                #expect(await sdk.pendingRecordZoneChanges() == before)
                #expect(await gate.pendingWaiterCount() == 0 && sdk.acceptedNetworkCalls.value == 0)
            } catch { running.cancel(); await gate.open(); _ = await running.value; throw error }
        }
    }

    @Test func nativeUpdatePendingRejectsOriginalGuardAndRetiredSDK() async throws {
        try await withNativePolicyFixture { f in
            let wrapped = Mutex<PolicyPendingEngine?>(nil)
            await f.factory.wrapNextEngine { sdk in
                let engine = PolicyPendingEngine(sdk); wrapped.withLock { $0 = engine }; return engine
            }
            let (policy, executor) = try await nativePolicy(f)
            let engine = try #require(wrapped.withLock { $0 })
            let sdk = try await f.engine()
            let before = await sdk.pendingRecordZoneChanges()
            try await addPendingDeck(f, name: "Pending authority race")
            let command = try await pendingCommand(policy)
            let gate = CloudCallbackGate()
            await engine.pauseNextSnapshot(gate)
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(command)
                #expect(result.error == nil && result.outputs.count == 1)
            }, whileRunning: {
                try await gate.waitForArrival()
                // Invalidate the actual owner while the adapter awaits SDK state.
                try await f.core.retireSDK(sdk.instanceID, policyStop: false)
                await gate.open()
            })
            #expect(await sdk.pendingRecordZoneChanges() == before)
            _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
            let emitted = try await f.core.emittedUpdateIDs()
            let trace = await sdk.recordedTrace()
            let stale = await executor.execute(command)
            #expect(stale.error as? BridgeError == .cloudPolicy(.staleCommand))
            #expect(try await f.core.emittedUpdateIDs() == emitted)
            #expect(await sdk.recordedTrace() == trace && sdk.acceptedNetworkCalls.value == 0)
        }
    }

    @Test func nativeOriginalRefusalKeepsLegacyErrorAndImmutableAttemptEvidence() async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            let command = try await pendingCommand(policy)
            let current = await executor.execute(command)
            #expect(current.error == nil && current.originalGuardRefusal == nil)
            _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
            let emitted = try await f.core.emittedUpdateIDs()
            let refused = await executor.execute(command)
            #expect(refused.error as? BridgeError == .cloudPolicy(.staleCommand))
            #expect(refused.originalGuardRefusal != nil && !refused.deferred)
            #expect(try await f.core.emittedUpdateIDs() == emitted)
            do {
                _ = try await policy.executeCore(command)
                Issue.record("Legacy accessor accepted the original stale command")
            } catch { #expect(error as? BridgeError == .cloudPolicy(.staleCommand)) }
            _ = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false)))
            #expect(current.originalGuardRefusal == nil && current.error == nil)
            #expect(refused.error as? BridgeError == .cloudPolicy(.staleCommand))
            #expect(refused.originalGuardRefusal != nil)
        }
    }

    @Test func nativeGenuineFailureThenPauseAndStaleReexecutionKeepsOriginalFailureAndOutputs() async throws {
        try await withNativePolicyFixture { f in
            let wrapped = Mutex<PolicyPendingEngine?>(nil)
            await f.factory.wrapNextEngine { sdk in
                let engine = PolicyPendingEngine(sdk); wrapped.withLock { $0 = engine }; return engine
            }
            let (policy, executor) = try await nativePolicy(f)
            let engine = try #require(wrapped.withLock { $0 })
            try await addPendingDeck(f, name: "Original error before Pause")
            let command = try await pendingCommand(policy)
            await engine.failNextApply(PolicyPendingApplyFailure())
            let deliveries = Mutex(0)
            let original = await CloudPolicyOutputAcknowledgement.$current.withValue({ outputs in
                deliveries.withLock { $0 += outputs.count }
            }) {
                await CloudPolicyOutputAcknowledgement.$input.withValue({ work in try await work() }) {
                    await executor.execute(command)
                }
            }
            #expect(original.error is PolicyPendingApplyFailure && original.originalGuardRefusal == nil)
            #expect(original.outputs.count == 1 && original.acknowledgedOutputCount == 0)
            #expect(deliveries.withLock { $0 } == 0)
            _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: false, media: false)))
            let later = await executor.execute(command)
            #expect(later.error as? BridgeError == .cloudPolicy(.staleCommand))
            #expect(later.originalGuardRefusal != nil)
            let followup = try #require(original.afterOutputs)
            // The caller still owns the original output ACK before this refresh.
            _ = try policyCommands(try #require(original.outputs.first))
            _ = await followup()
            #expect(original.error is PolicyPendingApplyFailure && original.originalGuardRefusal == nil)
            #expect(original.outputs.count == 1 && original.acknowledgedOutputCount == 0)
        }
    }

    @Test func nativeUpdatePendingDefersOrderedListsUntilOriginalCallbackReturns() async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            let engine = try await f.engine()
            let before = await engine.pendingRecordZoneChanges()
            let emitted = try await f.core.emittedUpdateIDs()
            let gate = CloudCallbackGate()
            await engine.pauseNextCallback(gate)
            let expected = try await withCloudTask(gates: [gate], operation: {
                await engine.emitRetained(.stateUpdate(Data([0x70, 0x34])), label: "pendingBridge")
            }, whileRunning: {
                try await gate.waitForArrival()
                var expected: [CloudPendingUpdate] = []
                var previous = emitted
                for cause in [CloudPolicyChangeCause.local, .cloud] {
                    try await addPendingDeck(f, name: "Deferred pending \(cause)")
                    let command = try await pendingCommand(policy, cause: cause)
                    let commandBoundary = try await f.core.emittedUpdateIDs()
                    let trackingIDs = commandBoundary.filter { !previous.contains($0) }
                    try #require(trackingIDs.count == 1)
                    let trackingID = try #require(trackingIDs.first)
                    let sendables = try await f.core.trackingSendables()
                    let kind: CloudUpdateKind = cause == .local ? .full : .changes
                    let expectedUpdate: CloudPolicyUpdate = cause == .local ? .full : .changes
                    guard case .updatePending(let update) = command.action else { throw BridgeError.internal("Expected UpdatePending") }
                    #expect(update == expectedUpdate)
                    let result = await executor.execute(command)
                    #expect(result.error == nil && result.outputs.count == 1 && result.acknowledgedOutputCount == 0)
                    previous = try await f.core.emittedUpdateIDs()
                    let commandIDs = previous.filter { !commandBoundary.contains($0) }
                    try #require(commandIDs.count == 1, "The explicit command must emit exactly one list")
                    let commandID = try #require(commandIDs.first)
                    expected.append(CloudPendingUpdate(id: trackingID, kind: .full, adds: sendables, removes: []))
                    expected.append(CloudPendingUpdate(id: commandID, kind: kind, adds: kind == .full ? sendables : [:], removes: []))
                    #expect(previous.filter { !emitted.contains($0) } == expected.map(\.id))
                }
                #expect(await engine.pendingRecordZoneChanges() == before)
                await gate.open()
                return expected
            })
            let ids = try await f.core.emittedUpdateIDs().filter { !emitted.contains($0) }
            #expect(ids == expected.map(\.id))
            try await expectPendingDelivery(expected, fixture: f, engine: engine)
            let trace = await engine.recordedTrace()
            let returned = try #require(trace.firstIndex(of: .eventReturned("pendingBridge")))
            let applied = trace.compactMap { event -> UInt64? in
                if case .pendingApplied(let id, _, _) = event, ids.contains(id) { id } else { nil }
            }
            #expect(applied == ids)
            #expect(!trace[..<returned].contains { if case .pendingApplied(let id, _, _) = $0 { ids.contains(id) } else { false } })
            let after = await engine.pendingRecordZoneChanges()
            let additions = Set(after.keys).subtracting(before.keys)
            #expect(additions.count == 2)
            #expect(after == (try await f.core.trackingSendables()))
            #expect(await gate.pendingWaiterCount() == 0 && engine.acceptedNetworkCalls.value == 0)
        }
    }
}

extension EventTests {
    @Test func nativePolicySetupRejectsLocalOnlyOpeningBeforeSDKCreation() async throws {
        try await withCloudKitFixture(startAdapter: false) { fixture in
            await #expect(throws: (any Error).self) { _ = try await nativePolicy(fixture) }
            #expect(await fixture.factory.engines().isEmpty)
            #expect(await fixture.adapter.retainedInstanceIDs().isEmpty)
        }
    }

    /// P4: a scheduling change during the actual native factory await revokes
    /// the issued start; its own SDK drains before the retained replacement runs.
    @Test func nativePolicyModeOffDuringHeldFactoryReleasesSupersededSDK() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let count = await fixture.factory.engines().count
            let gate = CloudCallbackGate()
            await fixture.factory.pauseNextCreation(gate)
            let on = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
            let first = try #require(try policyCommands(on).first { if case .configureAutomatic = $0.action { true } else { false } })
            let replacement = try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(first)
                #expect(result.error != nil)
            }, whileRunning: {
                try await gate.waitForArrival()
                #expect(await fixture.factory.engines().count == count + 1)
                let off = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false)))
                let retained = try #require(try policyCommands(off).first { if case .configureAutomatic = $0.action { true } else { false } })
                await gate.open()
                return retained
            })
            let superseded = try await fixture.engine()
            #expect(await superseded.recordedTrace().contains(.released))
            #expect(await fixture.adapter.retainedInstanceIDs().isEmpty)
            let result = await executor.execute(replacement)
            #expect(result.error == nil)
            let actual = try await fixture.engine()
            #expect(!actual.automaticallySync && actual.instanceID != superseded.instanceID)
            try await fixture.core.validateSDK(actual.instanceID)
        }
    }

    @Test func nativePolicyConfigurationDrainsOriginalBeforeReplacement() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let original = try await fixture.engine()
            let count = await fixture.factory.engines().count
            let gate = CloudCallbackGate()
            await original.pauseCancellation(gate)
            let output = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
            let command = try #require(try policyCommands(output).first { if case .configureAutomatic = $0.action { true } else { false } })
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(command)
                if let error = result.error { throw error }
            }, whileRunning: {
                try await gate.waitForArrival()
                #expect(await fixture.factory.engines().count == count)
                #expect(await fixture.adapter.retainedInstanceIDs().contains(original.instanceID))
                await gate.open()
            })
            #expect(await original.recordedTrace().contains(.released))
            let replacement = try await fixture.engine()
            #expect(replacement.instanceID != original.instanceID)
            #expect(replacement.automaticallySync)
            try await fixture.core.validateSDK(replacement.instanceID)
            #expect(await fixture.adapter.retainedInstanceIDs() == [replacement.instanceID])
        }
    }

    /// P12.3/P12.7: typed cancellation before capture uses the real facade's early-end port.
    @Test func nativePolicyPreCaptureCancellationCreatesNoRuntimeExchange() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture, gate: CancelledPolicyGate())
            let output = try await policy.input(.syncNowRequested(.command))
            let command = try #require(try policyCommands(output).first { if case .runExchange = $0.action { true } else { false } })
            let before = try await observePolicyExchange(command)
            let result = await executor.execute(command)
            #expect(result.error == nil && !result.deferred)
            let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
            var records: [CloudPolicyCommand] = []
            for command in policyOutputCommands {
                if case .recordExchange = command.action { records.append(command) }
            }
            let record = try #require(records.first)
            if case .recordExchange(let value) = record.action {
                guard case .cancelled = value.outcome else { Issue.record("pre-capture cancellation became success"); return }
            }
            let after = try await observePolicyExchange(command)
            #expect(before.mappedRuntimeID == nil && after.mappedRuntimeID == nil)
            #expect(after == before)
            #expect(records.count == 1)
            if case .recordExchange(let value) = record.action {
                #expect(value.exchange == before.policyExchange)
            }
            let persisted = await executor.execute(record)
            #expect(persisted.error == nil)
            #expect(try await policy.status().lastAttempt != nil)
            #expect(try await observePolicyExchange(command) == before)
        }
    }

    /// P12.2/P12.4: the existing worker completes the facade's actual capture.
    @Test func nativePolicyExchangeCompletesActualRuntimeCapture() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let originalAtSend = Mutex<UInt64?>(nil)
            await engine.observeNextSendFinalEntry { _ in
                if case .some(.runExchange(let exchange, .command)) = CloudMediaCancellationContext.selectedAction {
                    originalAtSend.withLock { $0 = exchange }
                }
            }
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil),
            ])
            let output = try await policy.input(.syncNowRequested(.command))
            let command = try #require(try policyCommands(output).first { if case .runExchange = $0.action { true } else { false } })
            let before = try await observePolicyExchange(command)
            let result = await executor.execute(command)
            #expect(result.error == nil)
            #expect(originalAtSend.withLock { $0 } == before.policyExchange)
            let completed = try await qualifiedRecordExchange(command, result: result, fixture: fixture, before: before, outcome: .succeeded)
            #expect(!completed.selectedNames.isEmpty)
            let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
            var records: [CloudPolicyCommand] = []
            for command in policyOutputCommands {
                if case .recordExchange = command.action { records.append(command) }
            }
            #expect(records.count == 1)
            #expect(!(await engine.recordedTrace()).contains(.reentrantNetworkCall))
        }
    }

    /// P12.3/V3.4: cancellation after capture drains the original SDK and
    /// qualifies the result through Runtime before facade.completeExchange.
    @Test func nativePolicyPostCaptureCancellationDrainsOriginalOperation() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            await engine.appendFetchScripts([.manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil)])
            let gate = CloudCallbackGate()
            await engine.pauseAfterBatch(gate)
            let output = try await policy.input(.syncNowRequested(.command))
            let command = try #require(try policyCommands(output).first { if case .runExchange = $0.action { true } else { false } })
            let before = try await observePolicyExchange(command)
            guard case .runExchange(let id, _) = command.action else { return }
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(command)
                #expect(result.error is CancellationError)
                try await qualifiedRecordExchange(command, result: result, fixture: fixture, before: before, outcome: .cancelled)
                let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
                let records = policyOutputCommands.filter { if case .recordExchange = $0.action { true } else { false } }
                let issued = try #require(records.first)
                guard case .recordExchange(let record) = issued.action, case .cancelled = record.outcome else {
                    Issue.record("cancelled SDK call had no qualified cancelled result"); return
                }
            }, whileRunning: {
                try await gate.waitForArrival()
                let cancel = try await policy.input(.syncNowCancelled(exchange: id))
                let retained = try #require(try policyCommands(cancel).first { if case .cancelExchange = $0.action { true } else { false } })
                let result = await executor.execute(retained)
                #expect(result.error == nil)
            })
            #expect(await engine.recordedTrace().contains(.cancelReturned))
            let observed = try await observePolicyExchange(command)
            let runtimeID = try #require(observed.mappedRuntimeID)
            #expect(try await fixture.core.exchangeResult(id: runtimeID)?.collection == .cancelled)
            #expect(!(try await fixture.core.status()).syncingCollection)
        }
    }
}

@Suite struct CloudPolicyExecutionLaneTests {
    @Test @MainActor func everyNativeExecutionSelectorUsesMaintenanceLane() async throws {
        let names: [BridgeCall] = [.testCloudPolicyExchangeObservation, .cloudPolicyExecutionBeginMedia, .cloudPolicyExecutionContinueMediaAfterSend, .cloudPolicyExchangeReadiness, .cloudPolicyExecutionOperationActivity, .cloudPolicyPrepareExecution, .cloudPolicyExecutionOriginalSdk,
                                  .cloudPolicyExecutionValidateExecution, .cloudPolicyExecutionRegisterSdk,
                                  .cloudPolicyExecutionRetireOriginal, .cloudPolicyExecutionBeginFetch, .cloudPolicyExecutionBeginSend]
        for name in names {
            let ran = try await CoreCloudPolicyService.call(name, probe)
            #expect(ran.queueLabel == Lane.maintenance.queueLabel && !ran.isMainThread)
        }
    }
}

private final class NativePolicyOriginalFailure: Error {}

private struct ImmediatePolicyReadinessDeadline: CloudPolicyReadinessClock {
    func waitForDeadline() async throws { try Task.checkCancellation() }
}

extension EventTests {
    @Test func nativePolicyDefaultReadinessWaitsForHeldFactoryWithoutCapturing() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let gate = CloudCallbackGate()
            await fixture.factory.pauseNextCreation(gate)
            let mode = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
            let configure = try #require(try policyCommands(mode).first { if case .configureAutomatic = $0.action { true } else { false } })
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(configure)
                if let error = result.error { throw error }
            }, whileRunning: {
                try await gate.waitForArrival()
                let output = try await policy.input(.syncNowRequested(.command))
                let command = try #require(try policyCommands(output).first { if case .runExchange = $0.action { true } else { false } })
                let before = try await observePolicyExchange(command)
                let timed = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter, readinessClock: ImmediatePolicyReadinessDeadline())
                let ended = await timed.execute(command)
                #expect(ended.error == nil)
                let policyOutputCommands: [CloudPolicyCommand] = try ended.outputs.flatMap(policyCommands)
                var records: [CloudPolicyCommand] = []
                for command in policyOutputCommands {
                    if case .recordExchange = command.action { records.append(command) }
                }
                #expect(records.count == 1)
                let record = try #require(records.first)
                guard case .recordExchange(let value) = record.action, case .waiting = value.outcome else {
                    Issue.record("held native creation must end waiting at its deadline"); return
                }
                #expect(try await observePolicyExchange(command) == before)
                #expect(before.mappedRuntimeID == nil)
                #expect(value.exchange == before.policyExchange)
                #expect(await timed.execute(record).error == nil)
                #expect(try await policy.status().lastAttempt != nil)
                await gate.open()
            })
        }
    }

    @Test(arguments: [false, true])
    func nativePolicyHeldFactoryRejectsAccountOrClose(close: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let gate = CloudCallbackGate()
            await fixture.factory.pauseNextCreation(gate)
            let output = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
            let command = try #require(try policyCommands(output).first { if case .configureAutomatic = $0.action { true } else { false } })
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(command)
                #expect(result.error != nil)
            }, whileRunning: {
                try await gate.waitForArrival()
                if close { try await fixture.collection.close() }
                else { try await fixture.core.accountChanged(sdk: nil, status: .available, identity: Data("synthetic-account-B".utf8)) }
                await gate.open()
            })
            let superseded = try await fixture.engine()
            #expect(await superseded.recordedTrace().contains(.cancelReturned))
            #expect(await superseded.recordedTrace().contains(.released))
            #expect(superseded.acceptedNetworkCalls.value == 0)
            #expect(await fixture.adapter.retainedInstanceIDs().isEmpty)
        }
    }

    /// Real Runtime invalidation during the last native preparation wait. The
    /// fake validates inside its owned task, as Native does before the SDK call.
    @Test(arguments: [false, true], [CloudFenceTransition.accountSwitch, .token, .generation])
    func nativeFinalEnqueueRejectsHeldWork(send: Bool, transition: CloudFenceTransition) async throws {
        try await withNativePolicyFixture { fixture in
            _ = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let gate = CloudCallbackGate()
            await engine.pauseNativeReturnedWork(gate)
            try await withCloudTask(gates: [gate], operation: {
                do {
                    if send { try await fixture.adapter.requestSend(zones: [fixture.zone]) }
                    else { try await fixture.adapter.requestFetch(zones: ["meguri-root", fixture.zone]) }
                    Issue.record("old native work began after invalidation")
                } catch { }
            }, whileRunning: {
                try await gate.waitForArrival()
                // Invalidate through real Core facts. Drain after releasing the
                // held preparation, since stop correctly joins original networks.
                switch transition {
                case .accountSwitch: try await fixture.core.accountChanged(sdk: nil, status: .available, identity: Data("synthetic-account-B".utf8))
                case .token: try await fixture.core.setPaused(true)
                default: try await fixture.collection.close()
                }
                await gate.open()
            })
            #expect(engine.acceptedNetworkCalls.value == 0)
            #expect(!(await engine.recordedTrace()).contains { if case .sendEntered = $0 { true } else if case .fetchEntered = $0 { true } else { false } })
        }
    }

    @Test func nativePolicyPostCaptureErrorRecordsActualFailureAndLaterSyncNowWorks() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let failure = NativePolicyOriginalFailure()
            await engine.failNextOperation(failure)
            let issued = try await policy.input(.syncNowRequested(.command))
            let command = try #require(try policyCommands(issued).first { if case .runExchange = $0.action { true } else { false } })
            let before = try await observePolicyExchange(command)
            let result = await executor.execute(command)
            #expect((result.error as? NativePolicyOriginalFailure) === failure)
            let actual = try await qualifiedRecordExchange(command, result: result, fixture: fixture, before: before, outcome: .failed)
            #expect(actual.collection == .failed)
            #expect(!actual.selectedNames.isEmpty && !actual.outstandingNames.isEmpty)
            let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
            var records: [CloudPolicyCommand] = []
            for command in policyOutputCommands {
                if case .recordExchange = command.action { records.append(command) }
            }
            #expect(records.count == 1)
            #expect(try await policy.status().error?.error == .other)
            if let record = records.first { #expect(await executor.execute(record).error == nil) }
            let failed = try await observePolicyExchange(command)
            #expect(failed.completedIDs == before.completedIDs.union([actual.id]))
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil),
            ])
            let later = try await fixture.adapter.syncNow()
            #expect(later.id > actual.id && later.collection == .succeeded)
            #expect(try await fixture.core.exchangeResult(id: later.id) == later)
            let after = try await observePolicyExchange(command)
            #expect(after.captures == failed.captures && after.activeID == nil)
            #expect(after.completedIDs == failed.completedIDs.union([later.id]))
            #expect(try await fixture.core.exchangeResult(id: actual.id) == actual)
        }
    }

    @Test func nativePolicyReservesBeforeCaptureWhilePublicWorkerDrains() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
            ] + Array(repeating: .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil), count: 5))
            let gate = CloudCallbackGate()
            let publicResult = Mutex<CloudExchangeResult?>(nil)
            await engine.pauseAfterBatch(gate)
            let (command, before, publicID) = try await withCloudTask(gates: [gate], operation: {
                let returned = try await fixture.adapter.syncNow()
                #expect(returned.collection == .succeeded)
                #expect(try await fixture.core.exchangeResult(id: returned.id) == returned)
                publicResult.withLock { $0 = returned }
            }, whileRunning: {
                try await gate.waitForArrival()
                let issued = try await policy.input(.syncNowRequested(.command))
                let command = try #require(try policyCommands(issued).first { if case .runExchange = $0.action { true } else { false } })
                let before = try await observePolicyExchange(command)
                let publicID = try #require(before.activeID)
                #expect(before.mappedRuntimeID == nil && !before.completedIDs.contains(publicID))
                try await withCloudTask(gates: [gate], expectedArrivals: [], operation: {
                    let result = await executor.execute(command)
                    #expect(result.error == nil)
                    let completed = try await qualifiedRecordExchange(command, result: result, fixture: fixture,
                        before: before, outcome: .succeeded, alsoCompleted: [publicID])
                    #expect(completed.id > publicID)
                    let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
                    let filteredRecordExchange = policyOutputCommands.filter { if case .recordExchange = $0.action { true } else { false } }
                    #expect(filteredRecordExchange.count == 1)
                }, whileRunning: {
                    try await fixture.adapter.waitForPolicyReservationWaiter()
                    #expect(try await observePolicyExchange(command) == before)
                    await gate.open()
                })
                return (command, before, publicID)
            })
            let first = try #require(publicResult.withLock { $0 })
            #expect(first.id == publicID)
            let after = try await observePolicyExchange(command)
            let policyID = try #require(after.mappedRuntimeID)
            #expect(policyID > publicID)
            #expect(try await fixture.core.exchangeResult(id: publicID) == first)
            #expect(try await fixture.core.exchangeResult(id: policyID)?.collection == .succeeded)
            #expect(after.completedIDs == before.completedIDs.union([publicID, policyID]))
            #expect(after.completedIDs.count == before.completedIDs.count + 2)
            let next = try await fixture.adapter.syncNow()
            #expect(next.id > policyID && next.collection == .succeeded)
            #expect(try await fixture.core.exchangeResult(id: next.id) == next)
            let final = try await observePolicyExchange(command)
            #expect(final.captures == after.captures && final.activeID == nil)
            #expect(final.completedIDs == after.completedIDs.union([next.id]))
        }
    }

    @Test(arguments: ["scheduling", "account", "sdk", "metadata"])
    func policySendRetiredBySchedulingChangeDoesNotFailPublication(mode: String) async throws {
        try await withNativePolicyFixture { fixture in
            let changeAccount = mode == "account", sdkFailure = mode == "sdk"
            let failure = NativePolicyOriginalFailure()
            let (policy, executor) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await fixture.engine()
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil)
            ])
            try await fixture.adapter.requestFetch(zones: ["meguri-root", fixture.zone])
            let pending = try policyCommands(try await policy.input(.foreground))
            let send = try #require(pending.first { if case .requestSend = $0.action { true } else { false } })
            let enteredBefore = engine.acceptedNetworkCalls.value
            let gate = CloudCallbackGate()
            if mode == "metadata" {
                await fixture.adapter.setPolicyOperationOriginObserver { await gate.enter() }
            } else if sdkFailure {
                await engine.failNextOperation(failure)
                await fixture.adapter.setSendErrorObserver { await gate.enter() }
            } else { await engine.pauseNextOperationTask(gate) }
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(send)
                #expect((result.error != nil) == (changeAccount || sdkFailure) && !result.deferred)
                if sdkFailure { #expect((result.error as? NativePolicyOriginalFailure) === failure) }
            }, whileRunning: {
                try await gate.waitForArrival()
                let changed = try policyCommands(try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false))))
                let configure = try #require(changed.first { if case .configureAutomatic = $0.action { true } else { false } })
                let original = Task { await executor.execute(configure) }
                do {
                    try await fixture.adapter.waitForStopping(instanceID: engine.instanceID)
                    if changeAccount {
                        try await fixture.core.accountChanged(sdk: nil, status: .available, identity: Data("synthetic-account-B".utf8))
                    }
                    await gate.open()
                    let result = await original.value
                    #expect((result.error != nil) == changeAccount && !result.deferred)
                } catch {
                    await gate.open()
                    _ = await original.value
                    throw error
                }
            })
            await fixture.adapter.setSendErrorObserver(nil)
            await fixture.adapter.setPolicyOperationOriginObserver(nil)
            #expect(engine.acceptedNetworkCalls.value == enteredBefore + (sdkFailure ? 1 : 0))
            #expect(await engine.recordedTrace().contains(.released))
            #expect(await fixture.factory.engines().count == (changeAccount ? 2 : 3))
            #expect(try await policy.status().error == nil)
            if !changeAccount {
                let next = try await fixture.engine()
                await next.appendFetchScripts([
                    .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                    .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil)
                ])
                let command = try #require(try policyCommands(try await policy.input(.syncNowRequested(.command)))
                    .first { if case .runExchange = $0.action { true } else { false } })
                let before = try await observePolicyExchange(command)
                let result = await executor.execute(command)
                #expect(result.error == nil)
                try await qualifiedRecordExchange(command, result: result, fixture: fixture, before: before, outcome: .succeeded)
            }
        }
    }

    @Test(arguments: [false, true])
    func policySendDuringReplacementCreationUsesNoUnreadyEngine(changeAccount: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let old = try await fixture.engine()
            await old.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil)
            ])
            try await fixture.adapter.requestFetch(zones: ["meguri-root", fixture.zone])
            let send = try #require(try policyCommands(try await policy.input(.foreground))
                .first { if case .requestSend = $0.action { true } else { false } })
            let gate = CloudCallbackGate()
            await fixture.factory.pauseNextCreation(gate)
            let configure = try #require(try policyCommands(try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false))))
                .first { if case .configureAutomatic = $0.action { true } else { false } })
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(configure)
                #expect((result.error != nil) == changeAccount && !result.deferred)
            }, whileRunning: {
                try await gate.waitForArrival()
                #expect(await fixture.adapter.retainedInstanceIDs().isEmpty)
                let pending = try await fixture.engine()
                #expect(pending.instanceID != old.instanceID)
                let calls = pending.acceptedNetworkCalls.value
                if changeAccount {
                    try await fixture.core.accountChanged(sdk: nil, status: .available, identity: Data("synthetic-account-B".utf8))
                }
                let result = await executor.execute(send)
                #expect((result.error != nil) == changeAccount && !result.deferred)
                #expect(pending.acceptedNetworkCalls.value == calls && calls == 0)
                #expect(await old.recordedTrace().contains(.released))
                await gate.open()
            })
            if changeAccount { #expect(await fixture.adapter.retainedInstanceIDs().isEmpty) }
            else {
                let ready = try await fixture.engine()
                #expect(await fixture.adapter.retainedInstanceIDs() == [ready.instanceID])
                await ready.appendFetchScripts([
                    .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                    .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil)
                ])
                let command = try #require(try policyCommands(try await policy.input(.syncNowRequested(.command)))
                    .first { if case .runExchange = $0.action { true } else { false } })
                let before = try await observePolicyExchange(command)
                let result = await executor.execute(command)
                #expect(result.error == nil)
                try await qualifiedRecordExchange(command, result: result, fixture: fixture, before: before, outcome: .succeeded)
            }
        }
    }

    @Test func nativePolicyHeldConfigureRunsAfterQualifiedExchangeEnd() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, executor) = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let count = await fixture.factory.engines().count
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil),
            ])
            let gate = CloudCallbackGate()
            await engine.pauseAfterBatch(gate)
            let issued = try await policy.input(.syncNowRequested(.command))
            let command = try #require(try policyCommands(issued).first { if case .runExchange = $0.action { true } else { false } })
            let before = try await observePolicyExchange(command)
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(command)
                #expect(result.error == nil)
                try await qualifiedRecordExchange(command, result: result, fixture: fixture, before: before, outcome: .succeeded)
                let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
                let filteredRecordExchange = policyOutputCommands.filter { if case .recordExchange = $0.action { true } else { false } }
                #expect(filteredRecordExchange.count == 1)
            }, whileRunning: {
                try await gate.waitForArrival()
                let changed = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
                let configure = try #require(try policyCommands(changed).first { if case .configureAutomatic = $0.action { true } else { false } })
                let held = await executor.execute(configure)
                #expect(held.deferred && held.error == nil)
                #expect(await fixture.factory.engines().count == count)
                #expect(!(await engine.recordedTrace()).contains(.released))
                await gate.open()
            })
            #expect(await fixture.factory.engines().count == count + 1)
            #expect(await engine.recordedTrace().contains(.released))
            let runtimeID = try #require(try await observePolicyExchange(command).mappedRuntimeID)
            #expect(try await fixture.core.exchangeResult(id: runtimeID)?.collection == .succeeded)
        }
    }
}

private struct ObservedPolicyReadinessDeadline: CloudPolicyReadinessClock {
    let gate: CloudCallbackGate
    func waitForDeadline() async throws {
        try await withTaskCancellationHandler {
            await gate.enter()
            try Task.checkCancellation()
        } onCancel: { Task { await gate.open() } }
    }
}

extension EventTests {
    @Test func nativePolicyReadinessResumesOnCreationEventWithOneCapture() async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, lifecycle) = try await nativePolicy(fixture)
            let factoryGate = CloudCallbackGate(), deadline = CloudCallbackGate()
            await fixture.factory.pauseNextCreation(factoryGate)
            let mode = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false)))
            let configure = try #require(try policyCommands(mode).first { if case .configureAutomatic = $0.action { true } else { false } })
            try await withCloudTask(gates: [factoryGate], operation: {
                let result = await lifecycle.execute(configure)
                if let error = result.error { throw error }
            }, whileRunning: {
                try await factoryGate.waitForArrival()
                let engine = try await fixture.engine()
                await engine.appendFetchScripts([
                    .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                ] + Array(repeating: .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil), count: 3))
                let executor = NativeCloudPolicyExecutor(policy: policy, adapter: fixture.adapter,
                    readinessClock: ObservedPolicyReadinessDeadline(gate: deadline))
                let requested = try await policy.input(.syncNowRequested(.command))
                let command = try #require(try policyCommands(requested).first { if case .runExchange = $0.action { true } else { false } })
                let before = try await observePolicyExchange(command)
                try await withCloudTask(gates: [factoryGate], expectedArrivals: [], operation: {
                    let result = await executor.execute(command)
                    #expect(result.error == nil)
                    try await qualifiedRecordExchange(command, result: result, fixture: fixture, before: before, outcome: .succeeded)
                    let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
                    let filteredRecordExchange = policyOutputCommands.filter { if case .recordExchange = $0.action { true } else { false } }
                    #expect(filteredRecordExchange.count == 1)
                }, whileRunning: {
                    try await deadline.waitForArrival()
                    #expect(try await observePolicyExchange(command) == before)
                    #expect(before.mappedRuntimeID == nil)
                    #expect(engine.acceptedNetworkCalls.value == 0)
                    await factoryGate.open()
                })
            })
        }
    }
}


extension EventTests {
    /// Admission precedes SDK invocation. A real Core transition while the
    /// owned task is queued must leave the actual SDK-entry counter unchanged.
    @Test(arguments: [false, true], [CloudFenceTransition.accountSwitch, .token, .generation])
    func nativeQueuedOperationRevalidatesBeforeSDKInvocation(send: Bool, transition: CloudFenceTransition) async throws {
        try await withNativePolicyFixture { fixture in
            _ = try await nativePolicy(fixture)
            let engine = try await fixture.engine()
            let gate = CloudCallbackGate()
            let calls = engine.acceptedNetworkCalls.value
            await engine.pauseNextOperationTask(gate)
            try await withCloudTask(gates: [gate], operation: {
                do {
                    if send { try await fixture.adapter.requestSend(zones: [fixture.zone]) }
                    else { try await fixture.adapter.requestFetch(zones: ["meguri-root", fixture.zone]) }
                    Issue.record("queued operation crossed invalidation")
                } catch { }
            }, whileRunning: {
                try await gate.waitForArrival()
                #expect(engine.acceptedNetworkCalls.value == calls)
                switch transition {
                case .accountSwitch: try await fixture.core.accountChanged(sdk: nil, status: .available, identity: Data("synthetic-account-B".utf8))
                case .token: try await fixture.core.setPaused(true)
                default: try await fixture.collection.close()
                }
                await gate.open()
            })
            #expect(engine.acceptedNetworkCalls.value == calls)
        }
    }
}

extension EventTests {
    @Test func nativeBackgroundFlushSendsOriginalCollectionWithoutAnExchangeOrFetch() async throws {
        try await withNativePolicyFixture { f in
            let (policy, setup) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
            let engine = try await f.engine()
            let before = await engine.recordedTrace()
            let deadline = CloudCallbackGate()
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: f.adapter, flushDeadline: { _ in
                try await withTaskCancellationHandler { await deadline.enter(); try Task.checkCancellation() }
                    onCancel: { Task { await deadline.open() } }
            })
            let output = try await policy.input(.background(deadlineMilliseconds: 1_900_000_000_000))
            let flush = try #require(try policyCommands(output).first { if case .backgroundFlush = $0.action { true } else { false } })
            let result = await executor.execute(flush)
            if let error = result.error { throw error }
            let trace = await engine.recordedTrace()
            let added = Array(trace.dropFirst(before.count))
            #expect(added.contains { if case .sendEntered = $0 { true } else { false } })
            #expect(!added.contains { if case .fetchEntered = $0 { true } else { false } })
            #expect(try await f.core.exchangeResult(id: 1) == nil)
            let name = try #require(f.names.first)
            #expect(try await f.core.entityFacts(name: name)?.ackedRev == 3)
        }
    }

    @Test func nativeExpiryCancelsOriginalHeldFlushWithoutWaitingForTheSend() async throws {
        try await withNativePolicyFixture { f in
            let (policy, setup) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: setup)
            let engine = try await f.engine()
            let held = CloudCallbackGate(), deadline = CloudCallbackGate()
            await engine.pauseAfterBatch(held)
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: f.adapter, flushDeadline: { _ in
                try await withTaskCancellationHandler { await deadline.enter(); try Task.checkCancellation() }
                    onCancel: { Task { await deadline.open() } }
            })
            let output = try await policy.input(.background(deadlineMilliseconds: 1_900_000_000_000))
            let flush = try #require(try policyCommands(output).first { if case .backgroundFlush = $0.action { true } else { false } })
            try await withCloudTask(gates: [held], operation: {
                let result = await executor.execute(flush)
                #expect(result.error != nil)
            }, whileRunning: {
                try await held.waitForArrival()
                let expiry = try await policy.input(.backgroundExpiring)
                let cancel = try #require(try policyCommands(expiry).first { if case .cancelBackgroundFlush = $0.action { true } else { false } })
                let result = await executor.execute(cancel)
                #expect(result.error == nil)
                // Only cancellation opens the held SDK call in this branch.
                #expect(await engine.recordedTrace().contains(.cancelEntered))
            })
            let name = try #require(f.names.first)
            #expect(try await f.core.entityFacts(name: name)?.mark != nil)
            #expect(try await f.core.exchangeResult(id: 1) == nil)
        }
    }

    @Test func suspendedOriginalSDKDeliveryWaitsForActualCoreResume() async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            let accounts = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let engine = try await f.engine()
            let original = engine.instanceID
            let made = await f.factory.engines().count
            let before = try await f.core.durableSnapshot()
            try await f.collection.flushForBackground()
            await accounts.freeze()
            try await f.adapter.freezeForSuspend()
            let result = try await f.collection.suspend(timeoutMilliseconds: 1_500)
            #expect(result == .suspended)
            let bytes = Data([0x73, 0x75, 0x73])
            let delivered = Task { await engine.emitRetained(.stateUpdate(bytes), label: "suspendedState") }
            let heldRead = CloudCallbackGate()
            var read: Task<CloudAppAccountReadResult, Never>?
            do {
                try await f.adapter.waitForSuspendedDelivery()
                #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
                #expect(try await f.core.durableSnapshot().serialization == before.serialization)
                _ = try await f.collection.resume()
                await accounts.resumeAfterCoreResume()
                #expect(!(await f.adapter.resumeDeliveries()))
                #expect(try await f.core.durableSnapshot().serialization == before.serialization)
                #expect(await f.adapter.retainedInstanceIDs() == [original])
                await f.factory.appendAccountRead(.success(.init(status: .available,
                    identity: Data("synthetic-account-A".utf8))), gate: heldRead)
                let id = try await accounts.begin(transition: false)
                let pending = Task { await accounts.read(id) }; read = pending
                try await heldRead.waitForArrival()
                // Local Core is usable while the sole actual platform read
                // waits. The SDK's exact original state is still undelivered.
                _ = try await CoreDeckService().addDeck(name: "Offline resume proof")
                guard case .started(let session, _, let head, _) = try await Core.startSession(deckID: f.reviewDeck) else {
                    throw BridgeError.internal("Local review did not resume while account proof waited")
                }
                let card = try #require(head.items.first)
                let answer = try await Core.answerSession(sessionID: session, cardID: card.cardId, rating: .good,
                    stateToken: card.stateToken, answeredAtMs: cloudFixtureOriginMilliseconds, elapsedMs: 100)
                if case .accepted = answer {} else { Issue.record("Local answer did not commit while account proof waited") }
                try await Core.endSession(sessionID: session)
                #expect(await engine.requestRetainedBatch(zone: f.zone) == nil)
                #expect(try await f.core.durableSnapshot().serialization == before.serialization)
                await heldRead.open()
                let proof = await pending.value
                #expect(proof.error == nil && proof.verified)
                try #require(proof.verified)
                for output in proof.outputs { try await consumePolicySetup(output, executor: executor) }
                await delivered.value
                try await f.core.validateSDK(original)
                #expect(await f.adapter.retainedInstanceIDs() == [original])
                #expect(await f.factory.engines().count == made)
                #expect(try await f.core.durableSnapshot().serialization == bytes)
                #expect(!(await engine.recordedTrace()).contains(.released))
            } catch {
                await heldRead.open()
                read?.cancel(); _ = await read?.value
                await accounts.close()
                try? await f.adapter.stop()
                await delivered.value
                throw error
            }
        }
    }

    @Test func resumedOriginalSDKStaysHeldAfterFailedReadUntilLaterFreshProof() async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            let accounts = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let engine = try await f.engine()
            let original = engine.instanceID
            let made = await f.factory.engines().count
            let before = try await f.core.durableSnapshot().serialization
            try await f.collection.flushForBackground()
            await accounts.freeze()
            try await f.adapter.freezeForSuspend()
            #expect(try await f.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
            let bytes = Data([0x66, 0x72, 0x65, 0x73, 0x68])
            let delivered = Task { await engine.emitRetained(.stateUpdate(bytes), label: "failedReadState") }
            do {
                try await f.adapter.waitForSuspendedDelivery()
                _ = try await f.collection.resume()
                await accounts.resumeAfterCoreResume()
                await f.factory.appendAccountRead(.failure(CloudOwnTransportFailure.missingReply))
                let failedID = try await accounts.begin(transition: false)
                let failed = await accounts.read(failedID)
                #expect(failed.error != nil && !failed.verified && failed.outputs.isEmpty)
                #expect(!(await f.adapter.resumeDeliveries()))
                #expect(try await f.core.durableSnapshot().serialization == before)
                #expect(await f.adapter.retainedInstanceIDs() == [original])
                // An accepted undetermined reply is also not fresh proof.
                await f.factory.appendAccountRead(.success(.init(status: .couldNotDetermine, identity: nil)))
                let unknownID = try await accounts.begin(transition: false)
                let unknown = await accounts.read(unknownID)
                #expect(unknown.error != nil && !unknown.verified && unknown.outputs.isEmpty)
                #expect(try await f.core.durableSnapshot().serialization == before)
                await f.factory.appendAccountRead(.success(.init(status: .available,
                    identity: Data("synthetic-account-A".utf8))))
                let freshID = try await accounts.begin(transition: true)
                let fresh = await accounts.read(freshID)
                #expect(fresh.error == nil && fresh.verified)
                try #require(fresh.verified)
                for output in fresh.outputs { try await consumePolicySetup(output, executor: executor) }
                await delivered.value
                try await f.core.validateSDK(original)
                #expect(try await f.core.durableSnapshot().serialization == bytes)
                #expect(await f.factory.engines().count == made)
            } catch {
                await accounts.close()
                try? await f.adapter.stop()
                await delivered.value
                throw error
            }
        }
    }

    @Test(arguments: [ResumeProofInvalidation.accountSwitch, .generation, .close])
    func heldResumeAccountReplyCannotOpenDeliveryForAnObsoleteOwner(transition: ResumeProofInvalidation) async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            let accounts = await CloudAppAccountReader(workflow: f.core, policy: policy, factory: f.factory, adapter: f.adapter)
            let engine = try await f.engine()
            let before = try await f.core.durableSnapshot().serialization
            try await f.collection.flushForBackground()
            await accounts.freeze()
            try await f.adapter.freezeForSuspend()
            #expect(try await f.collection.suspend(timeoutMilliseconds: 1_500) == .suspended)
            let delivered = Task { await engine.emitRetained(.stateUpdate(Data([0x6f, 0x6c, 0x64])), label: "obsoleteResumeState") }
            let heldRead = CloudCallbackGate()
            var read: Task<CloudAppAccountReadResult, Never>?
            do {
                try await f.adapter.waitForSuspendedDelivery()
                _ = try await f.collection.resume()
                await accounts.resumeAfterCoreResume()
                await f.factory.appendAccountRead(.success(.init(status: .available,
                    identity: Data("synthetic-account-A".utf8))), gate: heldRead)
                let id = try await accounts.begin(transition: false)
                let pending = Task { await accounts.read(id) }; read = pending
                try await heldRead.waitForArrival()
                var nextRead: UInt64?
                switch transition {
                case .accountSwitch:
                    // The later app-owned signal supersedes the held lookup.
                    await f.factory.appendAccountRead(.success(.init(status: .available,
                        identity: Data("synthetic-account-B".utf8))))
                    nextRead = try await accounts.begin(transition: true)
                case .generation:
                    // A real successor open must not be targeted by the
                    // account task's original retained Host/facades.
                    try await f.collection.close()
                    let location = CollectionLocation(collectionDirectory: f.directory.appending(path: "collection"),
                        backupsDirectory: f.directory.appending(path: "backups"),
                        appGroupSnapshot: f.directory.appending(path: "widget.snapshot"))
                    try await f.collection.open(location, randomSeed: 115)
                case .close:
                    await accounts.close()
                    try await f.adapter.stop()
                    try await f.collection.close()
                }
                await heldRead.open()
                let stale = await pending.value
                #expect(stale.error != nil && !stale.verified && stale.outputs.isEmpty)
                #expect(try await f.core.durableSnapshot().serialization == before)
                if let nextRead {
                    #expect(!(await f.adapter.resumeDeliveries()))
                    let switched = await accounts.read(nextRead)
                    #expect(!switched.verified)
                    for output in switched.outputs { try await consumePolicySetup(output, executor: executor) }
                    #expect(try await f.core.durableSnapshot().serialization == before)
                }
                // Account switch and close use the original stop/drain owner,
                // which releases the serial delivery without replaying it.
                try? await f.adapter.stop()
                await delivered.value
                #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                #expect(await engine.recordedTrace().contains(.released))
            } catch {
                await heldRead.open()
                read?.cancel(); _ = await read?.value
                await accounts.close()
                try? await f.adapter.stop()
                await delivered.value
                throw error
            }
        }
    }

}

extension EventTests {
    @Test(arguments: [false, true])
    func policyRefreshReleasesActualHeldAnswerWhenStartProjectionWasNeverObserved(automatic: Bool) async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            if automatic {
                try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            }
            if try await Core.fsrsPolicyStatus() != .ok {
                let enabled = try await Core.enableFsrs(); try await enabled.value()
            }
            guard case .started(let session, _, let head, _) = try await Core.startSession(deckID: f.reviewDeck) else {
                Issue.record("Real review session did not start"); return
            }
            do {
                let card = try #require(head.items.first)
                guard case .accepted = try await Core.answerSession(sessionID: session, cardID: card.cardId, rating: .good,
                    stateToken: card.stateToken, answeredAtMs: cloudFixtureOriginMilliseconds, elapsedMs: 100) else {
                    Issue.record("Real review answer did not commit"); try await Core.endSession(sessionID: session); return
                }
                try await Core.flushReviewForBackground()
                let engine = try await f.engine()
                // The batch barrier observes the session. The policy receives
                // neither a start projection nor a fabricated SessionEnded.
                try await f.adapter.requestSend(zones: [f.zone])
                #expect(!(await engine.recordedTrace()).contains {
                    if case .handed(_, let names) = $0 { names.contains(f.reviewName) } else { false }
                })
                let before = try await f.core.emittedUpdateIDs()
                try await Core.endSession(sessionID: session)
                let ended = try await policy.input(.refresh(.cloud))
                let actions = try policyCommands(ended)
                #expect(actions.contains { if case .updatePending(.full) = $0.action { true } else { false } })
                try await consumePolicySetup(ended, executor: executor)
                try await f.adapter.consumePendingUpdates()
                let observations = try await f.core.observations()
                let end = try #require(observations.firstIndex(of: .sessionEnded(session: session)))
                #expect(observations[end...].contains {
                    if case .corePendingEmitted(let kind, let id) = $0 { kind == .full && !before.contains(id) } else { false }
                })
                #expect(await engine.pendingRecordZoneChanges()[f.reviewName] == .save)
                let sends = actions.filter { if case .requestSend = $0.action { true } else { false } }
                #expect(sends.isEmpty == !automatic)
                if automatic {
                    for send in sends {
                        let result = await executor.execute(send)
                        if let error = result.error { throw error }
                    }
                } else { try await f.adapter.requestSend(zones: [f.zone]) }
                let sent = try #require(try await f.core.entityFacts(name: f.reviewName))
                #expect(sent.mark == nil && sent.ackedRev == sent.localRev)
                #expect(try await f.core.exchangeResult(id: 1) == nil)
            } catch { try? await Core.endSession(sessionID: session); throw error }
        }
    }
}

#if canImport(CloudKit)
extension EventTests {
    @Test func nativeMediaOffAndExpiryControlsStartDuringAHeldExchangeFetch() async throws {
        try await withNativePolicyFixture { f in
            let (policy, _) = try await nativePolicy(f)
            _ = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: true)))
            let media = try await f.core.media()
            let transport = try NativeCloudMediaTransport(queue: NativeOwnCapturingQueue(),
                staging: .init(directory: f.directory.appending(path: "control-staging"), available: { UInt64.max }))
            let executor = NativeCloudPolicyExecutor(policy: policy, adapter: f.adapter, mediaService: media, mediaTransport: transport)
            let engine = try await f.engine()
            let held = CloudCallbackGate()
            await engine.pauseNextOperationTask(held)
            let calls = engine.acceptedNetworkCalls.value
            let output = try await policy.input(.syncNowRequested(.command))
            let exchange = try #require(try policyCommands(output).first { if case .runExchange = $0.action { true } else { false } })
            try await withCloudTask(gates: [held], operation: {
                let result = await executor.execute(exchange)
                let policyOutputCommands: [CloudPolicyCommand] = try result.outputs.flatMap(policyCommands)
                let records = policyOutputCommands.filter { if case .recordExchange = $0.action { true } else { false } }
                #expect(records.count == 1)
            }, whileRunning: {
                try await held.waitForArrival()
                let off = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: false, media: false)))
                let pause = try #require(try policyCommands(off).first { if case .pauseMedia = $0.action { true } else { false } })
                #expect(await executor.execute(pause).error == nil)
                #expect(engine.acceptedNetworkCalls.value == calls)
                let expiry = try await policy.input(.backgroundExpiring)
                for command in try policyCommands(expiry) {
                    switch command.action {
                    case .cancelExchange, .cancelBackgroundFlush:
                        #expect(await executor.execute(command).error == nil)
                    default: break
                    }
                }
                #expect(await engine.recordedTrace().contains(.cancelEntered))
            })
        }
    }
}
#endif

enum ResumeProofInvalidation: Sendable { case accountSwitch, generation, close }

extension EventTests {
    @Test(arguments: [false, true], ["cancel", "token", "retire", "close", "startedFailure"])
    func nativeStartedPreparationRetainsOutputsAndRevalidatesBeforeSDK(send: Bool, transition: String) async throws {
        try await withNativePolicyFixture { fixture in
            let (policy, _) = try await nativePolicy(fixture)
            let issued = try await policy.input(.syncNowRequested(.command))
            let command = try #require(try policyCommands(issued).first { if case .runExchange = $0.action { true } else { false } })
            let execution = try #require(try await policy.prepareExecution(command))
            let sdk = try #require(try await execution.originalSDK())
            let engine = try await fixture.engine()
            let calls = engine.acceptedNetworkCalls.value
            let validateSDK = try await fixture.core.finalSDKAuthorization(sdk)
            let prepared: CloudKitNetworkEnqueue
            if send { prepared = try await engine.prepareSend(zones: [fixture.zone]) }
            else { prepared = try await engine.prepareFetch(options: .init(zones: ["meguri-root", fixture.zone], priorityZones: [], groupID: "held-started")) }
            let pendingBefore = try await policy.status().pending
            let added = try await CoreDeckService().addDeck(name: "Started preparation pending deck \(send)-\(transition)")
            try await fixture.core.awaitTracking(generation: added.summary.generation, seq: added.summary.seq)
            let held = CloudCallbackGate()
            let outputs = CloudPolicyInvocationOutputs()
            let zones: [CloudPolicyZone] = send ? [.collection] : [.root, .collection]
            let kind: CloudPolicyOperationKind = send ? .send : .fetch
            let operation = try prepared.enqueue(prepare: {
                if transition == "startedFailure" { await held.enter() }
                outputs.append(try await execution.reportInvocation(zones: zones, kind: kind))
                if transition != "startedFailure" { await held.enter() }
            }, validate: {
                try validateSDK()
                try execution.validateForEnqueue()
            })
            let deadline = CloudPolicyReadinessDecision()
            let watchdog = Task {
                do { try await Task.sleep(for: .seconds(10)) } catch { return }
                if deadline.expire() { await held.abort(CancellationError()); operation.cancel() }
            }
            let observed = Mutex<(any Error)?>(nil)
            var expected: BridgeError?
            do {
                expected = try await withCloudTask(gates: [held], operation: {
                    do { try await operation.drain(); Issue.record("invalidated Started preparation entered SDK") }
                    catch { observed.withLock { $0 = error } }
                }, whileRunning: {
                    try await held.waitForArrival()
                    #expect(engine.acceptedNetworkCalls.value == calls)
                    switch transition {
                    case "cancel": operation.cancel()
                    case "retire":
                        // Retire without synchronously joining the held task.
                        try await fixture.core.retireSDK(sdk, policyStop: false)
                    case "close": try await fixture.collection.close()
                    default: try await fixture.core.setPaused(true)
                    }
                    var expected: BridgeError?
                    if transition != "cancel" {
                        do {
                            if transition == "startedFailure" { _ = try await execution.reportInvocation(zones: zones, kind: kind) }
                            else { try validateSDK(); try execution.validateForEnqueue() }
                        } catch { expected = BridgeError(error) }
                    }
                    await held.open()
                    return expected
                })
            } catch {
                await held.open(); await operation.cancelAndDrain()
                watchdog.cancel(); await watchdog.value
                throw error
            }
            await operation.cancelAndDrain()
            let inTime = deadline.observeReady()
            watchdog.cancel(); await watchdog.value
            let retained = outputs.take()
            #expect(inTime && engine.acceptedNetworkCalls.value == calls)
            #expect(await held.pendingWaiterCount() == 0)
            if transition == "startedFailure" { #expect(retained.isEmpty) }
            else {
                #expect(retained.count == 1)
                let output = try #require(retained.first)
                #expect(try !policyCommands(output).isEmpty)
                #expect(try policyCommands(output).contains { command in
                    if case .statusChanged(let status) = command.action { status.pending == pendingBefore + 1 } else { false }
                })
            }
            if transition == "cancel" {
                #expect(observed.withLock { $0 is CancellationError })
                // Preserve the existing Ended cancellation lane and its cleanup.
                _ = try await execution.report(zones: zones, kind: kind, started: false, error: CancellationError())
                try await fixture.adapter.requestSend(zones: [fixture.zone])
                #expect(engine.acceptedNetworkCalls.value > calls)
            } else {
                let actual = observed.withLock { $0.map { BridgeError($0) } }
                #expect(expected != nil && actual == expected)
                if transition == "startedFailure" {
                    try await fixture.core.setPaused(false)
                    try await fixture.adapter.stop()
                    try await fixture.adapter.start(automaticallySync: false)
                    let successor = try await fixture.engine()
                    try await fixture.adapter.requestSend(zones: [fixture.zone])
                    #expect(successor.acceptedNetworkCalls.value > 0)
                    #expect(engine.acceptedNetworkCalls.value == calls)
                }
            }
        }
    }
}

#if DEBUG
enum DependentFetchRefusalCase: CaseIterable, Sendable, Equatable {
    case noAccount, otherAccount, noPark, noRetirement, revocationFailure, earlierFailure
    case retirementFailure
    var qualifies: Bool { self == .noAccount || self == .otherAccount }
}

extension EventTests {
    // Real issued commands and real Core account projection/StopEngine. The
    // fake holds only A's native enqueue; B must never begin its Core fetch.
    @Test(arguments: DependentFetchRefusalCase.allCases)
    func nativeDependentFetchRefusalRequiresOriginalProofParkAndAccountRetirement(mode: DependentFetchRefusalCase) async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await f.engine()
            let calls = engine.acceptedNetworkCalls.value
            let a = try #require(try policyCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            let b = try #require(try policyCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            let native = CloudCallbackGate(), park = CloudCallbackGate(), revoke = CloudCallbackGate()
            let originalFailure = NativePolicyOriginalFailure()
            let bCalls = Mutex<[BridgeCall]>([])
            let transitions = Mutex<[String]>([])
            let returned = Mutex(false)
            let aOutputs = Mutex(0), bOutputs = Mutex(0)
            let bResult = Mutex<CloudPolicyNativeResult?>(nil)
            await engine.pauseNextOperationTask(native)
            if mode != .earlierFailure { await park.open() }
            try await withCloudTask(gates: [native], operation: {
                let result = await CloudPolicyOutputAcknowledgement.$input.withValue({ work in
                    let output = try await work()
                    aOutputs.withLock { $0 += 1 }
                    return output
                }) {
                    await executor.execute(a, corePrepared: {}, acknowledgeOutputs: { outputs in
                        aOutputs.withLock { $0 += outputs.count }
                    })
                }
                // A's original failure remains observable and unqualified.
                #expect(result.error != nil && result.dependentFetchRefusal == nil)
                #expect(result.acknowledgedOutputCount == result.outputs.count)
                #expect(aOutputs.withLock { $0 } == result.outputs.count)
            }, whileRunning: {
                try await native.waitForArrival()
                let bTask = Task {
                    let transition: (@Sendable (CloudQuiescentDependencyWait, CloudQuiescentDependencyWait.Phase) async throws -> Void)?
                    if mode == .noPark { transition = nil }
                    else {
                        transition = { _, phase in
                            switch phase {
                            case .park:
                                // The preparation's actual result read precedes
                                // acceptance; no Core/native fetch has started.
                                #expect(bCalls.withLock { $0.filter { $0 == .cloudPolicyExecutionResult }.count } == 1)
                                transitions.withLock { $0.append("park") }
                                if mode == .earlierFailure {
                                    do { throw originalFailure }
                                    catch { await park.enter(); throw error }
                                }
                                await park.enter()
                            case .revoke:
                                transitions.withLock { $0.append("revoke") }
                                await revoke.enter()
                                if mode == .revocationFailure { throw originalFailure }
                            }
                        }
                    }
                    let result = await MaintenanceCallObservation.$capture.withValue({ call in bCalls.withLock { $0.append(call) } }) {
                        await CloudQuiescentDependencyWait.$transition.withValue(transition) {
                            await CloudPolicyOutputAcknowledgement.$input.withValue({ work in
                                let output = try await work()
                                bOutputs.withLock { $0 += 1 }
                                return output
                            }) {
                                await executor.execute(b, corePrepared: {}, acknowledgeOutputs: { outputs in
                                    bOutputs.withLock { $0 += outputs.count }
                                })
                            }
                        }
                    }
                    bResult.withLock { $0 = result }; returned.withLock { $0 = true }
                    if mode != .noPark { _ = await park.operationSucceeded() }
                    if mode != .noPark && mode != .earlierFailure { _ = await revoke.operationSucceeded() }
                }
                do {
                    if mode == .noPark {
                        // No accepted park exists. Synchronize at B's initial
                        // read-only preparation using its actual lane entries.
                        let limit = ContinuousClock.now + .seconds(10)
                        while bCalls.withLock({ !$0.contains(.cloudPolicyExecutionValidateExecution) }) {
                            try Task.checkCancellation()
                            guard ContinuousClock.now < limit, !returned.withLock({ $0 }) else {
                                throw BridgeError.internal("Dependent initial validation was not reached")
                            }
                            await Task.yield()
                        }
                        try await policy.validateOriginalOwner()
                    } else { try await park.waitForArrival() }
                    #expect(!returned.withLock { $0 })
                    #expect(!bCalls.withLock { $0.contains(.cloudPolicyExecutionStartBeginFetch) })
                    #expect(engine.acceptedNetworkCalls.value == calls)
                    // Project this completed identity read into the stop command.
                    let read = try await f.core.beginIdentityRead(transition: true)
                    let other = mode == .otherAccount
                    try #require(try await f.core.completeIdentityRead(id: read, status: other ? .available : .noAccount,
                        identity: other ? Data("synthetic-account-B".utf8) : nil))
                    let projected = try await policy.projectIdentityRead(read)
                    let stop = try #require(try policyCommands(projected).first { if case .stopEngine = $0.action { true } else { false } })
                    try #require(stop.accountRetirement != nil)
                    if mode != .earlierFailure { await park.open() }
                    if mode == .noRetirement {
                        // Guard revocation alone supplies no retired-instance fact.
                        await native.open()
                    } else {
                        if mode == .retirementFailure {
                            await f.adapter.failNextRetirement(originalFailure)
                            let failedStop = await executor.execute(stop)
                            #expect((failedStop.error as? NativePolicyOriginalFailure) === originalFailure)
                            #expect((await f.adapter.lastFailure as? NativePolicyOriginalFailure) === originalFailure)
                            #expect(await f.adapter.retainedInstanceIDs() == [engine.instanceID])
                            #expect(!(await engine.recordedTrace().contains(.released)))
                        }
                        let retirementCalls = Mutex<[BridgeCall]>([])
                        let stopped = await MaintenanceCallObservation.$capture.withValue({ call in retirementCalls.withLock { $0.append(call) } }) {
                            await executor.execute(stop)
                        }
                        #expect(stopped.error == nil)
                        #expect(await engine.recordedTrace().contains(.released))
                        if mode == .retirementFailure {
                            #expect(await f.adapter.retainedInstanceIDs().isEmpty)
                            #expect(retirementCalls.withLock { $0.contains(.cloudPolicyExecutionStartRetireOriginal) && $0.contains(.cloudPolicyRetirementResult) })
                            #expect((await f.adapter.lastFailure as? NativePolicyOriginalFailure) === originalFailure)
                        }
                    }
                    if mode == .earlierFailure { await park.open() }
                    else if mode != .noPark {
                        try await revoke.waitForArrival()
                        #expect(!returned.withLock { $0 })
                        #expect(!bCalls.withLock { $0.contains(.cloudPolicyExecutionStartBeginFetch) })
                        await revoke.open()
                    }
                    await bTask.value
                } catch {
                    await native.open(); await park.open(); await revoke.open()
                    bTask.cancel(); await bTask.value
                    throw error
                }
            })
            let result = try #require(bResult.withLock { $0 })
            if mode == .earlierFailure || mode == .revocationFailure {
                #expect((result.error as? NativePolicyOriginalFailure) === originalFailure)
            } else { #expect(result.error as? BridgeError == .cloudPolicy(.staleCommand)) }
            #expect((result.dependentFetchRefusal != nil) == mode.qualifies)
            #expect(result.originalGuardRefusal == nil && !result.deferred)
            #expect(!result.outputs.isEmpty && result.acknowledgedOutputCount == result.outputs.count)
            #expect(bOutputs.withLock { $0 } == result.outputs.count)
            #expect(!bCalls.withLock { $0.contains(.cloudPolicyExecutionStartBeginFetch) || $0.contains(.cloudPolicyExecutionStartReportStarted) })
            #expect(engine.acceptedNetworkCalls.value == calls)
            if mode == .retirementFailure {
                #expect(result.dependentFetchRefusal == nil)
                #expect((await f.adapter.lastFailure as? NativePolicyOriginalFailure) === originalFailure)
                #expect(transitions.withLock { $0 } == ["park", "revoke"])
                #expect(bCalls.withLock { $0.filter { $0 == .cloudPolicyStartPrepareExecution }.count } == 2)
            }
            if mode.qualifies {
                // Both original lifecycle transition ACKs were joined, and the
                // unchanged post-park preparation supplied the actual refusal.
                #expect(transitions.withLock { $0 } == ["park", "revoke"])
                #expect(bCalls.withLock { $0.filter { $0 == .cloudPolicyStartPrepareExecution }.count } == 2)
            }
        }
    }

    @Test func nativeDependentFetchCurrentInvalidSDKKeepsItsOriginalError() async throws {
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await f.engine()
            let calls = engine.acceptedNetworkCalls.value
            let command = try #require(try policyCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            let gate = CloudCallbackGate()
            await f.adapter.setPolicyOperationOriginObserver { await gate.enter() }
            try await withCloudTask(gates: [gate], operation: {
                let result = await executor.execute(command)
                #expect(result.error as? BridgeError == .cloudPolicy(.staleCommand))
                #expect(result.dependentFetchRefusal == nil && result.originalGuardRefusal == nil)
            }, whileRunning: {
                try await gate.waitForArrival()
                try await f.core.retireSDK(engine.instanceID, policyStop: false)
                await gate.open()
            })
            await f.adapter.setPolicyOperationOriginObserver(nil)
            #expect(engine.acceptedNetworkCalls.value == calls)
        }
    }

    @Test(arguments: ["stale", "invalidSDK", "wrongOwner", "wrongAction", "malformed", "generation"])
    func nativeDependentFetchInitialFailuresCannotAcquireQualification(mode: String) async throws {
        var foreign: CloudPolicyCommand?
        if mode == "wrongOwner" {
            try await withNativePolicyFixture { f in
                let (policy, executor) = try await nativePolicy(f)
                try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
                foreign = try #require(try policyCommands(try await policy.input(.foreground)).first { if case .requestFetch = $0.action { true } else { false } })
            }
        }
        try await withNativePolicyFixture { f in
            let (policy, executor) = try await nativePolicy(f)
            try await consumePolicySetup(policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: false))), executor: executor)
            let engine = try await f.engine()
            let calls = engine.acceptedNetworkCalls.value
            let commands = try policyCommands(try await policy.input(.foreground))
            let command = try #require(foreign ?? commands.first { if case .requestFetch = $0.action { true } else { false } })
            if mode == "stale" { _ = try await policy.input(.settings(.init(enabled: true, paused: true, automatic: true, media: false))) }
            if mode == "invalidSDK" { try await f.core.retireSDK(engine.instanceID, policyStop: false) }
            if mode == "generation" { try await CloudFenceTransition.generation.apply(to: f) }
            let parks = Mutex(0)
            try await CloudQuiescentDependencyWait.$transition.withValue({ _, _ in parks.withLock { $0 += 1 } }) {
                if mode == "wrongAction" || mode == "malformed" {
                    let wrong: CloudPolicyCommand
                    if mode == "malformed" { wrong = try await pendingCommand(policy) }
                    else {
                        let exchange = try await policy.input(.syncNowRequested(.command))
                        wrong = try #require(try policyCommands(exchange).first { if case .runExchange = $0.action { true } else { false } })
                    }
                    do {
                        try await f.adapter.executePolicyOperation(wrong, policy: policy)
                        Issue.record("Wrong action entered a policy fetch")
                    } catch {
                        #expect(error as? BridgeError == .cloudPolicy(mode == "malformed" ? .coreCapabilityUnavailable : .notNativeAction))
                        #expect(!(error is CloudDependentFetchRefusalError))
                    }
                } else {
                    let result = await executor.execute(command)
                    #expect(result.dependentFetchRefusal == nil && result.originalGuardRefusal == nil)
                    switch mode {
                    case "invalidSDK": #expect(result.error == nil) // Original NoOperation, not a qualified error.
                    case "wrongOwner": #expect(result.error as? BridgeError == .cloudPolicy(.wrongOwner))
                    case "generation": #expect(result.error as? BridgeError == .cloudPolicy(.closed))
                    default: #expect(result.error as? BridgeError == .cloudPolicy(.staleCommand))
                    }
                }
            }
            #expect(parks.withLock { $0 } == 0 && engine.acceptedNetworkCalls.value == calls)
        }
    }
}
#endif
