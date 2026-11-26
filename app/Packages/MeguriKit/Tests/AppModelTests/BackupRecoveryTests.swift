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
