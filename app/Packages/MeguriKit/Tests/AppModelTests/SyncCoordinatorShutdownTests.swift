import Foundation
@testable import Instrumentation
import func MeguriCoreBindings.testCloudPrepareAdapterBaseline
import Testing
@testable import AppModel
@testable import CoreBridge

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

// Uses the existing schema and Rust adapter baseline to obtain actual issued
// commands; only their execution is replaced by SyncOwnerFake. No transport runs.
@MainActor private func withShutdownCommands(
    _ body: (CloudPolicyCommand, CloudPolicyCommand) async throws -> Void
) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: root) }
    let backups = root.appending(path: "backups")
    try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
    let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
    try FileManager.default.copyItem(at: source, to: root.appending(path: "collection.anki2"))
    try await CoreCloudWorkflowService.configurePlatform(container: "synthetic-u114", environment: .development,
        status: .available, identity: Data("synthetic-account-A".utf8))
    let collection = CoreCollectionService()
    do {
        try await collection.open(.init(collectionDirectory: root, backupsDirectory: backups,
            appGroupSnapshot: root.appending(path: "widget.snapshot")), randomSeed: 114)
        try await MaintenanceLane.shared.call(BridgeCall("testCloudPrepareAdapterBaseline")) { @Sendable in
            _ = try testCloudPrepareAdapterBaseline(entityCount: 1, localRevision: 3,
                savedEngineState: nil, failNextInboxCommit: false, failNextStateCommit: false)
        }
        let workflow = CoreCloudWorkflowService(), policy = try await workflow.policy()
        _ = try await workflow.registerSDK(automatic: true)
        _ = try await policy.refreshAccount()
        _ = try await policy.input(.settings(.init(enabled: true, paused: false, automatic: true, media: true)))
        _ = try await policy.input(.network(reachable: true))
        guard case .commands(let commands) = try await policy.input(.foreground) else {
            throw BridgeError.internal("Expected baseline foreground commands")
        }
        let media = try #require(commands.first { if case .resumeMedia = $0.action { true } else { false } })
        let update = try #require(commands.first { if case .updatePending = $0.action { true } else { false } })
        try await body(media, update)
        try await collection.close()
    } catch { try? await collection.close(); throw error }
}
