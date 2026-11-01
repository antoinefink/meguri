import Foundation
import Testing
@testable import AppModel
@testable import CoreBridge

#if os(macOS)
extension ReviewCollectionStartupTests {
    @Test func ownedShutdownCancellationAcknowledgesOriginalMediaAndReopensHealthy() async throws {
        try await withShutdownCommands { media, _ in
            let original = SyncOwnerFake(), successor = SyncOwnerFake()
            let commandGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
            var attachments = 0, returnedCancelled = false, executions = 0, closeReturned = false
            original.executeCommand = { _ in
                executions += 1
                #expect(!Task.isCancelled)
                await commandGate.hold()
                returnedCancelled = Task.isCancelled
                return .init(outputs: [.commands([media])], error: CancellationError(), deferred: false)
            }
            original.onQuiesce = { await closeGate.hold() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
                attachments += 1; return attachments == 1 ? original : successor
            }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 100)
            await sync.prepareConnectionAuthority()
            original.nextCommands = [media]
            sync.receive(.refresh(.cloud))
            await commandGate.waitUntilHeld()
            let close = Task { let result = await sync.cancelAndDrain(); closeReturned = true; return result }
            await closeGate.waitUntilHeld()
            #expect(!closeReturned && !returnedCancelled)
            closeGate.release()
            await original.waitForQuiesce()
            #expect(original.quiesced && !original.closed && !closeReturned && !returnedCancelled)
            commandGate.release()
            #expect(await close.value)
            #expect(returnedCancelled && closeReturned && executions == 1)
            #expect(original.closed && original.closeCount == 1)
            #expect(!sync.operationFailed && sync.failureRevision == 0)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 101)
            await sync.prepareConnectionAuthority()
            #expect(attachments == 2 && sync.setupState == .attached && !sync.operationFailed)
            #expect(await sync.cancelAndDrain())
        }
    }

    @Test func normalShutdownResultsRunAndAcknowledgeRequiredEffectsBeforeCloseFinishes() async throws {
        try await withShutdownCommands { media, update in
            for lateDelivery in [false, true] {
                for requiredFailure in [false, true] {
                    let backend = SyncOwnerFake()
                    let commandGate = SyncOwnerGate(), requiredGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
                    var executions = 0, closeReturned = false
                    backend.executeCommand = { command in
                        executions += 1
                        if case .resumeMedia = command.action {
                            #expect(executions == 1 && !Task.isCancelled)
                            await commandGate.hold()
                            #expect(Task.isCancelled && SyncCommandTaskContext.current?.shutdownCancelled == true)
                            return .init(outputs: [.commands([update])], error: CancellationError(), deferred: false)
                        }
                        guard case .updatePending = command.action else {
                            Issue.record("Expected required updatePending effect")
                            return .init(outputs: [], error: nil, deferred: false)
                        }
                        #expect(executions == 2 && !Task.isCancelled)
                        #expect(backend.quiesced == lateDelivery && !backend.closed)
                        await requiredGate.hold()
                        let error: (any Error)? = requiredFailure ? BridgeError.internal("synthetic-required-effect-failure") : nil
                        return .init(outputs: [.commands([media])], error: error, deferred: false)
                    }
                    backend.onQuiesce = { await closeGate.hold() }
                    let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
                    await sync.preparePlatform(); await sync.completeOpenedHost(generation: 150)
                    await sync.prepareConnectionAuthority()
                    backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
                    await commandGate.waitUntilHeld()
                    let close = Task { let result = await sync.cancelAndDrain(); closeReturned = true; return result }
                    await closeGate.waitUntilHeld()
                    if lateDelivery { closeGate.release(); await backend.waitForQuiesce() }
                    commandGate.release()
                    await requiredGate.waitUntilHeld()
                    #expect(!closeReturned && executions == 2)
                    if !lateDelivery { closeGate.release(); await backend.waitForQuiesce() }
                    #expect(backend.quiesced && !backend.closed && !closeReturned)
                    requiredGate.release()
                    #expect(await close.value)
                    #expect(closeReturned && executions == 2 && backend.closeCount == 1)
                    #expect(sync.operationFailed == requiredFailure)
                    #expect(sync.failureRevision == (requiredFailure ? 1 : 0))
                }
            }
        }
    }

    @Test func backgroundExpiryCancellationHeldThroughCloseRemainsUnownedAndSticky() async throws {
        try await withShutdownCommands { media, _ in
            let backend = SyncOwnerFake()
            let commandGate = SyncOwnerGate(), cancelledGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
            var closeReturned = false, returnedCancelled = false
            backend.executeCommand = { _ in
                #expect(!Task.isCancelled)
                let context = SyncCommandTaskContext.current
                #expect(context != nil)
                await commandGate.hold()
                #expect(Task.isCancelled && context?.shutdownCancellationRequested == false)
                await cancelledGate.hold()
                returnedCancelled = Task.isCancelled
                #expect(context?.shutdownCancellationRequested == false)
                return .init(outputs: [], error: CancellationError(), deferred: false)
            }
            backend.onQuiesce = { await closeGate.hold() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 175)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
            await commandGate.waitUntilHeld()
            sync.backgroundExpired()
            commandGate.release()
            await cancelledGate.waitUntilHeld()
            let close = Task { let result = await sync.cancelAndDrain(); closeReturned = true; return result }
            await closeGate.waitUntilHeld()
            closeGate.release(); await backend.waitForQuiesce()
            #expect(backend.quiesced && !backend.closed && !closeReturned && !returnedCancelled)
            cancelledGate.release()
            #expect(await close.value)
            #expect(returnedCancelled && closeReturned && backend.closeCount == 1)
            #expect(sync.operationFailed && sync.failureRevision == 1)
        }
    }

    @Test func currentCommandFailuresStayStickyIncludingDuringShutdown() async throws {
        try await withShutdownCommands { media, _ in
            for duringClose in [false, true] {
                let errors: [any Error] = [CancellationError(), BridgeError.cloudPolicy(.staleCommand),
                                          BridgeError.internal("synthetic-command-failure")]
                for error in errors {
                    let backend = SyncOwnerFake(), gate = SyncOwnerGate()
                    var cancelled = false
                    backend.executeCommand = { _ in
                        await gate.hold(); cancelled = Task.isCancelled
                        return .init(outputs: [], error: error, deferred: false)
                    }
                    backend.onQuiesce = { gate.release() }
                    let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
                    await sync.preparePlatform(); await sync.completeOpenedHost(generation: 200)
                    await sync.prepareConnectionAuthority()
                    backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
                    await gate.waitUntilHeld()
                    if !duringClose {
                        gate.release(); await backend.waitForExecution(); await sync.waitForInputs()
                        #expect(!cancelled && sync.operationFailed && sync.failureRevision == 1)
                    }
                    #expect(await sync.cancelAndDrain())
                    let expectedFailure = !duringClose || !(error is CancellationError)
                    #expect(cancelled == duringClose)
                    #expect(sync.operationFailed == expectedFailure)
                    #expect(sync.failureRevision == (expectedFailure ? 1 : 0))
                    #expect(backend.closed && backend.closeCount == 1)
                }
            }
        }
    }

    @Test func genuineFailureOutputsDuringCloseStillReportAndRunRequiredEffects() async throws {
        try await withShutdownCommands { media, update in
            let backend = SyncOwnerFake(), commandGate = SyncOwnerGate(), closeGate = SyncOwnerGate()
            var executions = 0
            backend.executeCommand = { command in
                executions += 1
                if case .resumeMedia = command.action {
                    await commandGate.hold()
                    #expect(Task.isCancelled)
                    return .init(outputs: [.coreActionFailed(error: .cloudPolicy(.staleCommand), commands: [update]),
                                           .applyDeferred(error: .cancelled)],
                        error: CancellationError(), deferred: false)
                }
                #expect(!backend.closed)
                // Required follow-up work was issued after the shutdown cancel
                // pass: its CancellationError has no original-task cancellation.
                #expect(!Task.isCancelled)
                return .init(outputs: [], error: CancellationError(), deferred: false)
            }
            backend.onQuiesce = { await closeGate.hold() }
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 250)
            await sync.prepareConnectionAuthority()
            backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
            await commandGate.waitUntilHeld()
            let close = Task { await sync.cancelAndDrain() }
            await closeGate.waitUntilHeld()
            commandGate.release()
            await backend.waitForExecution(); await backend.waitForExecution()
            await sync.waitForInputs()
            #expect(executions == 2 && sync.operationFailed && sync.failureRevision == 3)
            closeGate.release()
            #expect(await close.value)
            #expect(sync.operationFailed && backend.closed)
        }
    }

}
#endif

