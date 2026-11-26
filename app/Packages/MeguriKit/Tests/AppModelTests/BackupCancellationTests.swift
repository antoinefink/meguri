@testable import CoreBridge
import Foundation
import Testing
@testable import AppModel

extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(1)), arguments: ["background", "explicit", "shutdown"])
    func cancelledBackupInspectionReattachesProviderBeforeLifecycleDrain(cancellation: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-inspection-cancel-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appending(path: "Collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let fixture = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: fixture, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.deviceDefaults = nil
        let providerDirectory = root.appending(path: "synthetic-installation")
        let service = BackupServiceProbe()
        let entry = BackupEntry(name: "backup-2030-01-02-09.00.00.colpkg", kind: .backup,
            wallTime: .init(year: 2030, month: 1, day: 2, hour: 9, minute: 0, second: 0), sizeBytes: 1234)
        await service.configure(entries: [entry], refusal: .init(cause: .cancelled))
        var backends: [BackupCancellationBackend] = []
        var observeSuccessor: (@MainActor (String) async -> Void)?
        defer { for backend in backends { backend.onBoundary = nil }; observeSuccessor = nil }
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            let backend = BackupCancellationBackend(provider: try await CoreProviderService.attached())
            if !backends.isEmpty { backend.onBoundary = { await observeSuccessor?($0) } }
            backends.append(backend)
            return backend
        }, defaults: nil, providerDirectory: providerDirectory, cloudConfigured: false)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: providerDirectory, maintenanceService: service)
        owner.studyPreparation.runsUnprompted = false
        var shutdown: Task<Bool, Never>?
        var restore: Task<Void, Never>?
        do {
            try #require(try await CoreCollectionService().lifecycle() == .noCollection)
            await owner.start()
            await sync.prepareConnectionAuthority()
            await owner.studyPreparation.waitForJob()
            await owner.waitForLifecycle()
            await owner.backupTrigger.drain()
            // Off still needs a real provider facade on the retained Ready Host.
            // Its public iCloud setup state deliberately stays unavailable.
            let generation = owner.generation
            #expect(sync.setupState == .configurationUnavailable)
            #expect(sync.providerState?.selection == .localOnly)
            #expect(!sync.settings.enabled)
            #expect(!sync.operationFailed)
            #expect(owner.sync === sync)
            #expect(sync.syncOwnerAttached, "Startup did not attach the Off provider")
            #expect(backends.count == 1, "Startup must construct exactly one backend")
            try #require(owner.state == .ready, "Real open failed: \(owner.error ?? String(describing: owner.state))")
            try #require(generation > 0, "Startup did not establish a Ready generation")
            try #require(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
            try #require(sync.syncOwnerAttached, "Cannot test reattachment without an attached predecessor")
            try #require(backends.count == 1)
            let predecessor = try #require(backends.first)
            let capturedProvider = try #require(predecessor.providerService)
            let providerState = try await capturedProvider.state()
            _ = try await CoreDeckService().addDeck(name: "Synthetic cancellation survivor")
            let decks = try await CoreDeckService().allDeckNames()
            sync.receive(.network(reachable: false))
            await sync.prepareConnectionAuthority()
            await owner.backupTrigger.drain()
            var terminalTaskWasCancelled = false
            var terminalAttachments = 0
            // Observe the attached owner on its ordered policy lane before
            // lifecycle code may freeze, resume or close that backend.
            observeSuccessor = { [weak owner, weak sync] boundary in
                guard let owner, let sync else { Issue.record("Lost restore owner"); return }
                if boundary == "settings" {
                    terminalAttachments += 1
                    #expect(terminalTaskWasCancelled)
                    #expect(!Task.isCancelled, "Terminal reattachment inherited restore cancellation")
                    #expect(owner.backups.failure == .cancelled)
                } else if boundary == "freeze" || boundary == "resume" || boundary == "quiesce" {
                    #expect(terminalAttachments == 1, "Lifecycle reached \(boundary) before terminal attachment")
                    #expect(!owner.backups.busy, "Lifecycle passed an undrained restore")
                } else { return }
                #expect(owner.sync === sync, "Restore must reuse its captured coordinator")
                #expect(sync.syncOwnerAttached, "No attached owner at \(boundary)")
                #expect(owner.state == .ready)
                #expect(owner.generation == generation)
                #expect(sync.generation == generation)
                do {
                    #expect(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
                    #expect(try await CoreDeckService().allDeckNames() == decks)
                    let retained = try await capturedProvider.state()
                    #expect(retained.selection == providerState.selection)
                    #expect(retained.revision == providerState.revision)
                } catch { Issue.record(error) }
            }
            await owner.backups.openedRestore()
            await service.configureDue(deadline: nil, held: true)
            await service.beforeRestore { @MainActor in terminalTaskWasCancelled = Task.isCancelled }
            let admittedRestore: Task<Void, Never> = try #require(owner.backups.restoreConfirmed(name: entry.name))
            var restoreEnded = false
            restore = Task { await admittedRestore.value; restoreEnded = true }
            try await waitForBackupCancellationStage("restore inspection admission") { await service.restoreOperation != nil }
            let operation = try #require(await service.restoreOperation)
            // Quiescence is the inspection boundary; close can follow it.
            #expect(predecessor.delegate.quiesced)
            #expect(predecessor.quiesces == 1)
            #expect(backends.count == 1)
            #expect(predecessor.boundaries.first == "settings")
            #expect(predecessor.boundaries.contains("offline"))
            #expect(owner.generation == generation)
            var shutdownEnded = false
            switch cancellation {
            case "explicit": owner.backups.cancel()
            case "shutdown": shutdown = Task { let result = await owner.closeForQuit(); shutdownEnded = true; return result }
            default: owner.setForeground(false)
            }
            try await waitForBackupInspectionCancellation(operation)
            // Cancellation requests cannot abandon the held terminal result.
            #expect(owner.backups.busy)
            #expect(!restoreEnded)
            #expect(!shutdownEnded)
            #expect(backends.count == 1)
            #expect(terminalAttachments == 0)
            #expect(!predecessor.boundaries.contains("freeze"))
            #expect(!predecessor.boundaries.contains("resume"))
            #expect(try await CoreDeckService().allDeckNames() == decks)
            #expect(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
            await service.release()
            try await waitForBackupCancellationStage("terminal restore reconciliation") { restoreEnded }
            await restore?.value
            #expect(terminalTaskWasCancelled)
            if cancellation != "shutdown" { #expect(owner.backups.failure == .cancelled) }
            #expect(!owner.backups.busy)
            #expect(await service.restoredNames == [entry.name])
            #expect(!owner.backups.candidateActive)
            #expect(!owner.backups.activationUncertain && !owner.backups.restoreCompleted)
            try #require(backends.count == 2)
            let reattached = backends[1]
            #expect(reattached !== predecessor)
            #expect(terminalAttachments == 1)
            #expect(predecessor.delegate.closeCount == 1)
            #expect(predecessor.quiesces == 1)
            #expect(predecessor.freezes == 0 && predecessor.resumes == 0)
            #expect(owner.sync === sync)
            #expect(owner.generation == generation)
            #expect(owner.error == nil)
            if let shutdown {
                try await waitForBackupCancellationStage("shutdown after terminal attachment") { shutdownEnded }
                try #require(await shutdown.value)
                #expect(reattached.boundaries == ["settings", "quiesce", "close"])
                #expect(reattached.freezes == 0 && reattached.resumes == 0)
                // Off has no policy quit command; closeForQuit acknowledges it
                // directly, then quiesces and closes the reattached backend.
                #expect(!reattached.boundaries.contains("quit"))
                #expect(try await CoreCollectionService().lifecycle() == .noCollection)
            } else {
                if cancellation == "explicit" { owner.setForeground(false) }
                await owner.waitForLifecycle()
                #expect(reattached.freezes == 1)
                #expect(reattached.delegate.closeCount == 0)
                #expect(reattached.boundaries == ["settings", "expired", "freeze"])
                let retainedProviderState = try await capturedProvider.state()
                #expect(retainedProviderState.selection == providerState.selection)
                #expect(retainedProviderState.revision == providerState.revision)
                owner.setForeground(true)
                await owner.waitForLifecycle()
                await sync.waitForInputs()
                #expect(reattached.resumes == 1)
                #expect(sync.syncOwnerAttached)
                #expect(owner.sync === sync)
                #expect(reattached.boundaries == ["settings", "expired", "freeze", "resume"])
                #expect(backends.count == 2, "Resume must reuse the terminally attached backend")
                #expect(owner.backups.canCreate && owner.backups.canOpenRestore)
                #expect(try await CoreDeckService().allDeckNames() == decks)
                #expect(owner.state == .ready && owner.generation == generation)
                #expect(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
                try #require(await owner.close())
            }
            #expect(reattached.quiesces == 1)
            #expect(reattached.delegate.closeCount == 1)
            #expect(backends.count == 2)
            #expect(try await CoreCollectionService().lifecycle() == .noCollection)
        } catch {
            await service.release(); await restore?.value; _ = await shutdown?.value
            if !(await owner.close()) { Issue.record("Cleanup close refused: \(owner.error ?? "unknown error")") }
            throw error
        }
    }
}

