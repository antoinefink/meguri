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
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil),
            ])
            let output = try await policy.input(.syncNowRequested(.command))
            let command = try #require(try policyCommands(output).first { if case .runExchange = $0.action { true } else { false } })
            let before = try await observePolicyExchange(command)
            let result = await executor.execute(command)
            #expect(result.error == nil)
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
