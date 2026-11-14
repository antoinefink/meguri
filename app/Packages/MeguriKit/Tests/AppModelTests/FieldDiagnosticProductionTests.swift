#if os(macOS)
import Foundation
import Testing
@testable import AppModel
@testable import CoreBridge

extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func acceptedStatusDiagnosticsSeedHistoryDeduplicateLiveResultsAndSerializeReplacement(completion: Bool) async throws {
        try await withDiagnosticStatusCommands { issued in
            let historical = issued.historical, live = issued.live, transitioning = issued.transitioning
            let original = SyncOwnerFake(), successor = SyncOwnerFake()
            var observations: [FieldDiagnosticObservation] = []
            var observationGenerations: [UInt64] = []
            weak var observedSync: SyncCoordinator?
            var factories = 0
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
                factories += 1
                return factories == 1 ? original : successor
            }, defaults: nil, recordDiagnostic: {
                observations.append($0)
                observationGenerations.append(observedSync?.generation ?? 0)
            })
            observedSync = sync
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 1)
            await sync.prepareConnectionAuthority()
            @MainActor func deliver(_ command: CloudPolicyCommand) async {
                original.nextCommands = [command]
                sync.receive(.network(reachable: true))
                await sync.waitForInputs(); await sync.drainOperations()
            }
            await deliver(historical)
            let seeded = observations
            #expect(!seeded.isEmpty)
            #expect(!seeded.contains { $0.stage == .exchangeCompleted })
            await deliver(historical)
            #expect(observations == seeded)
            await deliver(live)
            original.executeCommand = { _ in
                .init(outputs: [.coreActionFailed(error: .internal("synthetic record failure"), commands: [])],
                    error: nil, deferred: false)
            }
            await deliver(issued.historicalCompletion)
            #expect(!observations.contains { $0.stage == .exchangeCompleted })
            original.executeCommand = nil
            await deliver(issued.liveCompletion)
            let completed = observations
            #expect(completed.filter { $0.stage == .exchangeCompleted }.count == 1)
            await deliver(live)
            #expect(observations == completed)
            await deliver(issued.third)
            await deliver(live)
            await deliver(historical)
            await deliver(live)
            #expect(observations.filter { $0.stage == .exchangeCompleted }.count == 1)
            // StatusChanged and RecordExchange retain Core through result/ACK.
            // Replacement and barriers must wait for that phase; an old-revision
            // callback after successor installation is unreachable here.
            let gate = SyncOwnerGate()
            defer { gate.release() }
            var returned = 0
            original.executeCommand = { _ in
                await gate.hold()
                #expect(sync.generation == 1 && factories == 1 && !Task.isCancelled)
                returned += 1
                return .init(outputs: [], error: nil, deferred: false)
            }
            observations.removeAll()
            observationGenerations.removeAll()
            original.nextCommands = [completion ? issued.liveCompletion : transitioning]
            sync.receive(.network(reachable: true))
            await gate.waitUntilHeld()
            sync.collectionReplaced(2)
            let barrierEntered = AsyncStream<Void>.makeStream()
            defer { barrierEntered.continuation.finish() }
            var barrierReturned = false
            let barrier = Task { @MainActor in
                barrierEntered.continuation.yield(())
                await sync.waitForInputs()
                barrierReturned = true
            }
            for await _ in barrierEntered.stream { break }
            #expect(!barrierReturned && sync.generation == 1 && factories == 1 && observations.isEmpty)
            gate.release()
            await barrier.value
            #expect(barrierReturned && sync.generation == 2)
            await sync.waitForReplacement(); await sync.drainOperations()
            #expect(sync.generation == 2 && factories == 2 && original.closed && returned == 1)
            #expect(!observations.isEmpty)
            #expect(observations.filter { $0.stage == .exchangeCompleted }.count == (completion ? 1 : 0))
            #expect(observationGenerations == Array(repeating: 1, count: observations.count))
            let originalObservations = observations
            await sync.waitForInputs(); await sync.drainOperations()
            #expect(observations == originalObservations)
            observations.removeAll()
            observationGenerations.removeAll()
            successor.nextCommands = [live]
            sync.receive(.network(reachable: true))
            await sync.waitForInputs(); await sync.drainOperations()
            #expect(!observations.isEmpty)
            #expect(!observations.contains { $0.stage == .exchangeCompleted })
            #expect(observationGenerations == Array(repeating: 2, count: observations.count))
            let successorHistory = observations
            successor.nextCommands = [live]
            sync.receive(.network(reachable: true))
            await sync.waitForInputs(); await sync.drainOperations()
            #expect(observations == successorHistory)
            #expect(returned == 1)
            #expect(await sync.cancelAndDrain())
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func diagnosticsRejectCoreStatusAndCompletionReturningDuringEntryClose(completion: Bool) async throws {
        try await withDiagnosticStatusCommands { issued in
            let backend = SyncOwnerFake(), gate = SyncOwnerGate()
            defer { gate.release() }
            var observations: [FieldDiagnosticObservation] = []
            var returned = 0
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend }, defaults: nil,
                recordDiagnostic: { observations.append($0) })
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 1)
            await sync.prepareConnectionAuthority()
            backend.executeCommand = { _ in
                await gate.hold()
                #expect(Task.isCancelled && sync.generation == 1)
                returned += 1
                // Deliberately noncooperative execution: closing, rather than
                // an error result, must fence both diagnostic consumers.
                return .init(outputs: [], error: nil, deferred: false)
            }
            backend.nextCommands = [completion ? issued.liveCompletion : issued.transitioning]
            sync.receive(.network(reachable: true))
            await gate.waitUntilHeld()
            // This existing local-entry boundary closes admission synchronously
            // before joining Core; it does not wait behind the held command.
            sync.beginEntryActivation()
            gate.release()
            await sync.drainOperations()
            #expect(returned == 1 && observations.isEmpty && sync.status == nil)
            #expect(await sync.drainEntryActivation())
            #expect(observations.isEmpty && backend.closed)
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func diagnosticsRejectSupersededNonCoreOutputsCarryingStatusOrCompletion(completion: Bool) async throws {
        try await withDiagnosticStatusCommands { issued in
            let original = SyncOwnerFake(), successor = SyncOwnerFake(), gate = SyncOwnerGate()
            defer { gate.release() }
            var observations: [FieldDiagnosticObservation] = []
            var factories = 0, executions = 0
            let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
                factories += 1
                return factories == 1 ? original : successor
            }, defaults: nil, recordDiagnostic: { observations.append($0) })
            await sync.preparePlatform(); await sync.completeOpenedHost(generation: 1)
            await sync.prepareConnectionAuthority()
            original.executeCommand = { command in
                executions += 1
                if case .runExchange = command.action {
                    await gate.hold()
                    #expect(Task.isCancelled && sync.generation == 2 && factories == 1)
                    return .init(outputs: [.commands([completion ? issued.liveCompletion : issued.transitioning])],
                        error: nil, deferred: false)
                }
                Issue.record("Superseded output must not execute its status or completion")
                return .init(outputs: [], error: nil, deferred: false)
            }
            original.nextCommands = [issued.exchange]
            sync.receive(.network(reachable: true))
            await gate.waitUntilHeld()
            // RunExchange owns no Core phase here. Replacement can advance its
            // revision, but must still join the original task before installing
            // the successor. Returned old-owner outputs are rejected at ACK.
            sync.collectionReplaced(2)
            await sync.waitForInputs()
            await original.waitForQuiesce()
            #expect(sync.generation == 2 && factories == 1 && !original.closed)
            gate.release()
            await sync.waitForReplacement(); await sync.drainOperations()
            #expect(factories == 2 && original.closed && executions == 1)
            #expect(observations.isEmpty && sync.status == nil)
            successor.nextCommands = [issued.live]
            sync.receive(.network(reachable: true))
            await sync.waitForInputs(); await sync.drainOperations()
            #expect(!observations.isEmpty)
            #expect(!observations.contains { $0.stage == .exchangeCompleted })
            let successorHistory = observations
            await sync.waitForInputs(); await sync.drainOperations()
            #expect(observations == successorHistory && executions == 1)
            #expect(await sync.cancelAndDrain())
        }
    }
}

