import Foundation
import Synchronization
import Testing
@testable import CoreBridge

extension EventTests {
    /// P24/Q11 closure dependency: codec-authored conflict + the exact production
    /// delegate/transport seam. Signed CKSyncEngine constructors/event timing,
    /// cross-device signing, remaining codecs/generations/rekey stay open.
    @Test func wiredCloudAssetFirstConflictWaitsForTrueBookkeepingAndWholeEventChainThenMergesAndResends() async throws {
        try await withWiredAssetReadFixture { fixture, _, _, transport, completions in
            let engine = try await fixture.engine()
            let remote = try await AssetExecutorRemote.make(0)
            let notes = CoreNoteService()
            let before = try await notes.readNote(id: remote.noteID)
            var local = before.fields
            local[0] = String(repeating: "local", count: 160_000)
            let edit = try await notes.updateNote(.init(id: remote.noteID, fields: local, tags: before.tags))
            try await fixture.core.awaitTracking(generation: edit.summary.generation, seq: edit.summary.seq)
            let read = AssetExecutorOperation(outcome: .success(.assetRead(try remote.ownedResult(in: fixture.directory))))
            transport.install(read, for: remote.record.name)
            await engine.appendSendReplies([.conflict(remote.record)])
            let persisted = CloudCallbackGate(), afterSent = CloudCallbackGate()
            let chained = CloudCallbackGate(), barrier = CloudCallbackGate()
            await fixture.probe.pauseSent(persisted)
            await engine.pauseAfterSentHandler(afterSent)
            let initial = try await remote.observe()
            try await withCloudTask(gates: [persisted, afterSent, chained, barrier, read.terminal], operation: {
                try await fixture.adapter.requestSend(zones: [fixture.zone])
                try await fixture.adapter.waitForIdle()
            }, whileRunning: {
                try await persisted.waitForArrival()
                #expect(transport.preparedNames.withLock { $0.isEmpty })
                await engine.pauseNextCallback(chained)
                await engine.pauseNativeReturnedWork(barrier)
                let nextEvent = Task { await engine.emitRetained(.stateUpdate(Data([9, 4])), label: "chainedState") }
                await engine.waitForQueuedEvent()
                await persisted.open()
                try await chained.waitForArrival()
                try await afterSent.waitForArrival()
                #expect(transport.preparedNames.withLock { $0.isEmpty })
                await afterSent.open()
                // The next event owns the serial slot directly. A notification
                // for the sent event cannot cross that original engine barrier.
                #expect(read.enqueueCalls.withLock { $0 } == 0)
                await chained.open()
                await nextEvent.value
                try await barrier.waitForArrival()
                #expect(transport.preparedNames.withLock { $0.isEmpty })
                await barrier.open()
                try await read.terminal.waitForArrival()
                #expect(try await remote.observe().retryCount == 0)
                #expect(read.enqueueCalls.withLock { $0 } == 1)
                await read.terminal.open()
            })
            await completions.wait(for: 1)
            let merged = try await notes.readNote(id: remote.noteID)
            let remoteFields = try #require(String(data: remote.fields, encoding: .utf8)).components(separatedBy: "\u{1f}")
            #expect(merged.fields == [local[0], remoteFields[1]])
            let applied = try await remote.observe()
            #expect(applied.retryCount == nil && applied.changedByIncoming && applied.walReceipts > initial.walReceipts)
            #expect(try await fixture.core.trackingSendables()[remote.record.name] == .save)
            #expect(await engine.pendingRecordZoneChanges()[remote.record.name] == .save)
            try await fixture.adapter.requestSend(zones: [fixture.zone])
            try await fixture.adapter.waitForIdle()
            #expect(try await fixture.core.entityFacts(name: remote.record.name)?.mark == nil)
            #expect(transport.preparedNames.withLock { $0 } == [remote.record.name])
            #expect(!(await engine.recordedTrace()).contains(.reentrantNetworkCall))
        }
    }

