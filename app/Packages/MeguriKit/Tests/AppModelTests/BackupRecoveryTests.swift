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
        let owner = backupOwner(configuration, root: root)
        await owner.start()
        var restoredNoteID: Int64?
        do {
            try #require(owner.state == .ready)
            // Opened is a real native automatic trigger, without Settings appearing.
            let automatic = try await CoreMaintenanceService().status()
            #expect(automatic.lastBackup != nil)
            #expect(!(try await CoreMaintenanceService().listBackups()).isEmpty)
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
        let configuration = try backupConfiguration(root)
        let service = BackupServiceProbe()
        // Off has no cloud policy to acknowledge Quit. The configured-policy
        // refusal and real quit-command ACK paths belong to the shutdown suite.
        let sync = SyncCoordinator(directory: configuration.collectionLocation.directory,
            configuration: nil, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(),
            sync: sync,
            providerDirectory: root.appending(path: "synthetic-installation"), maintenanceService: service)
        // Force study preparation's real Ready transition to follow the initial
        // Opened call, rather than racing it while opening suppresses renewal.
        let preparationGate = SyncOwnerGate()
        var preparation = try #require(owner.studyPreparation.environment)
        let startPreparation = preparation.start
        preparation.start = {
            await preparationGate.hold()
            return try await startPreparation()
        }
        owner.studyPreparation.environment = preparation
        let startup = Task { await owner.start() }
        do {
            await preparationGate.waitUntilHeld()
            let openLimit = ContinuousClock.now.advanced(by: .seconds(4))
            while !owner.deckActionsReady, ContinuousClock.now < openLimit {
                try await Task.sleep(for: .milliseconds(1))
            }
            try #require(owner.deckActionsReady)
            #expect(await service.triggers == [.opened])
            preparationGate.release()
            await startup.value
            let renewalLimit = ContinuousClock.now.advanced(by: .seconds(4))
            while await service.triggers.count < 2, ContinuousClock.now < renewalLimit {
                try await Task.sleep(for: .milliseconds(1))
            }
            await owner.backupTrigger.drain()
            try #require(owner.state == .ready)
            #expect(sync.providerState?.selection == .localOnly)
            #expect(await service.triggers == [.opened, .opened])
            await service.configureDue(deadline: nil, held: true)
            var automaticEnded = false
            let automatic = Task { await owner.backupTrigger.run(.deadline); automaticEnded = true }
            await service.waitForStart()
            var quit: Task<Bool, Never>?
            var quitEnded = false
            let lease: UInt64?
            if quitting {
                lease = nil
                quit = Task {
                    let result = await owner.closeForQuit()
                    quitEnded = true
                    return result
                }
            } else {
                // Keep the collection's UInt64 lease token for both expiries.
                let token: UInt64? = owner.beginBackgroundTask(deadlineMilliseconds: 123_000)
                lease = token
                owner.backgroundTaskExpired(try #require(lease))
            }
            do { try await service.waitForOperationCancellation() }
            catch { await service.release(); await automatic.value; _ = await quit?.value; throw error }
            #expect(!automaticEnded && !quitEnded)
            await service.release(); await automatic.value
            #expect(automaticEnded)
            if let quit {
                try #require(await quit.value)
                #expect(quitEnded)
                #expect(await service.triggers == [.opened, .opened, .deadline, .quit])
            }
            else {
                await owner.waitForLifecycle()
                // Both genuine startup Opened events count; expiry adds no Background.
                #expect(await service.triggers == [.opened, .opened, .deadline])
                owner.setForeground(true)
                await owner.waitForLifecycle()
                #expect(await service.triggers == [.opened, .opened, .deadline, .opened])
                await service.configureDue(deadline: nil, held: true)
                let foreground = Task { await owner.backupTrigger.run(.opened) }
                await service.waitForStart()
                owner.backgroundTaskExpired(try #require(lease)) // Stale expiry must not cancel new work.
                try await service.requireOperationNotCancelled()
                await service.release(); await foreground.value
                #expect(await service.triggers == [.opened, .opened, .deadline, .opened, .opened])
                try #require(await owner.close())
            }
        } catch {
            preparationGate.release(); await service.release(); await startup.value
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