@MainActor private final class BackupCancellationBackend: SyncCoordinatorBackend {
    let delegate = SyncOwnerFake()
    let providerService: CoreProviderService?
    var boundaries: [String] = []
    var onBoundary: (@MainActor (String) async -> Void)?
    var quiesces = 0
    init(provider: CoreProviderService) { providerService = provider }
    private func record(_ boundary: String) async {
        boundaries.append(boundary)
        await onBoundary?(boundary)
    }
    var freezes = 0
    var resumes = 0
    var connection: CoreCloudConnectionService? { nil }
    var own: CloudOwnExecutor? { nil }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput {
        switch event {
        case .settings: await record("settings")
        case .network(false): await record("offline")
        case .background: await record("background")
        case .backgroundExpiring: await record("expired")
        case .quitRequested: await record("quit")
        default: break
        }
        return try await delegate.input(event)
    }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult { await delegate.execute(command) }
    func beginAccountRead(transition: Bool) async throws -> UInt64 { try await delegate.beginAccountRead(transition: transition) }
    func readAccount(_ read: UInt64) async -> SyncCommandResult { await delegate.readAccount(read) }
    func freezeForSuspend() async throws { freezes += 1; await record("freeze") }
    func resumeAfterCoreResume() async { resumes += 1; await record("resume") }
    func quiesce() async throws { try await delegate.quiesce(); quiesces += 1; await record("quiesce") }
    func close() async throws { try await delegate.close(); await record("close") }
}

