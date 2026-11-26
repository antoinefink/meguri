import CoreBridge
import Foundation
import Testing
@testable import AppModel

// Real process-host operations remain in the existing serialized suite. These
// regressions use only bundled/synthetic inputs and no lifecycle fixture edits.
extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(1)), arguments: [false, true], ["entry", "manual"])
    func delayedBackupPreparationCannotRepublishPredecessorAfterReplacement(damaged: Bool, activationOwner: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-preparation-race-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let configuration = try backupRaceConfiguration(root, healthy: false)
        if damaged {
            try Data("Synthetic damaged predecessor".utf8).write(to: configuration.collectionLocation.collectionFile)
        }
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: nil),
            providerDirectory: root.appending(path: "synthetic-installation"))
        await owner.start()
        let originalGeneration = owner.generation, gate = BackupRaceGate()
        owner.readBackupLifecycle = {
            let predecessor = try await CoreCollectionService().lifecycle()
            if damaged { guard case .failed = predecessor else { throw BridgeError.internal("Expected failed predecessor") } }
            else { #expect(predecessor == .noCollection) }
            await gate.hold()
            return predecessor
        }
        let preparation = Task { try await owner.prepareBackups() }
        do {
            try await gate.waitUntilHeld()
            // Reading a retained generation must not publish half the owner.
            #expect(owner.generation == originalGeneration)
            let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
            if activationOwner == "entry" {
                owner.entry.inspect(source); await owner.entry.waitForCompletion()
                try #require(owner.entry.preview != nil)
                owner.entry.confirm(); await owner.entry.waitForCompletion()
                try #require(owner.entry.lastActivation != nil)
            } else {
                // Exercise the admission shared by manual transfers and Meguri
                // activation, with a real local replacement as its write step.
                let service = CoreCollectionEntryService()
                let location = try await owner.prepareEntry()
                let inspected = try await service.inspect(source: source, location: location,
                    operation: CollectionEntryOperation(), progress: { _ in })
                do {
                    try owner.beginManualActivation()
                    _ = try await service.activate(inspected, operation: CollectionEntryOperation(), progress: { _ in })
                    try #require(await owner.finishEntryActivation(candidateActive: true))
                    try await inspected.discard()
                } catch { try? await inspected.discard(); throw error }
            }
            try #require(owner.state == .ready && owner.generation != originalGeneration)
            let replacement = owner.generation
            gate.release()
            switch await preparation.result {
            case .success: Issue.record("Preempted preparation must refuse its stale snapshot")
            case .failure(let error): #expect(error is CancellationError)
            }
            #expect(owner.state == .ready && owner.lifecycle == .ready(generation: replacement))
            #expect(owner.generation == replacement && owner.error == nil)
            #expect(try await CoreCollectionService().lifecycle() == .ready(generation: replacement))
            // A successor can still prepare and close: no old Core reservation
            // or task cleanup may close or block its retained host.
            owner.readBackupLifecycle = { try await CoreCollectionService().lifecycle() }
            try await owner.prepareBackups()
            #expect(owner.state == .ready && owner.generation == replacement)
            try #require(await owner.close())
        } catch {
            gate.release(); _ = await preparation.result
            _ = await owner.close(); throw error
        }
    }

    @Test(.timeLimit(.minutes(1)))
    func refusedBackupDrainKeepsHealthyPredecessorAndSyncHoldUntilSafeRetry() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-drain-retry-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let configuration = try backupRaceConfiguration(root, healthy: true)
        let original = SyncOwnerFake(), service = BackupServiceProbe()
        let entry = BackupEntry(name: "backup-2030-01-02-09.00.00.colpkg", kind: .backup,
            wallTime: .init(year: 2030, month: 1, day: 2, hour: 9, minute: 0, second: 0), sizeBytes: 1234)
        await service.configure(entries: [entry], refusal: .init(cause: .corrupt))
        var factories = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in
            factories += 1
            return factories == 1 ? original : SyncOwnerFake()
        }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"), maintenanceService: service)
        await owner.start()
        do {
            try #require(owner.state == .ready)
            await sync.prepareConnectionAuthority()
            await owner.backups.openedRestore()
            let generation = owner.generation
            original.onQuiesce = { throw BridgeError.internal("Synthetic pre-write drain refusal") }
            for _ in 0..<2 {
                let refused = try #require(owner.backups.restoreConfirmed(name: entry.name))
                await refused.value
                #expect(owner.state == .ready && owner.error == nil && owner.generation == generation)
                #expect(owner.lifecycle == .ready(generation: generation))
                #expect(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
                #expect(await service.restoredNames.isEmpty)
                #expect(!owner.canStudy && !owner.backups.canCreate && owner.backups.canOpenRestore)
                #expect(sync.operationFailed && !original.closed && factories == 1)
                #expect(owner.backups.failure == .busy(runningOperation: "iCloud drain"))
                let events = original.events
                sync.receive(.foreground); await sync.waitForInputs()
                #expect(original.events == events) // Failed drain kept original sync admission closed.
            }
            original.onQuiesce = nil
            let retry = try #require(owner.backups.restoreConfirmed(name: entry.name))
            await retry.value
            #expect(await service.restoredNames == [entry.name]) // Only the drained attempt reaches inspection.
            #expect(owner.backups.failure == .corrupt && !owner.backups.candidateActive)
            #expect(owner.state == .ready && owner.error == nil && owner.generation == generation)
            #expect(owner.backups.canCreate && owner.canStudy && sync.setupState == .attached)
            #expect(original.closed && original.closeCount == 1 && factories > 1)
            try #require(await owner.close())
        } catch { original.onQuiesce = nil; _ = await owner.close(); throw error }
    }

    private func backupRaceConfiguration(_ root: URL, healthy: Bool) throws -> LaunchConfiguration {
        let directory = root.appending(path: "Collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        if healthy {
            let fixture = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
            try FileManager.default.copyItem(at: fixture, to: directory.appending(path: "collection.anki2"))
        }
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        return configuration
    }
}

/// One-shot causal hold with bounded admission and release on early failure.
@MainActor final class BackupRaceGate {
    private let starts = AsyncStream<Bool>.makeStream()
    private var continuation: CheckedContinuation<Void, Never>?
    private var released = false
    func hold() async {
        guard !released else { return }
        await withCheckedContinuation {
            continuation = $0
            starts.continuation.yield(true)
        }
    }
    func waitUntilHeld() async throws {
        let stream = starts.stream
        try await withThrowingTaskGroup(of: Bool.self) { group in
            group.addTask {
                for await started in stream { return started }
                return false
            }
            group.addTask {
                try await Task.sleep(for: .seconds(10))
                throw BridgeError.internal("Timed out waiting for backup race admission")
            }
            defer { group.cancelAll() }
            try #require(try await group.next() == true)
        }
    }
    func release() { released = true; continuation?.resume(); continuation = nil }
}