extension ReviewCollectionStartupTests {
    @Test func unownedAccountCancellationDuringCloseRemainsFailure() async {
        let backend = SyncOwnerFake()
        backend.holdAccount = true
        backend.accountResult = .init(outputs: [], error: CancellationError(), deferred: false)
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 300)
        await backend.waitForAccount()
        #expect(await sync.cancelAndDrain())
        #expect(sync.operationFailed && sync.failureRevision == 1)
    }

    @Test func shutdownCloseFailureRetainsBackendAndFailureForExplicitDrainRetry() async {
        let backend = SyncOwnerFake()
        backend.onClose = { throw CancellationError() }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
        await sync.preparePlatform(); await sync.completeOpenedHost(generation: 400)
        await sync.prepareConnectionAuthority()
        #expect(!(await sync.cancelAndDrain()))
        #expect(sync.operationFailed && !backend.closed && backend.closeCount == 0)
        backend.onClose = nil
        #expect(await sync.cancelAndDrain())
        #expect(sync.operationFailed && backend.closed && backend.closeCount == 1)
    }

}

#if os(macOS)
extension ReviewCollectionStartupTests {
    @Test func replacedResultsAndSameGenerationRetiredCallbacksRemainFenced() async throws {
        try await withShutdownCommands { media, _ in
            // Reusing the local generation across close/reopen also exercises
            // hostRevision fencing, independently of the generation comparison.
            for replacement in [false, true] {
                let original = SyncOwnerFake(), successor = SyncOwnerFake(), gate = SyncOwnerGate()
                var attachments = 0, executions = 0
                var callbacks: [@Sendable () async -> Void] = []
                original.executeCommand = { _ in
                    executions += 1; await gate.hold()
                    return .init(outputs: [.commands([media])], error: BridgeError.cloudPolicy(.staleCommand), deferred: false)
                }
                original.onQuiesce = { gate.release() }
                let sync = SyncCoordinator(bootstrap: {}, makeBackend: { persisted in
                    callbacks.append(persisted)
                    attachments += 1; return attachments == 1 ? original : successor
                }, defaults: nil)
                await sync.preparePlatform(); await sync.completeOpenedHost(generation: 500)
                await sync.prepareConnectionAuthority()
                original.nextCommands = [media]; sync.receive(.refresh(.cloud))
                await gate.waitUntilHeld()
                if replacement {
                    sync.collectionReplaced(501); await sync.waitForReplacement()
                    #expect(!sync.operationFailed && sync.failureRevision == 0)
                } else {
                    #expect(await sync.cancelAndDrain())
                    #expect(sync.operationFailed && sync.failureRevision == 1)
                    await sync.preparePlatform(); await sync.completeOpenedHost(generation: 500)
                    #expect(sync.operationFailed && sync.failureRevision == 1)
                }
                await sync.prepareConnectionAuthority()
                #expect(attachments == 2 && executions == 1 && sync.setupState == .attached)
                let reads = successor.persistedReads
                await callbacks[0](); await sync.waitForInputs()
                #expect(original.persistedReads == 0 && successor.persistedReads == reads)
                #expect(await sync.cancelAndDrain())
            }
        }
    }
}

