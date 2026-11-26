import CoreBridge
import Foundation
import Testing
@testable import AppModel

// Real operations share the serialized process-host suite. All data is synthetic.
extension ReviewCollectionStartupTests {
    @Test func nativeBackupsRestoreAnOlderCollectionKeepSafetyCopyAndSurviveReopen() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "native-backups-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let configuration = try backupConfiguration(root)
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() },
            defaults: nil, cloudConfigured: false)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"))
        owner.studyPreparation.runsUnprompted = false
        await owner.start()
        var restoredNoteID: Int64?
        do {
            await sync.waitForInputs()
            await owner.studyPreparation.waitForJob()
            await owner.waitForLifecycle()
            await owner.backupTrigger.drain()
            try #require(owner.state == .ready)
            // T3.2's process-wide attempt floor can defer Opened even for this
            // fresh folder. Always follow the app's scene lifecycle: Background
            // has no floor (T3.4), and neither trigger needs Settings to appear.
            let scene = UUID()
            #expect(!owner.updateSceneActivity(scene, active: true, background: false))
            try #require(owner.updateSceneActivity(scene, active: false, background: true))
            owner.setForeground(false)
            await owner.waitForLifecycle()
            #expect(!owner.foreground && owner.error == nil)
            #expect(!owner.updateSceneActivity(scene, active: true, background: false))
            await owner.waitForLifecycle()
            await owner.backupTrigger.drain()
            try #require(owner.foreground && !owner.resuming && owner.state == .ready)
            try #require(try await CoreCollectionService().lifecycle() == .ready(generation: owner.generation))
            let automatic = try await CoreMaintenanceService().status()
            let lastBackup = try #require(automatic.lastBackup)
            let automaticBackups = try await CoreMaintenanceService().listBackups()
            let archive = try #require(automaticBackups.first { $0.name == lastBackup.name })
            #expect(archive.kind == .backup && archive.sizeBytes > 0)
            let bytes = try Data(contentsOf: configuration.collectionLocation.backupsDirectory.appending(path: archive.name))
            #expect(!bytes.isEmpty && UInt64(bytes.count) == archive.sizeBytes)
            let beforeName = "Synthetic before backup"
            let afterName = "Synthetic after backup"
            let deck = try await CoreDeckService().addDeck(name: beforeName)
            let type = try await CoreNotetypeService().addNotetype(.init(name: "Synthetic backup QA", cloze: false,
                fields: ["Question", "Answer"], templates: [.init(name: "Card", front: "{{Question}}", back: "{{Answer}}", browserFront: "", browserBack: "")], css: nil))
            let note = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Earlier question", "Earlier answer"],
                tags: [], deckId: deck.id, guid: nil, updateAddDefaults: false))
            restoredNoteID = note.id
            let first = try #require(owner.backups.create())
            await first.value
            let original = try #require(owner.backups.created)
            _ = try await CoreDeckService().addDeck(name: afterName)
            _ = try await CoreNoteService().updateNote(.init(id: note.id, fields: ["Later question", "Later answer"], tags: []))
            let second = try #require(owner.backups.create())
            await second.value
            let later = try #require(owner.backups.created)
            #expect(original.name != later.name)
            owner.presentBackupRestore()
            await owner.backups.openedRestore()
            owner.backups.selectedName = original.name
            let restored = try #require(owner.backups.restoreConfirmed(name: original.name))
            await restored.value
            try #require(owner.backups.restoreCompleted)
            #expect(owner.navigation.sheet == nil && owner.navigation.section == .home)
            #expect(try await CoreDeckService().matchDeckName(name: beforeName).deck != nil)
            #expect(try await CoreDeckService().matchDeckName(name: afterName).deck == nil)
            #expect(try await CoreNoteService().readNote(id: note.id).fields == ["Earlier question", "Earlier answer"])
            let safety = try #require(owner.backups.safetyBackup)
            let listed = try await CoreMaintenanceService().listBackups()
            #expect(listed.contains { $0.name == safety })
            // Read the predecessor through a real restore, proving its contents.
            owner.presentBackupRestore(); await owner.backups.openedRestore()
            owner.backups.selectedName = safety
            let restoreSafety = try #require(owner.backups.restoreConfirmed(name: safety))
            await restoreSafety.value
            try #require(owner.backups.restoreCompleted)
            #expect(try await CoreDeckService().matchDeckName(name: afterName).deck != nil)
            #expect(try await CoreNoteService().readNote(id: note.id).fields == ["Later question", "Later answer"])
            owner.presentBackupRestore(); await owner.backups.openedRestore()
            owner.backups.selectedName = original.name
            let again = try #require(owner.backups.restoreConfirmed(name: original.name))
            await again.value
            try #require(owner.backups.restoreCompleted)
            try #require(await owner.close())
        } catch { _ = await owner.close(); throw error }
        let reopened = backupOwner(configuration, root: root)
        await reopened.start()
        do {
            try #require(reopened.state == .ready)
            #expect(try await CoreDeckService().matchDeckName(name: "Synthetic before backup").deck != nil)
            #expect(try await CoreDeckService().matchDeckName(name: "Synthetic after backup").deck == nil)
            let noteID = try #require(restoredNoteID)
            #expect(try await CoreNoteService().readNote(id: noteID).fields == ["Earlier question", "Earlier answer"])
            try #require(await reopened.close())
        } catch { _ = await reopened.close(); throw error }
    }

    @Test func coldDamagedCollectionRejectsCandidateThenRestoresOfflineAndPreservesDamagedBytes() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-cold-recovery-\(UUID().uuidString)")
        let files = FileManager.default
        defer { try? files.removeItem(at: root) }
        let configuration = try backupConfiguration(root)
        let initial = backupOwner(configuration, root: root)
        await initial.start()
        let savedName: String
        do {
            try #require(initial.state == .ready)
            _ = try await CoreDeckService().addDeck(name: "Synthetic recovered deck")
            let create = try #require(initial.backups.create())
            await create.value
            savedName = try #require(initial.backups.created).name
            try #require(await initial.close())
        } catch { _ = await initial.close(); throw error }
        let damaged = Data("Synthetic unreadable collection; preserve these bytes".utf8)
        try damaged.write(to: configuration.collectionLocation.collectionFile)
        let rejectedName = "backup-2099-01-01-00.00.00.colpkg"
        try Data("Synthetic invalid package".utf8).write(to: configuration.collectionLocation.backupsDirectory.appending(path: rejectedName))
        let recovery = backupOwner(configuration, root: root)
        await recovery.start()
        do {
            guard case .failed = recovery.state else { Issue.record("Cold damaged file must fail its ordinary open"); _ = await recovery.close(); return }
            recovery.presentBackupRestore()
            await recovery.backups.openedRestore() // Establishes the retained offline host first.
            try #require(recovery.backups.entries.contains { $0.name == savedName })
            recovery.backups.selectedName = rejectedName
            let refused = try #require(recovery.backups.restoreConfirmed(name: rejectedName))
            await refused.value
            #expect(recovery.navigation.sheet == .backupRestore)
            #expect(recovery.backups.canOpenRestore && !recovery.backups.candidateActive)
            #expect(try Data(contentsOf: configuration.collectionLocation.collectionFile) == damaged)
            recovery.backups.selectedName = savedName
            let restore = try #require(recovery.backups.restoreConfirmed(name: savedName))
            await restore.value
            try #require(recovery.backups.restoreCompleted)
            #expect(try await CoreDeckService().matchDeckName(name: "Synthetic recovered deck").deck != nil)
            let copies = try files.contentsOfDirectory(at: configuration.collectionLocation.backupsDirectory,
                includingPropertiesForKeys: nil).filter { $0.lastPathComponent.hasPrefix("corrupt-") && $0.pathExtension == "anki2" }
            try #require(copies.count == 1)
            #expect(try Data(contentsOf: copies[0]) == damaged)
            try #require(await recovery.close())
        } catch { _ = await recovery.close(); throw error }
    }

    @Test(arguments: [false, true])
    func earlyBackupRestoreRefusalRejoinsTheUnchangedArmedDeadline(cancelBeforeInspection: Bool) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-refusal-timer-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = try backupConfiguration(root)
        let now = Date(timeIntervalSince1970: 1_894_701_600)
        configuration.clock = .fixed(now)
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let invalid = "backup-2030-01-15-11.59.00.colpkg"
        let directory = configuration.collectionLocation.backupsDirectory
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let candidate = directory.appending(path: invalid)
        try Data("Synthetic invalid backup".utf8).write(to: candidate)
        // T2.2 counts every entry's creation time, including corrupt packages.
        // Seed before opening so rejection cannot introduce a newer interval.
        let created = now.addingTimeInterval(-60)
        try FileManager.default.setAttributes([.creationDate: created, .modificationDate: created],
            ofItemAtPath: candidate.path(percentEncoded: false))
        let attributes = try FileManager.default.attributesOfItem(atPath: candidate.path(percentEncoded: false))
        try #require((attributes[.creationDate] as? Date) == created)
        let owner = backupOwner(configuration, root: root)
        await owner.start()
        do {
            try #require(owner.state == .ready)
            _ = try await CoreDeckService().addDeck(name: "Synthetic pending backup")
            await owner.backupTrigger.run(.opened)
            let armed = try #require(owner.backupTrigger.deadlineMilliseconds)
            #expect(armed == Int64(now.timeIntervalSince1970 * 1_000) + 29 * 60_000)
            if cancelBeforeInspection {
                try await owner.beginBackupRestore()
                #expect(owner.backupTrigger.deadlineMilliseconds == nil)
                _ = await owner.finishBackupRestore(candidateActive: false, generation: nil)
            } else {
                await owner.backups.openedRestore()
                owner.backups.selectedName = invalid
                let restore = try #require(owner.backups.restoreConfirmed(name: invalid))
                await restore.value
                #expect(!owner.backups.candidateActive && owner.backups.failure != nil)
            }
            #expect(owner.state == .ready)
            #expect(owner.backupTrigger.deadlineMilliseconds == armed)
            await owner.backupTrigger.run(.deadline) // An early timer keeps the same future deadline.
            #expect(owner.backupTrigger.deadlineMilliseconds == armed)
            try #require(await owner.close())
        } catch { _ = await owner.close(); throw error }
    }

    @Test(.timeLimit(.minutes(1)))
    func coldDamagedCollectionListingCancelQuitsNormallyAndColdReopensWithoutChangingBytes() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-cancel-damaged-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let configuration = try backupConfiguration(root)
        let damaged = Data("Synthetic invalid collection retained across recovery cancellation".utf8)
        try damaged.write(to: configuration.collectionLocation.collectionFile)
        for _ in 0..<2 {
            let owner = backupOwner(configuration, root: root)
            await owner.start()
            do {
                guard case .failed = owner.state else { throw BridgeError.internal("Expected damaged cold open") }
                owner.presentBackupRestore()
                await owner.backups.openedRestore()
                // Listing retains a real Failed host even with no backup selected.
                guard case .failed = try await CoreCollectionService().lifecycle() else {
                    throw BridgeError.internal("Expected retained failed recovery host")
                }
                owner.navigation.dismissSheet()
                let began = ContinuousClock.now
                try #require(await owner.closeForQuit())
                #expect(began.duration(to: .now) < .seconds(5))
                #expect(try Data(contentsOf: configuration.collectionLocation.collectionFile) == damaged)
            } catch { _ = await owner.close(); throw error }
        }
    }

    @Test func uncertainBackupActivationKeepsRecoveryAfterDismissalAndAllowsDrainedClose() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-uncertain-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        let configuration = try backupConfiguration(root)
        let owner = backupOwner(configuration, root: root)
        await owner.start()
        do {
            try #require(owner.state == .ready)
            try await owner.beginBackupRestore()
            // Lose the host before activation facts can be delivered. This is
            // real lifecycle evidence, not a fabricated terminal restore result.
            try await CoreCollectionService().close()
            let completion = await owner.finishBackupRestore(candidateActive: nil, generation: nil)
            #expect(completion.candidateActive == nil && !completion.ready)
            owner.navigation.dismissSheet()
            owner.receiveLifecycle(.ready(generation: owner.generation)) // Late delivery cannot reopen input.
            #expect(owner.backupRecoveryRequired && owner.homePhase == .unavailable)
            #expect(!owner.backups.canCreate && !owner.backups.canOpenRestore)
            #expect(!owner.entryAllowed && owner.backupTrigger.deadlineMilliseconds == nil)
            try #require(await owner.closeForQuit())
        } catch { _ = await owner.close(); throw error }
        let reopened = backupOwner(configuration, root: root)
        await reopened.start()
        #expect(reopened.state == .ready && !reopened.backupRecoveryRequired)
        #expect(await reopened.close())
    }

    @Test(.timeLimit(.minutes(1)), arguments: [false, true])
    func automaticBackupCancellationLetsBackgroundSuspendOrQuitRejoin(quitting: Bool) async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "backup-lifecycle-cancel-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: root) }
        var configuration = try backupConfiguration(root)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        // Ready publishes the engine's initial deadline independently of the
        // probe. Seed T2.3's persisted reference so the real deadline is nil.
        let initialSync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() },
            defaults: nil, cloudConfigured: false)
        let initial = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: initialSync, providerDirectory: root.appending(path: "synthetic-installation"))
        initial.studyPreparation.runsUnprompted = false
        await initial.start()
        do {
            await initialSync.waitForInputs()
            await initial.studyPreparation.waitForJob()
            await initial.waitForLifecycle()
            await initial.backupTrigger.drain()
            try #require(initial.state == .ready)
            let create = try #require(initial.backups.create())
            await create.value
            let receipt = try #require(initial.backups.created)
            try #require(receipt.sizeBytes > 0)
            try #require(initial.backups.entries.contains { $0.name == receipt.name && $0.sizeBytes == receipt.sizeBytes })
            try #require(await initial.close())
        } catch { _ = await initial.close(); throw error }
        let service = BackupServiceProbe()
        // Off has no cloud policy to acknowledge Quit. Keep startup inert:
        // sync and study-preparation writes have their own regression suites.
        let backend = SyncOwnerFake()
        let sync = SyncCoordinator(bootstrap: {}, makeBackend: { _ in backend },
            defaults: nil, cloudConfigured: false)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"), maintenanceService: service)
        owner.studyPreparation.runsUnprompted = false
        await owner.start()
        do {
            // Join actual setup before taking the baseline. With no preparatory
            // writes there is no later Ready transition waiting on a real job.
            await sync.waitForInputs()
            await owner.studyPreparation.waitForJob()
            await owner.waitForLifecycle()
            await owner.backupTrigger.drain()
            let startupState = owner.state
            let startupLifecycle = try await CoreCollectionService().lifecycle()
            let startupTriggers = await service.triggers
            let startupDeadline = owner.backupTrigger.deadlineMilliseconds
            let startupDecks = try await CoreDeckService().allDeckNames()
            try #require(startupState == .ready)
            #expect(startupLifecycle == .ready(generation: owner.generation))
            #expect(!owner.studyPreparation.running && !sync.settings.enabled)
            #expect(!startupTriggers.isEmpty)
            #expect(startupTriggers.allSatisfy { $0 == .opened })
            #expect(startupDeadline == nil)
            let baseline = startupTriggers.count

            await service.configureDue(deadline: nil, held: true)
            var automaticEnded = false
            let automatic = Task { await owner.backupTrigger.run(.deadline); automaticEnded = true }
            var quit: Task<Bool, Never>?
            var quitEnded = false
            let began = ContinuousClock.now
            let lease: UInt64?
            let deadlineOperation: MaintenanceOperation
            do {
                try await service.waitForStart()
                deadlineOperation = try #require(await service.dueOperation)
                let heldTriggers = await service.triggers
                #expect(Array(heldTriggers.dropFirst(baseline)) == [.deadline])
                if quitting {
                    lease = nil
                    quit = Task {
                        let result = await owner.closeForQuit()
                        quitEnded = true
                        return result
                    }
                } else {
                    // Expire synchronously, before the queued background path can dispatch.
                    lease = owner.beginBackgroundTask(deadlineMilliseconds: 123_000)
                    owner.backgroundTaskExpired(try #require(lease))
                }
                try await service.waitForOperationCancellation()
                let cancellingOperation = await service.dueOperation
                let cancellingTriggers = await service.triggers
                let cancellingLifecycle = try await CoreCollectionService().lifecycle()
                // This read requires the live collection: cancellation must not
                // suspend or close it while the service still owns the result.
                let cancellingDecks = try await CoreDeckService().allDeckNames()
                #expect(cancellingOperation === deadlineOperation)
                #expect(Array(cancellingTriggers.dropFirst(baseline)) == [.deadline])
                #expect(cancellingLifecycle == startupLifecycle)
                #expect(cancellingDecks == startupDecks)
                #expect(!automaticEnded && !quitEnded && !backend.closed)
            } catch {
                await service.release(); await automatic.value; _ = await quit?.value
                throw error
            }
            await service.release(); await automatic.value
            #expect(automaticEnded)
            if let quit {
                let closed = await quit.value
                let quitDuration = began.duration(to: .now)
                let quitTriggers = await service.triggers
                let quitOperation = try #require(await service.dueOperation)
                let closedLifecycle = try await CoreCollectionService().lifecycle()
                try #require(closed)
                #expect(quitEnded && quitDuration < .seconds(5))
                #expect(Array(quitTriggers.dropFirst(baseline)) == [.deadline, .quit])
                #expect(quitOperation !== deadlineOperation)
                #expect(closedLifecycle == .noCollection)
            } else {
                await owner.waitForLifecycle()
                let backgroundTriggers = await service.triggers
                // The expired lease dispatches no Background maintenance.
                #expect(Array(backgroundTriggers.dropFirst(baseline)) == [.deadline])
                owner.setForeground(true)
                await owner.waitForLifecycle()
                await owner.backupTrigger.drain()
                let resumedTriggers = await service.triggers
                let resumed = Array(resumedTriggers.dropFirst(backgroundTriggers.count))
                // Resume may also deliver Ready; every resulting call is Opened.
                #expect(!resumed.isEmpty && resumed.allSatisfy { $0 == .opened })
                await service.configureDue(deadline: nil, held: true)
                let foreground = Task { await owner.backupTrigger.run(.opened) }
                do {
                    try await service.waitForStart()
                    let foregroundOperation = try #require(await service.dueOperation)
                    #expect(foregroundOperation !== deadlineOperation)
                    owner.backgroundTaskExpired(try #require(lease))
                    try await service.requireOperationNotCancelled()
                    let afterStaleExpiration = await service.dueOperation
                    #expect(afterStaleExpiration === foregroundOperation)
                } catch {
                    await service.release(); await foreground.value
                    throw error
                }
                await service.release(); await foreground.value
                try await service.requireOperationNotCancelled()
                let foregroundTriggers = await service.triggers
                #expect(Array(foregroundTriggers.dropFirst(resumedTriggers.count)) == [.opened])
                let closed = await owner.close()
                let finalTriggers = await service.triggers
                let closedLifecycle = try await CoreCollectionService().lifecycle()
                try #require(closed)
                #expect(Array(finalTriggers.dropFirst(foregroundTriggers.count)) == [.background])
                #expect(closedLifecycle == .noCollection)
            }
        } catch {
            await service.release()
            _ = await owner.close(); throw error
        }
    }

    @Test func anotherForegroundSceneRetainsTheSharedCollectionLifetime() {
        let configuration = LaunchConfiguration()
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let first = UUID(), second = UUID()
        #expect(!owner.updateSceneActivity(first, active: true, background: false))
        #expect(!owner.updateSceneActivity(second, active: true, background: false))
        #expect(!owner.updateSceneActivity(first, active: false, background: true))
        #expect(!owner.removeSceneActivity(first))
        #expect(owner.foreground)
        #expect(owner.updateSceneActivity(second, active: false, background: true))
    }

    private func backupConfiguration(_ root: URL) throws -> LaunchConfiguration {
        let directory = root.appending(path: "Collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let fixture = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        try FileManager.default.copyItem(at: fixture, to: directory.appending(path: "collection.anki2"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        return configuration
    }
    private func backupOwner(_ configuration: LaunchConfiguration, root: URL) -> ReviewCollection {
        ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: SyncCoordinator(bootstrap: {}, makeBackend: { _ in SyncOwnerFake() }, defaults: nil),
            providerDirectory: root.appending(path: "synthetic-installation"))
    }
}
