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
            original.onClose = { await closeGate.hold() }
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
            await original.waitForClose()
            #expect(original.closed && !closeReturned && !returnedCancelled)
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
                        #expect(backend.closed == lateDelivery)
                        await requiredGate.hold()
                        let error: (any Error)? = requiredFailure ? BridgeError.internal("synthetic-required-effect-failure") : nil
                        return .init(outputs: [.commands([media])], error: error, deferred: false)
                    }
                    backend.onClose = { await closeGate.hold() }
                    let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil)
                    await sync.preparePlatform(); await sync.completeOpenedHost(generation: 150)
                    await sync.prepareConnectionAuthority()
                    backend.nextCommands = [media]; sync.receive(.refresh(.cloud))
                    await commandGate.waitUntilHeld()
                    let close = Task { let result = await sync.cancelAndDrain(); closeReturned = true; return result }
                    await closeGate.waitUntilHeld()
                    if lateDelivery { closeGate.release(); await backend.waitForClose() }
                    commandGate.release()
                    await requiredGate.waitUntilHeld()
                    #expect(!closeReturned && executions == 2)
                    if !lateDelivery { closeGate.release(); await backend.waitForClose() }
                    #expect(backend.closed && !closeReturned)
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
            backend.onClose = { await closeGate.hold() }
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
            closeGate.release(); await backend.waitForClose()
            #expect(backend.closed && !closeReturned && !returnedCancelled)
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
                    backend.onClose = { gate.release() }
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
            backend.onClose = { await closeGate.hold() }
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
                original.onClose = { gate.release() }
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

// The real harness retains its host; only copied, observed opaque handles reach
// SyncOwnerFake. Its scheduler assertions do not prove native SDK acceptance.
@MainActor private func withShutdownCommands(
    _ body: (CloudPolicyCommand, CloudPolicyCommand) async throws -> Void
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