extension ReviewCollectionStartupTests {
    @Test func genuineShutdownResultFinalizesOnOriginalAuthorityBeforeNativeClose() async throws {
        try await withShutdownHarness { harness in
            let backend = try harness.backend
            let engine = try #require(await harness.factory.engines.last)
            let originalGuard = try await backend.workflow.currentGuard()
            let resultGate = SyncOwnerGate(), updateGate = SyncOwnerGate()
            let exchangeHeld = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            let updateHeld = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
            var updateContext: SyncCommandTaskContext?
            var finalizedUpdates = 0, heldUpdateFinalized = false
            backend.afterExecution = { command, result in
                if case .runExchange = command.action {
                    #expect(!Task.isCancelled && result.error == nil && !result.deferred)
                    exchangeHeld.continuation.yield(true)
                    await resultGate.hold()
                    #expect(Task.isCancelled && SyncCommandTaskContext.current?.shutdownCancelled == true)
                }
                if case .updatePending = command.action, backend.quiesced {
                    finalizedUpdates += 1
                    #expect(result.error == nil && !result.deferred)
                    if let updateContext, SyncCommandTaskContext.current === updateContext {
                        heldUpdateFinalized = true
                        #expect(Task.isCancelled && updateContext.shutdownCancelled)
                        #expect(updateContext.generation == harness.owner.generation)
                    } else {
                        #expect(!Task.isCancelled)
                    }
                    #expect(!backend.closed && backend.closeEntered == 0)
                    do {
                        try await backend.workflow.validateSDK(engine.instanceID)
                        #expect(try await backend.workflow.currentGuard() == originalGuard)
                    } catch { Issue.record("Original SDK validation failed") }
                }
            }
            defer {
                resultGate.release(); updateGate.release()
                backend.beforeExecution = nil; backend.afterExecution = nil
                exchangeHeld.continuation.finish(); updateHeld.continuation.finish()
            }
            var close: Task<Bool, Never>?
            do {
                harness.sync.receive(.network(reachable: true))
                harness.sync.syncNow()
                try #require(try await serviceAppSignal(exchangeHeld.stream, stage: "shutdown exchange result held"))
                backend.beforeExecution = { command in
                    if case .updatePending = command.action, updateContext == nil {
                        let context = SyncCommandTaskContext.current
                        #expect(context != nil && context?.generation == harness.owner.generation)
                        #expect(!Task.isCancelled)
                        updateContext = context
                        updateHeld.continuation.yield(true)
                        await updateGate.hold()
                        #expect(Task.isCancelled && context?.shutdownCancelled == true)
                        #expect(backend.quiesced && !backend.closed && backend.closeEntered == 0)
                    }
                }
                let fixture = try InitialCloudFixture.load()
                let seed = try #require(fixture.notes.sorted { $0.id < $1.id }.first { !$0.fields.isEmpty })
                let note = try await CoreNoteService().readNote(id: seed.id)
                var fields = note.fields
                fields[0] += " Synthetic shutdown edit"
                let mutation = try await CoreNoteService().updateNote(.init(id: note.id, fields: fields, tags: note.tags))
                try #require(mutation.written && mutation.id == note.id)
                try await CoreCollectionService().flushForBackground()
                try await backend.workflow.awaitTracking(generation: mutation.summary.generation, seq: mutation.summary.seq)
                await harness.sync.waitForInputs()
                harness.sync.receive(.refresh(.local))
                await harness.sync.waitForInputs()
                try #require(try await serviceAppSignal(updateHeld.stream, stage: "shutdown pending update held"))
                close = Task { await harness.owner.close() }
                try await backend.waitForQuiesce()
                #expect(!backend.closed && backend.closeEntered == 0)
                try await backend.workflow.validateSDK(engine.instanceID)
                #expect(try await backend.workflow.currentGuard() == originalGuard)
            } catch {
                backend.beforeExecution = nil; backend.afterExecution = nil
                resultGate.release(); updateGate.release()
                if let close { _ = await close.value }
                else { _ = await harness.owner.close() }
                throw error
            }
            resultGate.release(); updateGate.release()
            let closing = try #require(close)
            #expect(await closing.value)
            #expect(finalizedUpdates > 0)
            #expect(heldUpdateFinalized)
            #expect(!harness.sync.operationFailed && harness.sync.failureRevision == 0)
            #expect(backend.closed && backend.closeEntered == 1 && backend.closeReturned == 1)
        }
    }
}

