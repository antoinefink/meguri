import Foundation
import Dispatch
import Synchronization
import Testing
import MeguriCoreBindings
@testable import CoreBridge

private final class MutationClock: CollectionClock {
    let onMain = Mutex<[Bool]>([])
    func nowMilliseconds() -> Int64 { onMain.withLock { $0.append(Thread.isMainThread) }; return 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { onMain.withLock { $0.append(Thread.isMainThread) }; return 0 }
    func zoneIdentifier() -> String { onMain.withLock { $0.append(Thread.isMainThread) }; return "UTC" }
}

extension EventTests {
    @Test @MainActor func typedMutationsUndoMediaAndControlUseTheOpenedHostOffMain() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let collection = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: collection, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: collection, backupsDirectory: backups,
                                                     appGroupSnapshot: root.appending(path: "widget.snapshot"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let clock = MutationClock()
        let collectionService = CoreCollectionService(clock: clock)
        let decks = CoreDeckService(), notes = CoreNoteService(), types = CoreNotetypeService()
        let tags = CoreTagService(), undo = CoreUndoService(), media = CoreMediaService()
        do {
            try await collectionService.open(location, randomSeed: 37)
            let generation = try await collectionService.waitAvailable(timeoutMilliseconds: 0)
            let deck = try await decks.addDeck(name: "Mutation services")
            #expect(deck.summary.generation == generation)
            #expect(deck.summary.undoName == "Add Deck")
            #expect(deck.summary.changes.decks.contains { $0.id == deck.id && $0.change == .added })
            let renaming = try await decks.startRenameDeck(id: deck.id, name: "Mutation services renamed")
            // The starter returns a job; an interactive status call can run
            // before its value is collected on the maintenance lane.
            _ = try await undo.peekUndo(scope: .global)
            #expect(try await renaming.value().id == deck.id)
            let moving = try await decks.startMoveDecks(ids: [deck.id], parent: nil)
            #expect(try await moving.value().count == 0)
            let collapsing = try await decks.startCollapseDecks(ids: [deck.id], scope: .browser, collapsed: false)
            #expect(try await collapsing.value().generation == generation)
            let nt = try await types.addStockNotetype(kind: .basic, name: "Service basic")
            let original = try await types.readNotetype(id: nt.id)
            let styled = try await types.setNotetypeCss(id: nt.id, base: original.base, css: ".card { color: green; }")
            #expect(styled.summary.seq > nt.summary.seq)
            await #expect(throws: BridgeError.stale(table: .notetypes, key: String(nt.id))) {
                try await types.renameNotetype(id: nt.id, base: original.base, name: "Stale edit")
            }
            let note = try await notes.addNote(.init(notetypeId: nt.id, fields: ["front", "back"], tags: ["Parent::Child"],
                                                    deckId: deck.id, guid: "service-guid", updateAddDefaults: false))
            #expect(note.summary.generation == generation)
            #expect(note.summary.changes.notes.contains { $0.id == note.id })
            #expect(note.cards.count == 1)
            #expect(try await notes.checkNoteFields(notetypeId: nt.id, fields: ["front", "back"], ownNote: nil).duplicates == [note.id])
            let saved = try await notes.updateNote(.init(id: note.id, fields: ["persisted", "back"], tags: ["Parent::Child"]))
            #expect(saved.written)
            #expect(try await tags.markNotes(notes: [note.id]).count == 1)
            #expect(try await notes.readNote(id: note.id).tags.contains("marked"))
            #expect(try await tags.toggleNoteMark(current: note.id, notes: [note.id]).count == 1)
            let flagJob = try await tags.startSetCardFlag(cards: Array(repeating: note.cards[0], count: 2_001), flag: 5)
            #expect(try await flagJob.value().count == 1)
            guard case .available(let step) = try await undo.peekUndo(scope: .global) else {
                Issue.record("flag undo missing"); try await collectionService.close(); return
            }
            #expect(step.longWrite != nil)
            let restoring = try await undo.startUndo(scope: .global)
            guard case .global(let summary, let restored) = try await restoring.value() else {
                Issue.record("global undo routed to session"); try await collectionService.close(); return
            }
            #expect(restored.id == step.id && restored.longWrite == step.longWrite)
            #expect(summary.generation == generation)
            let redoing = try await undo.startRedo(scope: .global)
            _ = try await redoing.value()
            let renameTags = try await tags.startRenameTag(from: "Parent", to: "Renamed")
            #expect(try await renameTags.value().count == 1)
            #expect(try await notes.readNote(id: note.id).tags.contains("Renamed::Child"))
            let safe = try await media.addMedia(name: "Attachment.PNG", bytes: [1,2,3])
            #expect(safe == "attachment.png")
            #expect(try await media.addMedia(name: "ATTACHMENT.PNG", bytes: [1,2,3]) == safe)
            let beforeDelete = try await decks.deckDeletionSummary(ids: [deck.id])
            let refused = try await decks.startDeleteDecks(ids: [deck.id])
            guard case .failed(.unavailable) = await refused.job.outcome() else {
                Issue.record("card deletion must refuse until U127 is wired"); try await collectionService.close(); return
            }
            #expect(try await decks.deckDeletionSummary(ids: [deck.id]) == beforeDelete)
            try await collectionService.flushForBackground()
            #expect(try await collectionService.suspend(timeoutMilliseconds: 5_000) == .suspended)
            let resumed = try await collectionService.resume()
            #expect(resumed.activeSession == nil)
            #expect(!resumed.externalChange)
            try await collectionService.close()
            try await collectionService.open(location, randomSeed: 37)
            #expect(try await notes.readNote(id: note.id).fields == ["persisted", "back"])
            #expect(try await types.readNotetype(id: nt.id).css == ".card { color: green; }")
            #expect(try await media.addMedia(name: "Attachment.PNG", bytes: [1,2,3]) == safe)
            let deletion = try await notes.startDeleteNotes(ids: [note.id])
            #expect(try await deletion.value().count == 1)
            await #expect(throws: BridgeError.noteNotFound(id: note.id)) { try await notes.readNote(id: note.id) }
            let missing = try await notes.startDeleteNotes(ids: [note.id])
            let noOp = try await missing.value()
            #expect(noOp.count == 0 && noOp.summary == nil)
            let lastGeneration = try await collectionService.waitAvailable(timeoutMilliseconds: 0)
            try await collectionService.close()
            for await event in CoreEventStream.shared.events {
                if event.generation == lastGeneration && event.kind == .lifecycle(.noCollection) { break }
            }
            let mainCalls = clock.onMain.withLock { $0 }
            #expect(!mainCalls.isEmpty && mainCalls.allSatisfy { !$0 })
        } catch {
            try? await collectionService.close()
            throw error
        }
    }
}