    @Test func wiredCloudAssetSyncNowWaitsForOriginalFirstReadAndResendsMergedSelectionBeforeCompleting() async throws {
        try await withWiredAssetReadFixture { fixture, policy, executor, transport, _ in
            let engine = try await fixture.engine()
            let remote = try await AssetExecutorRemote.make(0)
            let notes = CoreNoteService()
            let before = try await notes.readNote(id: remote.noteID)
            var editedFields = before.fields
            editedFields[0] = String(repeating: "local", count: 160_000)
            let local = editedFields
            let edit = try await notes.updateNote(.init(id: remote.noteID, fields: local, tags: before.tags))
            try await fixture.core.awaitTracking(generation: edit.summary.generation, seq: edit.summary.seq)
            let read = AssetExecutorOperation(outcome: .success(.assetRead(try remote.ownedResult(in: fixture.directory))))
            transport.install(read, for: remote.record.name)
            await engine.appendSendReplies([.conflict(remote.record), .saved])
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil),
            ])
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected Sync Now commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            let finished = Mutex(false)
            try await withCloudTask(gates: [read.terminal], operation: {
                let result = await executor.execute(command)
                finished.withLock { $0 = true }
                if let error = result.error { throw error }
                #expect(!result.deferred)
                let runtimeID = try await wiredAssetExchangeRuntimeID(command)
                let stored = try #require(try await fixture.core.exchangeResult(id: runtimeID))
                #expect(stored.collection == .succeeded)
                #expect(stored.selectedNames.contains(remote.record.name))
                #expect(stored.ackedNames.contains(remote.record.name) && stored.outstandingNames.isEmpty)
                let records = result.outputs.flatMap { output -> [CloudPolicyExchangeRecord] in
                    guard case .commands(let commands) = output else { return [] }
                    return commands.compactMap { if case .recordExchange(let record) = $0.action { record } else { nil } }
                }
                #expect(records.count == 1)
                let record = try #require(records.first)
                if case .succeeded = record.outcome {} else { Issue.record("Sync Now did not record success") }
            }, whileRunning: {
                try await read.terminal.waitForArrival()
                #expect(!finished.withLock { $0 })
                let runtimeID = try await wiredAssetExchangeRuntimeID(command)
                #expect(try await fixture.core.exchangeResult(id: runtimeID) == nil)
                #expect(try await notes.readNote(id: remote.noteID).fields == local)
                #expect(try await remote.observe().retryCount == 0)
                await read.terminal.open()
            })
            let remoteFields = try #require(String(data: remote.fields, encoding: .utf8)).components(separatedBy: "\u{1f}")
            #expect(try await notes.readNote(id: remote.noteID).fields == [local[0], remoteFields[1]])
            let facts = try #require(try await fixture.core.entityFacts(name: remote.record.name))
            #expect(facts.mark == nil && facts.ackedPayloadDigest == facts.payloadDigest)
            #expect(transport.preparedNames.withLock { $0 } == [remote.record.name])
            let trace = await engine.recordedTrace()
            let hands = trace.filter { if case .handed(_, let names) = $0 { names.contains(remote.record.name) } else { false } }
            #expect(hands.count == 2)
            #expect(trace.contains { if case .savedRemoved(let names) = $0 { names.contains(remote.record.name) } else { false } })
            #expect(!trace.contains(.reentrantNetworkCall))
        }
    }

    @Test func wiredCloudAssetSyncNowCancellationCancelsOriginalPreparedReadAndDrainsItsTerminalBeforeCompleting() async throws {
        try await withWiredAssetReadFixture { fixture, policy, executor, transport, completions in
            let engine = try await fixture.engine()
            let remote = try await AssetExecutorRemote.make(0)
            let notes = CoreNoteService()
            let before = try await notes.readNote(id: remote.noteID)
            var editedFields = before.fields
            editedFields[0] = String(repeating: "local", count: 160_000)
            let local = editedFields
            let edit = try await notes.updateNote(.init(id: remote.noteID, fields: local, tags: before.tags))
            try await fixture.core.awaitTracking(generation: edit.summary.generation, seq: edit.summary.seq)
            let read = AssetExecutorOperation(outcome: .success(.assetRead(try remote.ownedResult(in: fixture.directory))))
            transport.install(read, for: remote.record.name)
            await engine.appendSendReplies([.conflict(remote.record), .saved])
            await engine.appendFetchScripts([
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [fixture.root], group: nil),
                .manual(fetchedZones: ["meguri-root", fixture.zone], records: [], group: nil),
            ])
            guard case .commands(let commands) = try await policy.input(.syncNowRequested(.command)) else {
                throw BridgeError.internal("Expected Sync Now commands")
            }
            let command = try #require(commands.first { if case .runExchange = $0.action { true } else { false } })
            guard case .runExchange(let exchange, let source) = command.action else {
                throw BridgeError.internal("Expected original exchange")
            }
            let finished = Mutex(false)
            try await withCloudTask(gates: [read.terminal], operation: {
                let result = await executor.execute(command)
                finished.withLock { $0 = true }
                #expect(result.error is CancellationError && !result.deferred)
                let runtimeID = try await wiredAssetExchangeRuntimeID(command)
                let stored = try #require(try await fixture.core.exchangeResult(id: runtimeID))
                #expect(stored.id == runtimeID && stored.collection == .cancelled)
                #expect(stored.selectedNames.contains(remote.record.name))
                #expect(!stored.ackedNames.contains(remote.record.name))
                let records = result.outputs.flatMap { output -> [CloudPolicyExchangeRecord] in
                    guard case .commands(let commands) = output else { return [] }
                    return commands.compactMap { if case .recordExchange(let record) = $0.action { record } else { nil } }
                }
                #expect(records.count == 1)
                let record = try #require(records.first)
                #expect(record.exchange == exchange && record.source == source)
                if case .cancelled = record.outcome {} else { Issue.record("Sync Now did not record cancellation") }
                #expect(try await fixture.core.exchangeResult(id: runtimeID) == stored)
            }, whileRunning: {
                try await read.terminal.waitForArrival()
                try await fixture.adapter.waitForFirstAssetReadJoin(sdk: engine.instanceID)
                guard case .commands(let commands) = try await policy.input(.syncNowCancelled(exchange: exchange)) else {
                    throw BridgeError.internal("Expected exchange cancellation commands")
                }
                let cancel = try #require(commands.first { if case .cancelExchange = $0.action { true } else { false } })
                #expect((await executor.execute(cancel)).error == nil)
                try await read.waitUntilCancelled()
                #expect(read.cancelled.withLock { $0 })
                #expect(read.enqueueCalls.withLock { $0 } == 1 && read.replyCalls.withLock { $0 } == 1)
                #expect(!finished.withLock { $0 })
                let runtimeID = try await wiredAssetExchangeRuntimeID(command)
                #expect(try await fixture.core.exchangeResult(id: runtimeID) == nil)
                #expect(try await notes.readNote(id: remote.noteID).fields == local)
                #expect(try await remote.observe().retryCount == 0)
                #expect(await fixture.adapter.retainedInstanceIDs().contains(engine.instanceID))
                await read.terminal.open()
            })
            await completions.wait(for: 1)
            try await fixture.adapter.waitForIdle()
            #expect(!(try await fixture.core.status()).syncingCollection)
            #expect(try await remote.observe().retryCount == nil)
            #expect(transport.preparedNames.withLock { $0 } == [remote.record.name])
            #expect(read.enqueueCalls.withLock { $0 } == 1 && read.replyCalls.withLock { $0 } == 1)
            let trace = await engine.recordedTrace()
            let hands = trace.filter { if case .handed(_, let names) = $0 { names.contains(remote.record.name) } else { false } }
            #expect(hands.count == 1)
            #expect(trace.contains(.cancelReturned))
            #expect(!trace.contains(.reentrantNetworkCall))
        }
    }

    @Test func wiredCloudAssetRetainsEveryIssuedRetryWhileOriginalFinitePassRunsAndAdmitsLateEntriesOnlyLater() async throws {
        try await withWiredAssetReadFixture { fixture, policy, executor, transport, _ in
            let first = try await AssetExecutorRemote.make(1), second = try await AssetExecutorRemote.make(2)
            let firstRead = AssetExecutorOperation(outcome: .success(.assetRead(try first.ownedResult(in: fixture.directory))))
            let secondRead = AssetExecutorOperation(outcome: .success(.assetRead(try second.ownedResult(in: fixture.directory))))
            transport.install(firstRead, for: first.record.name); transport.install(secondRead, for: second.record.name)
            let firstCommand = try await wiredAssetRetryCommand(fixture, policy: policy, records: [first])
            let sdk = try await fixture.engine().instanceID
            try await withCloudTask(gates: [firstRead.terminal], operation: {
                let result = await executor.execute(firstCommand)
                if let error = result.error { throw error }
                #expect(result.error == nil && !result.deferred)
            }, whileRunning: {
                try await firstRead.terminal.waitForArrival()
                let secondCommand = try await wiredAssetRetryCommand(fixture, policy: policy, records: [second])
                let thirdCommand = try await wiredAssetRetryCommand(fixture, policy: policy)
                try await withCloudTask(gates: [firstRead.terminal, secondRead.terminal], expectedArrivals: [secondRead.terminal], operation: {
                    async let secondResult = executor.execute(secondCommand)
                    async let thirdResult = executor.execute(thirdCommand)
                    let results = await (secondResult, thirdResult)
                    if let error = results.0.error { throw error }
                    if let error = results.1.error { throw error }
                    #expect(results.0.error == nil && !results.0.deferred)
                    #expect(results.1.error == nil && !results.1.deferred)
                }, whileRunning: {
                    try await fixture.adapter.waitForQueuedAssetReads(sdk: sdk, count: 2)
                    #expect(secondRead.enqueueCalls.withLock { $0 } == 0)
                    await firstRead.terminal.open()
                    try await secondRead.terminal.waitForArrival()
                    #expect(try await first.observe().rowFields == first.fields)
                    #expect(try await second.observe().retryCount == 1)
                    await secondRead.terminal.open()
                })
            })
            try await fixture.adapter.waitForIdle()
            #expect(transport.preparedNames.withLock { $0 } == [first.record.name, second.record.name])
            #expect(try await second.observe().rowFields == second.fields)
        }
    }

    enum WiredAssetTransition: Sendable, Equatable, CaseIterable { case stop, configure, suspend, account, hostClose }

    @Test(arguments: WiredAssetTransition.allCases)
    func wiredCloudAssetLifecycleJoinsAcceptedOriginalAndCancelsQueuedCommandBeforeRetirement(transition: WiredAssetTransition) async throws {
        try await withWiredAssetReadFixture { fixture, policy, executor, transport, _ in
            let remote = try await AssetExecutorRemote.make(1)
            let read = AssetExecutorOperation(outcome: .failure(CancellationError()))
            transport.install(read, for: remote.record.name)
            let command = try await wiredAssetRetryCommand(fixture, policy: policy, records: [remote])
            let sdk = try await fixture.engine().instanceID
            try await withCloudTask(gates: [read.terminal], operation: {
                let result = await executor.execute(command)
                #expect(result.error != nil)
            }, whileRunning: {
                try await read.terminal.waitForArrival()
                let queued = try await wiredAssetRetryCommand(fixture, policy: policy)
                let pending = Task { await executor.execute(queued) }
                try await fixture.adapter.waitForQueuedAssetReads(sdk: sdk, count: 1)
                let finished = Mutex(false)
                let draining = Task {
                    switch transition {
                    case .stop: try await fixture.adapter.stop()
                    case .configure: try await fixture.adapter.configureAutomatic(false)
                    case .suspend: try await fixture.adapter.freezeForSuspend()
                    case .account: try await fixture.adapter.accountChanged(status: .noAccount, identity: nil)
                    case .hostClose:
                        try await fixture.collection.close()
                        // Core may reject old terminal activity after Host close;
                        // native ownership/release still has to join the worker.
                        try? await fixture.adapter.stop()
                    }
                    finished.withLock { $0 = true }
                }
                try await read.waitUntilCancelled()
                #expect(!finished.withLock { $0 })
                #expect(await fixture.adapter.retainedInstanceIDs().contains(sdk))
                if transition == .stop || transition == .configure || transition == .suspend {
                    try await fixture.core.validateSDK(sdk)
                }
                #expect(read.replyCalls.withLock { $0 } == 1)
                await read.terminal.open()
                try await draining.value
                #expect((await pending.value).error != nil)
                switch transition {
                case .configure:
                    let replacement = try await fixture.engine()
                    #expect(replacement.instanceID != sdk)
                    try await fixture.core.validateSDK(replacement.instanceID)
                    #expect(await replacement.pendingRecordZoneChanges() == (try await fixture.core.trackingSendables()))
                case .suspend:
                    #expect(await fixture.adapter.retainedInstanceIDs() == [sdk])
                    #expect(await fixture.adapter.resumeDeliveries())
                    try await fixture.core.validateSDK(sdk)
                default: #expect(!(await fixture.adapter.retainedInstanceIDs()).contains(sdk))
                }
            })
            #expect(transport.preparedNames.withLock { $0 } == [remote.record.name])
        }
    }

    @Test(arguments: [WiredAssetTransition.stop, .suspend, .account, .hostClose])
    func wiredCloudAssetLifecycleJoinsBlockedOriginalBarrierWithoutStartingQueuedRead(transition: WiredAssetTransition) async throws {
        try await withWiredAssetReadFixture { fixture, policy, executor, transport, _ in
            let remote = try await AssetExecutorRemote.make(1)
            let command = try await wiredAssetRetryCommand(fixture, policy: policy, records: [remote])
            let engine = try await fixture.engine(), barrier = CloudCallbackGate()
            await engine.pauseNativeReturnedWork(barrier)
            try await withCloudTask(gates: [barrier], operation: {
                #expect((await executor.execute(command)).error != nil)
            }, whileRunning: {
                try await barrier.waitForArrival()
                let finished = Mutex(false)
                #expect(try await remote.observe().retryCount == 0)
                let stop = Task {
                    switch transition {
                    case .stop: try await fixture.adapter.stop()
                    case .suspend: try await fixture.adapter.freezeForSuspend()
                    case .account: try await fixture.adapter.accountChanged(status: .noAccount, identity: nil)
                    case .hostClose:
                        try await fixture.collection.close()
                        try? await fixture.adapter.stop()
                    case .configure: throw BridgeError.internal("Configure uses accepted-read coverage")
                    }
                    finished.withLock { $0 = true }
                }
                try await fixture.adapter.waitForAssetReadAdmissionClosed(sdk: engine.instanceID)
                #expect(!finished.withLock { $0 })
                #expect(transport.preparedNames.withLock { $0.isEmpty })
                if transition == .stop || transition == .suspend { try await fixture.core.validateSDK(engine.instanceID) }
                await barrier.open()
                try await stop.value
                if transition != .hostClose { #expect(try await remote.observe().retryCount == 0) }
                if transition == .suspend { #expect(await fixture.adapter.resumeDeliveries()) }
            })
        }
    }
}

extension EventTests {
    @Test(arguments: [false, true])
    func wiredCloudAssetSoftFactsSurviveLaterStrictThrowOrAppCancelAndEmptyFirstPassClearsNothing(cancel: Bool) async throws {
        try await withWiredAssetReadFixture { fixture, policy, executor, transport, _ in
            let first = try await AssetExecutorRemote.make(1), second = try await AssetExecutorRemote.make(2)
            let soft = AssetExecutorOperation(outcome: .success(.assetRead(.nativeFailure(code: 3, retryAfterMilliseconds: 4_000))))
            let terminal: any Error = cancel ? CancellationError() : CloudKitSDKOperationFailure(
                underlying: CloudOwnTransportFailure.missingReply, code: 6, retryAfterMilliseconds: 120_000)
            let strict = AssetExecutorOperation(outcome: .failure(terminal))
            transport.install(soft, for: first.record.name); transport.install(strict, for: second.record.name)
            let command = try await wiredAssetRetryCommand(fixture, policy: policy, records: [first, second])
            let running = Task { await executor.execute(command) }
            try await withCloudTask(gates: [soft.terminal, strict.terminal], operation: {
                #expect((await running.value).error != nil)
            }, whileRunning: {
                try await soft.terminal.waitForArrival()
                await soft.terminal.open()
                try await strict.terminal.waitForArrival()
                #expect(try await first.observe().retryCount == 1)
                if cancel { running.cancel(); try await strict.waitUntilCancelled() }
                await strict.terminal.open()
            })
            let status = try await policy.status()
            #expect(status.error?.code == (cancel ? 3 : 6))
            #expect(status.error?.retryAfterMilliseconds == (cancel ? 4_000 : 120_000))
            #expect(try await second.observe().ownedCount == 0)
            // With no conflict entries, sending must leave the read error intact.
            try await fixture.adapter.requestSend(zones: [fixture.zone])
            try await fixture.adapter.waitForIdle()
            let afterEmpty = try await policy.status()
            #expect(afterEmpty.error?.code == status.error?.code)
            #expect(transport.preparedNames.withLock { $0 } == [first.record.name, second.record.name])
        }
    }
}