// The real harness retains its host; only copied, observed opaque handles reach
// SyncOwnerFake. Its scheduler assertions do not prove native SDK acceptance.
@MainActor private func withShutdownCommands(
    _ body: (CloudPolicyCommand, CloudPolicyCommand) async throws -> Void
) async throws {
    try await withShutdownHarness { harness in
        let backend = try harness.backend
        let observed = AsyncStream<Bool>.makeStream(bufferingPolicy: .bufferingNewest(1))
        defer { backend.onCommands = nil; observed.continuation.finish() }
        var media: CloudPolicyCommand?, update: CloudPolicyCommand?
        backend.onCommands = { commands in
            for command in commands {
                switch command.action {
                case .resumeMedia where media == nil: media = command
                case .updatePending where update == nil: update = command
                default: break
                }
            }
            if media != nil && update != nil {
                observed.continuation.yield(true)
                observed.continuation.finish()
            }
        }
        try await harness.exchange(phase: .aBaseline)
        // RecordExchange can issue ResumeMedia after exchange() returns.
        try #require(try await serviceAppSignal(observed.stream, stage: "shutdown fixture issued commands"))
        backend.onCommands = nil
        try await body(try #require(media), try #require(update))
    }
}

@MainActor private func withShutdownHarness(
    _ body: (ServiceAppHarness) async throws -> Void
) async throws {
    let fixture = try InitialCloudFixture.load()
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-shutdown-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    defer { try? FileManager.default.removeItem(at: root) }
    let serviceDirectory = root.appending(path: "service")
    try FileManager.default.createDirectory(at: serviceDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, transport: .binaryAssetsV1)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let harness = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire)
        active = harness
        defer { harness.defaults.removePersistentDomain(forName: harness.suite) }
        try await wire.seed(fixture)
        try await harness.adopt()
        try await body(harness)
        try #require(await harness.owner.close())
        let exit = await process.close()
        try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
    } catch {
        if let active { await active.cleanupAfterServiceFailure() }
        else {
            let exit = await process.cancel()
            #expect(exit.joined, "Shutdown fixture must join its owned Service process")
        }
        throw error
    }
}
#endif
