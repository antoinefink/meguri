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
        let service = BackupServiceProbe()
        let entry = BackupEntry(name: "backup-2030-01-02-09.00.00.colpkg", kind: .backup,
            wallTime: .init(year: 2030, month: 1, day: 2, hour: 9, minute: 0, second: 0), sizeBytes: 1234)
        await service.configure(entries: [entry], refusal: .init(cause: .cancelled))
        var backends: [BackupCancellationBackend] = []
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            let backend = BackupCancellationBackend()
            backends.append(backend)
            return backend
        }, defaults: nil, cloudConfigured: false)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"), maintenanceService: service)
        owner.studyPreparation.runsUnprompted = false
        await owner.start()
        var shutdown: Task<Bool, Never>?
        var restore: Task<Void, Never>?
        do {
            await sync.waitForInputs()
            await owner.studyPreparation.waitForJob()
            await owner.waitForLifecycle()
            await owner.backupTrigger.drain()
            try #require(owner.state == .ready && sync.syncOwnerAttached && backends.count == 1)
            let generation = owner.generation
            let predecessor = try #require(backends.first)
            await owner.backups.openedRestore()
            await service.configureDue(deadline: nil, held: true)
            var terminalTaskWasCancelled = false
            await service.beforeRestore { @MainActor in terminalTaskWasCancelled = Task.isCancelled }
            let admittedRestore: Task<Void, Never> = try #require(owner.backups.restoreConfirmed(name: entry.name))
            restore = admittedRestore
            await service.waitForStart()
            let operation = try #require(await service.restoreOperation)
            // Inspection has drained the attached provider, before any replacement.
            #expect(predecessor.delegate.closed && backends.count == 1)
            #expect(owner.generation == generation)
            var shutdownEnded = false
            switch cancellation {
            case "explicit": owner.backups.cancel()
            case "shutdown": shutdown = Task { let result = await owner.close(); shutdownEnded = true; return result }
            default: owner.setForeground(false)
            }
            try await waitForBackupInspectionCancellation(operation)
            // Cancellation requests cannot abandon the held terminal result.
            #expect(owner.backups.busy && !shutdownEnded && backends.count == 1)
            #expect(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
            await service.release()
            await restore?.value
            #expect(terminalTaskWasCancelled)
            if cancellation != "shutdown" { #expect(owner.backups.failure == .cancelled) }
            #expect(!owner.backups.candidateActive)
            #expect(!owner.backups.activationUncertain && !owner.backups.restoreCompleted)
            try #require(backends.count == 2)
            let reattached = backends[1]
            #expect(owner.generation == generation && owner.error == nil)
            if let shutdown {
                try #require(await shutdown.value)
                #expect(reattached.delegate.closed && shutdownEnded)
                #expect(try await CoreCollectionService().lifecycle() == .noCollection)
            } else {
                if cancellation == "explicit" { owner.setForeground(false) }
                await owner.waitForLifecycle()
                #expect(reattached.freezes == 1 && !reattached.delegate.closed)
                owner.setForeground(true)
                await owner.waitForLifecycle()
                await sync.waitForInputs()
                #expect(reattached.resumes == 1 && sync.syncOwnerAttached)
                #expect(owner.state == .ready && owner.generation == generation)
                #expect(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
                try #require(await owner.close())
            }
        } catch {
            await service.release(); await restore?.value; _ = await shutdown?.value
            _ = await owner.close(); throw error
        }
    }
}

@MainActor private final class BackupCancellationBackend: SyncCoordinatorBackend {
    let delegate = SyncOwnerFake()
    var freezes = 0
    var resumes = 0
    var connection: CoreCloudConnectionService? { nil }
    var own: CloudOwnExecutor? { nil }
    func input(_ event: CloudPolicyEvent) async throws -> CloudPolicyOutput { try await delegate.input(event) }
    func execute(_ command: CloudPolicyCommand) async -> SyncCommandResult { await delegate.execute(command) }
    func beginAccountRead(transition: Bool) async throws -> UInt64 { try await delegate.beginAccountRead(transition: transition) }
    func readAccount(_ read: UInt64) async -> SyncCommandResult { await delegate.readAccount(read) }
    func freezeForSuspend() async throws { freezes += 1 }
    func resumeAfterCoreResume() async { resumes += 1 }
    func quiesce() async throws { try await delegate.quiesce() }
    func close() async throws { try await delegate.close() }
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
