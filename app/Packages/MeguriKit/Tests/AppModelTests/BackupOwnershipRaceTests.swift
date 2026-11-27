import CoreBridge
import Foundation
import Testing
@testable import AppModel

// Real process-host operations remain in the existing serialized suite. These
// regressions use only bundled/synthetic inputs and no lifecycle fixture edits.
extension ReviewCollectionStartupTests {
    @Test(.timeLimit(.minutes(1)), arguments: ["entry", "manual"])
    func delayedBackupPreparationCannotRepublishPredecessorAfterReplacement(activationOwner: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-preparation-race-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let configuration = try backupRaceConfiguration(root)
        let owner = backupRaceOwner(configuration, root: root)
        let gate = BackupRaceGate()
        var preparation: Task<Void, any Error>?
        do {
            try await startBackupRaceOwner(owner)
            _ = try await CoreDeckService().addDeck(name: "Synthetic predecessor only")
            let originalGeneration = owner.generation
            var snapshot: CollectionLifecycle?
            owner.readBackupLifecycle = {
                // This is a genuine old native snapshot, captured before the hold.
                let predecessor = try await CoreCollectionService().lifecycle()
                try #require(predecessor == .ready(generation: originalGeneration))
                snapshot = predecessor
                await gate.hold()
                return predecessor
            }
            let pending = Task { try await owner.prepareBackups() }
            preparation = pending
            try await waitForBackupRaceSnapshot(gate, pending: pending)
            #expect(snapshot == .ready(generation: originalGeneration))
            #expect(owner.generation == originalGeneration)
            let activated = try await activateBackupRaceReplacement(owner, activationOwner: activationOwner)
            try #require(owner.state == .ready && owner.generation == activated.generation,
                "\(owner.error ?? owner.entry.message ?? "Replacement host not ready")")
            try #require(activated.generation != originalGeneration)
            // Healthy replacement must take the real T6 safety backup, not T7 recovery.
            let safety = try #require(activated.backup)
            #expect(FileManager.default.fileExists(atPath: configuration.collectionLocation.backupsDirectory.appending(path: safety.name).path))
            #expect(try await CoreDeckService().matchDeckName(name: "Synthetic predecessor only").deck == nil)
            let replacement = owner.generation, liveSync = owner.sync
            let note = try await addBackupRaceNote()
            let cards = try await Core.cardExcerpts(note.cards)
            try #require(cards.count == 1)
            guard case .found = cards[0] else { throw BridgeError.internal("Expected a real successor card: \(cards)") }
            gate.release()
            try await requireStaleBackupPreparation(pending)
            #expect(owner.sync === liveSync && owner.generation == replacement)
            #expect(owner.state == .ready && owner.lifecycle == .ready(generation: replacement) && owner.error == nil)
            #expect(try await CoreCollectionService().lifecycle() == .ready(generation: replacement))
            #expect(try await Core.cardExcerpts(note.cards) == cards)
            #expect(try await CoreNoteService().readNote(id: note.id).fields == ["Live question", "Live answer"])
            // A fresh preparation still uses that successor, and close releases it.
            owner.readBackupLifecycle = { try await CoreCollectionService().lifecycle() }
            try await owner.prepareBackups()
            #expect(owner.state == .ready && owner.generation == replacement && owner.sync === liveSync)
            try #require(await owner.close(), "Close refused: \(owner.error ?? "unknown error")")
            #expect(try await CoreCollectionService().lifecycle() == .noCollection)
        } catch {
            gate.release(); _ = await preparation?.result
            await cleanupBackupRaceOwner(owner)
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true], ["entry", "manual"])
    func staleBackupLifecycleProjectionIsRefusedAfterUnchangedActivation(failedRead: Bool, activationOwner: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-projection-race-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let owner = backupRaceOwner(try backupRaceConfiguration(root), root: root)
        let gate = BackupRaceGate()
        var preparation: Task<Void, any Error>?
        var admitted = false
        do {
            try await startBackupRaceOwner(owner)
            let originalGeneration = owner.generation, liveSync = owner.sync
            let note = try await addBackupRaceNote()
            let cards = try await Core.cardExcerpts(note.cards)
            try #require(cards.count == 1)
            guard case .found = cards[0] else { throw BridgeError.internal("Expected a real predecessor card: \(cards)") }
            owner.readBackupLifecycle = {
                try #require(try await CoreCollectionService().lifecycle() == .ready(generation: originalGeneration))
                // Only the returned read is injected. The native host stays healthy;
                // this proves stale presentation refusal, not damaged-file recovery.
                let old: CollectionLifecycle = failedRead ? .failed(.corrupt(detail: "Synthetic stale read")) : .noCollection
                await gate.hold()
                return old
            }
            let pending = Task { try await owner.prepareBackups() }
            preparation = pending
            try await waitForBackupRaceSnapshot(gate, pending: pending)
            if activationOwner == "entry" {
                try owner.beginEntryActivation()
                admitted = true
                try await owner.drainEntryActivation()
            } else {
                try owner.beginManualActivation()
                admitted = true
            }
            // An admitted operation may finish without changing any collection.
            // The generation check alone cannot reject this stale continuation.
            let finished = await owner.finishEntryActivation(candidateActive: false)
            admitted = false
            try #require(finished, "Admission cleanup failed: \(owner.error ?? "unknown error")")
            try #require(owner.generation == originalGeneration && owner.sync === liveSync)
            gate.release()
            try await requireStaleBackupPreparation(pending)
            #expect(owner.state == .ready && owner.lifecycle == .ready(generation: originalGeneration) && owner.error == nil)
            #expect(owner.generation == originalGeneration && owner.sync === liveSync)
            #expect(try await CoreCollectionService().lifecycle() == .ready(generation: originalGeneration))
            #expect(try await Core.cardExcerpts(note.cards) == cards)
            owner.readBackupLifecycle = { try await CoreCollectionService().lifecycle() }
            try await owner.prepareBackups()
            #expect(owner.state == .ready && owner.generation == originalGeneration)
            try #require(await owner.close(), "Close refused: \(owner.error ?? "unknown error")")
            #expect(try await CoreCollectionService().lifecycle() == .noCollection)
        } catch {
            gate.release(); _ = await preparation?.result
            if admitted {
                #expect(await owner.finishEntryActivation(candidateActive: false),
                    "Failed to finish admission during cleanup: \(owner.error ?? "unknown error")")
            }
            await cleanupBackupRaceOwner(owner)
            throw error
        }
    }

    @Test(.timeLimit(.minutes(1)), arguments: ["none", "explicit", "task"])
    func refusedBackupDrainKeepsHealthyPredecessorAndSyncHoldUntilSafeRetry(cancellation: String) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-drain-retry-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let configuration = try backupRaceConfiguration(root)
        let original = SyncOwnerFake(), successor = SyncOwnerFake(), service = BackupServiceProbe()
        let entry = BackupEntry(name: "backup-2030-01-02-09.00.00.colpkg", kind: .backup,
            wallTime: .init(year: 2030, month: 1, day: 2, hour: 9, minute: 0, second: 0), sizeBytes: 1234)
        await service.configure(entries: [entry], refusal: .init(cause: .corrupt))
        var factories = 0
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: {
            factories += 1
            return factories == 1 ? original : (factories == 2 ? successor : SyncOwnerFake())
        }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"), maintenanceService: service)
        let retryDrain = BackupRaceGate()
        var pendingRetry: Task<Void, Never>?
        await owner.start()
        do {
            try #require(owner.state == .ready)
            await sync.waitForInputs()
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
                #expect(owner.backups.failure == .busy(runningOperation: "sync drain"))
                let events = original.events
                await sync.foregroundAfterLocalResume(); await sync.waitForInputs()
                #expect(original.events == events) // Failed drain kept original sync admission closed.
            }
            if cancellation != "none" {
                original.onQuiesce = { await retryDrain.hold() }
                let cancelled = try #require(owner.backups.restoreConfirmed(name: entry.name))
                pendingRetry = cancelled
                try await retryDrain.waitUntilHeld()
                // The refused attempt removed its editor decision. Cancellation
                // must still stop retry before a new replacement is admitted.
                #expect(owner.backups.canCancel && !owner.canStudy)
                if cancellation == "explicit" { owner.backups.cancel() }
                else { cancelled.cancel() }
                retryDrain.release(); await cancelled.value
                #expect(await service.restoredNames.isEmpty)
                #expect(owner.backups.failure == .cancelled && !owner.backups.busy)
                #expect(!owner.backups.candidateActive && !owner.backups.activationUncertain)
                #expect(!owner.backups.restoreCompleted && owner.backups.selectedName == entry.name)
                #expect(owner.state == .ready && owner.error == nil && owner.generation == generation)
                #expect(owner.lifecycle == .ready(generation: generation))
                #expect(try await CoreCollectionService().lifecycle() == .ready(generation: generation))
                #expect(owner.backups.canCreate)
                #expect(owner.canStudy)
                #expect(sync.setupState == .attached)
                #expect(original !== successor)
                #expect(original.closed)
                #expect(original.closeCount == 1)
                #expect(factories == 2)
                #expect(!successor.closed)
                await sync.waitForInputs()
                let originalEvents = original.events, successorEvents = successor.events
                // Background still reaches the reattached owner.
                _ = await sync.backgroundAfterLocalFlush(deadlineMilliseconds: 123_000); await sync.waitForInputs()
                #expect(successor.events == successorEvents + ["background"])
                #expect(original.events == originalEvents)
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
        } catch {
            retryDrain.release(); await pendingRetry?.value
            original.onQuiesce = nil; _ = await owner.close(); throw error
        }
    }

    private func activateBackupRaceReplacement(_ owner: ReviewCollection, activationOwner: String) async throws -> CollectionActivation {
        let source = try #require(Bundle.module.url(forResource: "review", withExtension: "colpkg"))
        if activationOwner == "entry" {
            owner.entry.inspect(source); await owner.entry.waitForCompletion()
            let preview = try #require(owner.entry.preview,
                "Inspection failed: \(String(describing: owner.entry.failure)); \(owner.entry.message ?? "no message")")
            try #require(preview.facts.replacingExisting && preview.facts.cards == 11 && preview.facts.notes == 6)
            owner.entry.confirm(); await owner.entry.waitForCompletion()
            try #require(owner.entry.failure == nil,
                "Activation failed: \(String(describing: owner.entry.failure)); \(owner.entry.message ?? "no message")")
            return try #require(owner.entry.lastActivation, "\(owner.entry.message ?? "No activation receipt")")
        }
        // Real local replacement under the admission shared by manual transfers
        // and Meguri activation. No server, fake lifecycle or fake native host.
        let service = CoreCollectionEntryService()
        let location = try await owner.prepareEntry()
        let inspected = try await service.inspect(source: source, location: location,
            operation: CollectionEntryOperation(), progress: { _ in })
        var admitted = false
        var active = false
        do {
            try #require(inspected.facts.replacingExisting && inspected.facts.cards == 11 && inspected.facts.notes == 6)
            let decision = ReviewCollection.EditorReplacementDecision()
            let admission = try #require(await owner.prepareEditorReplacement(operation: "Synthetic local replacement",
                decision: decision, current: { true }))
            defer { owner.finishEditorReplacement(admission.id) }
            try #require(admission.accepted)
            try owner.beginManualActivation(replacement: admission.id)
            admitted = true
            try decision.admitEffect()
            let activated = try await service.activate(inspected, operation: CollectionEntryOperation(), progress: { _ in })
            active = true
            let finished = await owner.finishEntryActivation(candidateActive: true)
            admitted = false
            try #require(finished, "Replacement setup failed: \(owner.error ?? "unknown error")")
            try await inspected.discard()
            return activated
        } catch {
            // Even a refused real activation must release native admission and
            // inspection ownership before the next serialized case uses Core.
            if admitted {
                let candidateActive = active || (error as? CollectionEntryFailure)?.candidateActive == true
                #expect(await owner.finishEntryActivation(candidateActive: candidateActive),
                    "Failed to finish refused activation: \(owner.error ?? "unknown error")")
            }
            do { try await inspected.discard() } catch { Issue.record(error) }
            throw error
        }
    }

    private func waitForBackupRaceSnapshot(_ gate: BackupRaceGate, pending: Task<Void, any Error>) async throws {
        do { try await gate.waitUntilHeld() }
        catch {
            gate.release()
            // Prefer the actual setup error over a missing-barrier timeout.
            try await pending.value
            throw error
        }
    }

    private func requireStaleBackupPreparation(_ pending: Task<Void, any Error>) async throws {
        switch await pending.result {
        case .success: Issue.record("Preempted preparation must refuse its stale snapshot")
        case .failure(let error): try #require(error is CancellationError, "Unexpected preparation failure: \(error)")
        }
    }

    private func backupRaceOwner(_ configuration: LaunchConfiguration, root: URL) -> ReviewCollection {
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { SyncOwnerFake() }, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
        owner.studyPreparation.runsUnprompted = false
        return owner
    }

    private func startBackupRaceOwner(_ owner: ReviewCollection) async throws {
        try #require(try await CoreCollectionService().lifecycle() == .noCollection)
        await owner.start()
        await owner.sync.waitForInputs()
        await owner.studyPreparation.waitForJob()
        await owner.waitForLifecycle()
        await owner.backupTrigger.drain()
        try #require(owner.state == .ready, "Real open failed: \(owner.error ?? String(describing: owner.state))")
        try #require(!owner.studyPreparation.running && !owner.sync.settings.enabled)
        #expect(try await CoreCollectionService().lifecycle() == .ready(generation: owner.generation))
    }

    private func addBackupRaceNote() async throws -> NoteMutation {
        let deck = try await CoreDeckService().addDeck(name: "Synthetic live successor")
        let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic backup race QA", cloze: false,
            fields: ["Question", "Answer"], templates: [.init(name: "Card", front: "{{Question}}", back: "{{Answer}}", browserFront: "", browserBack: "")], css: nil))
        let note = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Live question", "Live answer"],
            tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
        try #require(note.cards.count == 1)
        return note
    }

    private func cleanupBackupRaceOwner(_ owner: ReviewCollection) async {
        if !(await owner.close()) {
            Issue.record("Cleanup close refused: \(owner.error ?? "unknown error")")
            do { try await CoreCollectionService().close() } catch { Issue.record(error) }
        }
        do { #expect(try await CoreCollectionService().lifecycle() == .noCollection) }
        catch { Issue.record(error) }
    }

    private func backupRaceConfiguration(_ root: URL) throws -> LaunchConfiguration {
        // Same bundled database/package and sibling Backups layout as
        // CollectionEntryTests and BackupRecoveryTests; never a damaged import.
        let directory = root.appending(path: "Collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let fixture = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: fixture, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.deviceDefaults = nil
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
