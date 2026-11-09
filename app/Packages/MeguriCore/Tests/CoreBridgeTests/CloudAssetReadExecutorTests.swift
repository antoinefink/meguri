import Foundation
import Instrumentation
import MeguriCoreBindings
import Synchronization
import Testing
@testable import CoreBridge

extension EventTests {
    @Test func assetExecutorDrainsActualWorkSeriallyThroughWriterAndWAL() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let first = try await AssetExecutorRemote.make(1)
            let second = try await AssetExecutorRemote.make(2)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [first, second])
            let firstRead = AssetExecutorOperation(outcome: .success(.assetRead(try first.ownedResult(in: fixture.directory))))
            let secondRead = AssetExecutorOperation(outcome: .success(.assetRead(try second.ownedResult(in: fixture.directory))))
            let transport = AssetExecutorTransport([first.record.name: firstRead, second.record.name: secondRead])
            let executor = CloudAssetReadExecutor(transport: transport)
            let before = try await first.observe()
            #expect(before.rowFields == nil && before.retryCount == 0)
            try await withCloudTask(gates: [firstRead.terminal, secondRead.terminal], operation: {
                let result = try await executor.run(pass)
                #expect(result.outcome == .succeeded && result.readCount == 2 && result.failures.isEmpty)
            }, whileRunning: {
                try await firstRead.terminal.waitForArrival()
                #expect(transport.preparedNames.withLock { $0 } == [first.record.name])
                #expect(firstRead.enqueueCalls.withLock { $0 } == 1)
                #expect(secondRead.enqueueCalls.withLock { $0 } == 0)
                #expect(try await first.observe().retryCount == 1)
                #expect(try await second.observe().retryCount == 0)
                // Only an actual pass/work exists; another selection is denied
                // until this exact original terminal completes.
                await #expect(throws: (any Error).self) { _ = try await pass.next() }
                let owned = fixture.directory.appending(path: "owned-\(first.noteID)")
                #expect(try FileManager.default.contentsOfDirectory(atPath: owned.path).count == 1)
                await firstRead.terminal.open()
                try await secondRead.terminal.waitForArrival()
                let applied = try await first.observe()
                #expect(applied.rowFields == first.fields && applied.retryCount == nil)
                #expect(applied.changedByIncoming && applied.walReceipts > before.walReceipts)
                #expect(transport.preparedNames.withLock { $0 } == [first.record.name, second.record.name])
                await secondRead.terminal.open()
            })
            for remote in [first, second] {
                let applied = try await remote.observe()
                #expect(applied.rowFields == remote.fields && applied.retryCount == nil)
                #expect(applied.changedByIncoming && applied.walReceipts > before.walReceipts)
                // The SDK source was deleted before run; only the owned result
                // carries its copy through reply and awaited core completion.
                #expect(!FileManager.default.fileExists(atPath: fixture.directory.appending(path: "sdk-\(remote.noteID)").path))
                #expect(try FileManager.default.contentsOfDirectory(atPath: fixture.directory.appending(path: "owned-\(remote.noteID)").path).isEmpty)
            }
            await #expect(throws: (any Error).self) { _ = try await pass.next() }
            await executor.close()
        }
    }

    @Test(arguments: [false, true])
    func assetExecutorFailedReadStillDrainsNextActualEntryAndReportsRoundFailures(secondFails: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let first = try await AssetExecutorRemote.make(1)
            let second = try await AssetExecutorRemote.make(2)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [first, second])
            let failed = CloudAssetReadResult.nativeFailure(code: 6, retryAfterMilliseconds: 120_000)
            let firstRead = AssetExecutorOperation(outcome: .success(.assetRead(failed)))
            let secondRead: AssetExecutorOperation
            if secondFails { secondRead = AssetExecutorOperation(outcome: .success(.assetRead(.failed))) }
            else { secondRead = AssetExecutorOperation(outcome: .success(.assetRead(try second.ownedResult(in: fixture.directory)))) }
            let transport = AssetExecutorTransport([first.record.name: firstRead, second.record.name: secondRead])
            let executor = CloudAssetReadExecutor(transport: transport)
            let before = try await second.observe()
            var expectedFailures = [try #require(failed.failureFacts)]
            if secondFails { expectedFailures.append(try #require(CloudAssetReadResult.failed.failureFacts)) }
            let failures = expectedFailures
            try await withCloudTask(gates: [firstRead.terminal, secondRead.terminal], operation: {
                let result = try await executor.run(pass)
                #expect(result.outcome == .failed && result.readCount == 2)
                #expect(result.failures == failures)
            }, whileRunning: {
                try await firstRead.terminal.waitForArrival()
                #expect(transport.preparedNames.withLock { $0 } == [first.record.name])
                #expect(secondRead.enqueueCalls.withLock { $0 } == 0)
                await firstRead.terminal.open()
                try await secondRead.terminal.waitForArrival()
                let failedEntry = try await first.observe()
                #expect(failedEntry.rowFields == nil && failedEntry.retryCount == 1 && !failedEntry.changedByIncoming)
                #expect(transport.preparedNames.withLock { $0 } == [first.record.name, second.record.name])
                #expect(firstRead.replyCalls.withLock { $0 } == 1)
                #expect(secondRead.enqueueCalls.withLock { $0 } == 1)
                await secondRead.terminal.open()
            })
            let failedEntry = try await first.observe()
            #expect(failedEntry.rowFields == nil && failedEntry.retryCount == 1 && !failedEntry.changedByIncoming)
            let next = try await second.observe()
            if secondFails {
                #expect(next.rowFields == nil && next.retryCount == 1 && !next.changedByIncoming)
            } else {
                #expect(next.rowFields == second.fields && next.retryCount == nil)
                #expect(next.changedByIncoming && next.walReceipts > before.walReceipts)
                #expect(try FileManager.default.contentsOfDirectory(atPath: fixture.directory.appending(path: "owned-\(second.noteID)").path).isEmpty)
            }
            await #expect(throws: (any Error).self) { _ = try await pass.next() }
            await executor.close()
        }
    }

    @Test func assetExecutorCancellationAfterNextAdmissionDisposesOriginalIssuedWork() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let execution = try await assetExecutorRetryExecution(fixture, policy: policy, records: [remote])
            let rawExecution = execution.raw
            let raw = try await MaintenanceLane.shared.call(.cloudPolicyExecutionBeginAssetReadPass) {
                try rawExecution.beginAssetReadPass()
            }
            let running = Mutex<Task<CloudAssetReadPassResult, any Error>?>(nil)
            let admitted = Mutex<MeguriCoreBindings.JobHandle?>(nil)
            let pass = CoreCloudAssetReadPass(raw, track: { handle in
                let job = try Jobs.track(handle)
                admitted.withLock { $0 = handle }
                // Cancels the awaiting Swift executor after real next admission.
                // Core remains uncancelled and must return its actual selection.
                running.withLock { $0 }?.cancel()
                return job
            })
            let transport = AssetExecutorTransport(failPreparation: true)
            let executor = CloudAssetReadExecutor(transport: transport)
            let entry = CloudCallbackGate()
            let task = Task {
                await entry.enter()
                return try await executor.run(pass)
            }
            running.withLock { $0 = task }
            try await entry.waitForArrival()
            await entry.open()
            let result = await task.result
            if case .failure(let error) = result { #expect(error is CancellationError) }
            else { Issue.record("Cancelled executor returned success") }
            let handle = try #require(admitted.withLock { $0 })
            #expect(try await CoreBridge.CancelToken(raw: handle.cancel).isCancelled() == false)
            for await event in CoreEventStream.shared.events {
                if case .jobFinished(let id, let outcome) = event.kind, id.rawValue == handle.id {
                    #expect(outcome == .succeeded)
                    break
                }
            }
            #expect(transport.preparedNames.withLock { $0 }.isEmpty)
            let unchanged = try await remote.observe()
            #expect(unchanged.retryCount == 0 && unchanged.ownedCount == 0 && unchanged.rowFields == nil)
            // Executor finished the original pass after cancelling its actual
            // Selected handle. A fresh retry still owns the unchanged entry.
            let later = try await assetExecutorRetryPass(fixture, policy: policy, records: [])
            let selected = try #require(try await later.next())
            #expect(selected.name == remote.record.name)
            #expect(try await later.cancel(selected.handle))
            try await later.finish()
            await executor.close()
        }
    }

    @Test func assetExecutorFalseQueueOwesNoReplyAndChargesNoAttempt() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [remote])
            let operation = AssetExecutorOperation(accept: false, outcome: .success(.assetRead(.failed)))
            let executor = CloudAssetReadExecutor(transport: AssetExecutorTransport([remote.record.name: operation]))
            let result = await Task { try await executor.run(pass) }.result
            expectAssetExecutorFailure(result, .queueRejected)
            #expect(operation.enqueueCalls.withLock { $0 } == 1)
            #expect(operation.replyCalls.withLock { $0 } == 0)
            #expect(operation.cancelled.withLock { $0 })
            let observed = try await remote.observe()
            #expect(observed.retryCount == 0 && observed.rowFields == nil)
            // A fresh, explicitly issued retry can select the same real entry;
            // the executor did not restart its refused pass.
            let later = try await assetExecutorRetryPass(fixture, policy: policy, records: [])
            let work = try #require(try await later.next())
            #expect(work.name == remote.record.name && work.kind == .retry)
            #expect(try await later.cancel(work.handle))
            try await later.finish()
            await executor.close()
        }
    }

    @Test(arguments: [false, true])
    func assetExecutorPrepareFailureOrPreEntryCancellationCancelsUnstartedWork(cancel: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [remote])
            let transport = AssetExecutorTransport(failPreparation: true)
            let executor = CloudAssetReadExecutor(transport: transport)
            let entry = CloudCallbackGate()
            let task = Task {
                await entry.enter()
                try await executor.run(pass)
            }
            try await entry.waitForArrival()
            if cancel { task.cancel() }
            await entry.open()
            let result = await task.result
            if cancel {
                guard case .failure(let error) = result else { Issue.record("Cancelled run succeeded"); return }
                #expect(error is CancellationError)
                #expect(transport.preparedNames.withLock { $0.isEmpty })
            } else { expectAssetExecutorFailure(result, .invalidCursor) }
            #expect(try await remote.observe().retryCount == 0)
            let later = try await assetExecutorRetryPass(fixture, policy: policy, records: [])
            let selected = try #require(try await later.next())
            #expect(selected.name == remote.record.name)
            #expect(try await later.cancel(selected.handle))
            try await later.finish()
            await executor.close()
        }
    }

    @Test(arguments: [false, true])
    func assetExecutorCancellationAndConcurrentCloseJoinOriginalTerminal(close: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [remote])
            let operation = AssetExecutorOperation(outcome: .failure(CancellationError()))
            let transport = AssetExecutorTransport([remote.record.name: operation])
            let executor = CloudAssetReadExecutor(transport: transport)
            let finished = Mutex(false)
            let run = Task {
                do { try await executor.run(pass); Issue.record("Cancelled read succeeded") }
                catch { #expect(error is CancellationError) }
                finished.withLock { $0 = true }
            }
            try await withCloudTask(gates: [operation.terminal], operation: { await run.value }, whileRunning: {
                try await operation.terminal.waitForArrival()
                expectAssetExecutorFailure(await Task { try await executor.run(pass) }.result, .busy)
                let closesFinished = Mutex(0)
                let firstClose: Task<Void, Never>?
                let secondClose: Task<Void, Never>?
                if close {
                    firstClose = Task { await executor.close(); closesFinished.withLock { $0 += 1 } }
                    secondClose = Task { await executor.close(); closesFinished.withLock { $0 += 1 } }
                } else {
                    firstClose = nil; secondClose = nil
                    run.cancel()
                }
                do {
                    try await operation.waitUntilCancelled()
                    #expect(!finished.withLock { $0 })
                    #expect(closesFinished.withLock { $0 } == 0)
                    #expect(operation.replyCalls.withLock { $0 } == 1)
                    #expect(try await remote.observe().retryCount == 1)
                    await operation.terminal.open()
                    await firstClose?.value
                    await secondClose?.value
                    if close { #expect(closesFinished.withLock { $0 } == 2) }
                } catch {
                    await operation.terminal.open()
                    await firstClose?.value; await secondClose?.value
                    throw error
                }
            })
            #expect(finished.withLock { $0 })
            let observed = try await remote.observe()
            #expect(observed.rowFields == nil && observed.retryCount == 1)
            // Selecting the same entry on a new actual retry proves the old
            // accepted selection retired, even though its task was cancelled.
            let later = try await assetExecutorRetryPass(fixture, policy: policy, records: [])
            let selected = try #require(try await later.next())
            #expect(selected.name == remote.record.name)
            #expect(try await later.cancel(selected.handle))
            try await later.finish()
            await executor.close()
            expectAssetExecutorFailure(await Task { try await executor.run(pass) }.result, .closed)
            #expect(transport.preparedNames.withLock { $0 } == [remote.record.name])
        }
    }

    @Test func assetExecutorAcceptedAccountingErrorJoinsOriginalTerminalAndLeavesOtherEntryUntouched() async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let original = try await AssetExecutorRemote.make(1)
            let untouched = try await AssetExecutorRemote.make(2)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [original, untouched])
            let operation = AssetExecutorOperation(outcome: .success(.assetRead(.failed)))
            let transport = AssetExecutorTransport([original.record.name: operation])
            let executor = CloudAssetReadExecutor(transport: transport)
            try await MaintenanceLane.shared.call("test.cloudAssetReadFailNextAccounting") {
                try MeguriCoreBindings.testCloudAssetReadFailNextAccounting()
            }
            let finished = Mutex(false)
            try await withCloudTask(gates: [operation.terminal], operation: {
                do { try await executor.run(pass); Issue.record("Actual accounting fault became success") }
                catch {
                    #expect(error is BridgeError)
                    #expect(!(error is CancellationError))
                }
                finished.withLock { $0 = true }
            }, whileRunning: {
                try await operation.terminal.waitForArrival()
                #expect(operation.enqueueCalls.withLock { $0 } == 1)
                #expect(operation.cancelled.withLock { $0 })
                #expect(!finished.withLock { $0 })
                #expect(try await original.observe().retryCount == 0)
                #expect(try await original.observe().ownedCount == 1)
                #expect(try await untouched.observe().retryCount == 0)
                #expect(transport.preparedNames.withLock { $0 } == [original.record.name])
                // The REAL failed accounting transaction fences new admission.
                await #expect(throws: (any Error).self) {
                    _ = try await fixture.core.beginFirstAssetReadPass(sdk: fixture.engine().instanceID)
                }
                await operation.terminal.open()
            })
            #expect(finished.withLock { $0 })
            #expect(operation.replyCalls.withLock { $0 } == 1)
            #expect(try await original.observe().rowFields == nil)
            #expect(try await original.observe().ownedCount == 0)
            #expect(try await untouched.observe().rowFields == nil)
            #expect(try await untouched.observe().retryCount == 0)
            #expect(transport.preparedNames.withLock { $0 } == [original.record.name])
            await executor.close()
        }
    }

    @Test(arguments: [false, true])
    func assetExecutorWrongReplyAndStrictTransportErrorSettleFailedRead(wrongReply: Bool) async throws {
        try await withNativePolicyFixture { fixture in
            let policy = try await assetExecutorPolicy(fixture)
            let remote = try await AssetExecutorRemote.make(1)
            let pass = try await assetExecutorRetryPass(fixture, policy: policy, records: [remote])
            let operation = AssetExecutorOperation(outcome: wrongReply ? .success(.read(.missing)) : .failure(CloudOwnTransportFailure.missingReply))
            let executor = CloudAssetReadExecutor(transport: AssetExecutorTransport([remote.record.name: operation]))
            await operation.terminal.open()
            let result = await Task { try await executor.run(pass) }.result
            expectAssetExecutorFailure(result, wrongReply ? .unexpectedReply : .missingReply)
            let observed = try await remote.observe()
            #expect(observed.rowFields == nil && observed.retryCount == 1 && !observed.changedByIncoming)
            let later = try await assetExecutorRetryPass(fixture, policy: policy, records: [])
            let selected = try #require(try await later.next())
            #expect(selected.name == remote.record.name)
            #expect(try await later.cancel(selected.handle))
            try await later.finish()
            await executor.close()
        }
    }

    @Test func assetExecutorEmptyPassFinishesOnceWithoutPreparingTransport() async throws {
        try await withNativePolicyFixture { fixture in
            _ = try await assetExecutorPolicy(fixture)
            let pass = try await fixture.core.beginFirstAssetReadPass(sdk: fixture.engine().instanceID)
            let transport = AssetExecutorTransport()
            let executor = CloudAssetReadExecutor(transport: transport)
            let result = try await executor.run(pass)
            #expect(result.outcome == .succeeded && result.readCount == 0 && result.failures.isEmpty)
            #expect(transport.preparedNames.withLock { $0.isEmpty })
            await #expect(throws: (any Error).self) { _ = try await pass.next() }
            await executor.close()
        }
    }
}
