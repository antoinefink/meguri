#if DEBUG
import Foundation
import Testing
@testable import CoreBridge

extension EventTests {
    @Test @MainActor func originalRoleRetainsActualEditAndRefusesUnboundOrSuccessorObservation() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "original-role-observation-\(UUID().uuidString)")
        let data = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CollectionLocation(collectionDirectory: data, backupsDirectory: backups,
                                          appGroupSnapshot: root.appending(path: "widget"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        let collection = CoreCollectionService()
        do {
            try await MaintenanceLane.shared.bootstrapCollection(location)
            try await collection.open(location, randomSeed: 329)
            let decks = try await CoreDeckService().allDeckNames().decks.map(\.id)
            let deletion = try await CoreDeckService().startDeleteDecks(
                ids: decks, confirmed: try await CoreDeckService().deckDeletionSummary(ids: decks))
            _ = try await deletion.value()
            _ = try await CoreDeckService().renameDeck(id: 1, name: "Generated-D1")
            let child = try await CoreDeckService().addDeck(name: "Generated-D1::Generated-D2")
            let type = try await CoreNotetypeService().addNotetype(.init(name: "QA", cloze: false,
                fields: ["A", "Q"], templates: [.init(name: "Face", front: "{{Q}}", back: "{{A}}",
                browserFront: "", browserBack: "")], css: nil))
            let note = try await CoreNoteService().addNote(.init(notetypeId: type.id, fields: ["Generated-A", "Generated-Q"],
                tags: ["retained"], deckId: child.id, guid: nil, updateAddDefaults: false))
            try await collection.flushForBackground()
            let main = try Data(contentsOf: data.appending(path: "collection.anki2"))
            let wal = try? Data(contentsOf: data.appending(path: "collection.anki2-wal"))
            let role = try await OriginalCloudRoleInspection.inspect(main: main, wal: wal)
            #expect(role.facts.qOrdinal == 1 && role.facts.fieldCount == 2)
            let before = try await role.validateOwned()
            #expect(!before.qChanged && before.noteDigest == role.facts.noteDigest)
            await #expect(throws: (any Error).self) { try await role.observe() }
            let edited = try await role.fixedEdit()
            #expect(edited.generation == before.generation && edited.seq > before.seq)
            #expect(edited.localRevision == nil && edited.source == .userUndoable && edited.updatedNotes == 1)
            let actual = try await role.validateOwned()
            #expect(actual.qChanged && actual.noteDigest == edited.noteDigest && actual.noteDigest.count == 32)
            #expect(actual.noteDigest != before.noteDigest)
            let row = try await CoreNoteService().readNote(id: note.id)
            #expect(row.fields == ["Generated-A", "Generated-sync-now-n1-Q"])
            await #expect(throws: (any Error).self) { try await role.fixedEdit() }
            #expect(try await CoreNoteService().readNote(id: note.id) == row)
            await #expect(throws: (any Error).self) { try await role.observe() }
            try await collection.close()
            try await collection.open(location, randomSeed: 330)
            // A readable successor row is not retained operation authority.
            #expect(try await role.validateOwned().noteDigest == edited.noteDigest)
            await #expect(throws: (any Error).self) { try await role.observe() }
            try await collection.close()
        } catch {
            try? await collection.close()
            throw error
        }
    }
}
#endif