@MainActor private func waitForBackupCancellationStage(_ stage: String, until ready: @MainActor () async -> Bool) async throws {
    let deadline = ContinuousClock.now.advanced(by: .seconds(4))
    while ContinuousClock.now < deadline {
        if await ready() { return }
        try await Task.sleep(for: .milliseconds(1))
    }
    throw BridgeError.internal("Timed out waiting for \(stage)")
}

private func waitForBackupInspectionCancellation(_ operation: MaintenanceOperation) async throws {
    let deadline = ContinuousClock.now.advanced(by: .seconds(4))
    while ContinuousClock.now < deadline {
        do { try operation.check() } catch BridgeError.cancelled { return }
        try await Task.sleep(for: .milliseconds(1))
    }
    throw BridgeError.internal("Restore inspection did not receive cancellation")
}

@MainActor @Suite struct BackupPrestartCancellationTests {
    @Test(arguments: ["explicit", "drain", "task"])
    func cancellationBeforeSchedulingNeverBeginsRestoreAdmission(cancellation: String) async throws {
        let host = BackupPrestartHost(), service = BackupServiceProbe()
        let entry = BackupEntry(name: "backup-2030-01-02-09.00.00.colpkg", kind: .backup,
            wallTime: .init(year: 2030, month: 1, day: 2, hour: 9, minute: 0, second: 0), sizeBytes: 1234)
        await service.configure(entries: [entry])
        let model = BackupCoordinator(host: host, service: service)
        await model.openedRestore()
        let restore = try #require(model.restoreConfirmed(name: entry.name))
        // No suspension between scheduling and cancellation on the main actor.
        switch cancellation {
        case "explicit": model.cancel()
        case "drain": await model.cancelAndDrain()
        default: restore.cancel()
        }
        await restore.value
        #expect(host.admissions == 0 && host.finishes == 0)
        #expect(await service.restoredNames.isEmpty)
        #expect(model.failure == .cancelled && !model.busy)
    }
}

@MainActor private final class BackupPrestartHost: BackupHost {
    var backupGeneration: UInt64 { 1 }
    var backupsWritable: Bool { true }
    var backupsAvailable: Bool { true }
    var backupCancellationSafe: Bool { true }
    var admissions = 0
    var finishes = 0
    func prepareBackups() async throws {}
    func beginBackupRestore() async throws { admissions += 1 }
    func cancelBackupRestoreAdmission() {}
    func finishBackupRestore(candidateActive: Bool?, generation: UInt64?) async -> BackupRestoreCompletion {
        finishes += 1
        return .init(candidateActive: candidateActive, ready: true)
    }
    func backupRestoreSucceeded() {}
}