private struct DiagnosticStatusCommands {
    let historical: CloudPolicyCommand
    let live: CloudPolicyCommand
    let third: CloudPolicyCommand
    let transitioning: CloudPolicyCommand
    let historicalCompletion: CloudPolicyCommand
    let liveCompletion: CloudPolicyCommand
    let exchange: CloudPolicyCommand
}

/// Acquire opaque commands from a real retained service owner; substitute only
/// execution in the scheduler test. No fabricated command capabilities, peer
/// delivery inference, diagnostics storage, or clipboard access.
@MainActor private func withDiagnosticStatusCommands(
    _ body: @MainActor (DiagnosticStatusCommands) async throws -> Void
) async throws {
    let fixture = try InitialCloudFixture.load()
    let files = FileManager.default
    let root = files.temporaryDirectory.appending(path: "synthetic-diagnostic-status-\(UUID().uuidString)")
    try files.createDirectory(at: root, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    defer { try? files.removeItem(at: root) }
    let serviceDirectory = root.appending(path: "service")
    try files.createDirectory(at: serviceDirectory, withIntermediateDirectories: false, attributes: [.posixPermissions: 0o700])
    let process = try await CloudServiceProcess.open(stateDirectory: serviceDirectory, transport: .binaryAssetsV1)
    var active: ServiceAppHarness?
    do {
        let wire = try ServiceWorkflowWire(process: process, directory: root.appending(path: "transport"))
        let harness = try ServiceAppHarness(root: root.appending(path: "collection"), wire: wire)
        active = harness
        defer { harness.defaults.removePersistentDomain(forName: harness.suite) }
        var commands: [CloudPolicyCommand] = []
        harness.onCommands = { commands.append(contentsOf: $0) }
        defer { harness.onCommands = nil }
        try await wire.seed(fixture)
        try await harness.adopt()
        commands.removeAll()
        try await harness.exchange(phase: .aBaseline)
        await harness.sync.drainOperations()
        let historical = try #require(commands.last { command in
            if case .statusChanged(let status) = command.action { status.lastAttempt != nil } else { false }
        })
        let historicalCompletion = try #require(commands.last { command in
            if case .recordExchange = command.action { true } else { false }
        })
        commands.removeAll()
        try await harness.exchange(phase: .aMergedSave)
        await harness.sync.drainOperations()
        let live = try #require(commands.last { command in
            if case .statusChanged(let status) = command.action { status.lastAttempt != nil } else { false }
        })
        let transitioning = try #require(commands.first { command in
            if case .statusChanged(let status) = command.action, case .syncingCollection = status.state { true } else { false }
        })
        if case .statusChanged(let first) = historical.action, case .statusChanged(let second) = live.action {
            try #require(first.lastAttempt?.exchange != second.lastAttempt?.exchange)
        }
        let liveCompletion = try #require(commands.last { command in
            if case .recordExchange = command.action { true } else { false }
        })
        let exchange = try #require(commands.last { command in
            if case .runExchange = command.action { true } else { false }
        })
        commands.removeAll()
        try await harness.exchange(phase: .aMergedSave)
        await harness.sync.drainOperations()
        let third = try #require(commands.last { command in
            if case .statusChanged(let status) = command.action { status.lastAttempt != nil } else { false }
        })
        if case .statusChanged(let second) = live.action, case .statusChanged(let thirdStatus) = third.action {
            try #require(second.lastAttempt?.exchange != thirdStatus.lastAttempt?.exchange)
        }
        try await body(.init(historical: historical, live: live, third: third,
            transitioning: transitioning, historicalCompletion: historicalCompletion, liveCompletion: liveCompletion,
            exchange: exchange))
        try #require(await harness.owner.close())
        let exit = await process.close()
        try #require(exit.status == 0 && exit.joined && !exit.unexpectedOutput && !exit.ioFailed)
        try #require(exit.diagnostics.isEmpty && !exit.diagnosticLimitExceeded)
    } catch {
        if let active { await active.cleanupAfterServiceFailure() }
        else {
            let exit = await process.cancel()
            #expect(exit.joined)
        }
        throw error
    }
}
#endif
