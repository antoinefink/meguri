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

private final class MutationDiskCapacity: DiskSpaceProviding {
    let bytes = Mutex<UInt64>(.max)
    func availableBytes(at directory: URL) throws -> UInt64 { bytes.withLock { $0 } }
}

extension EventTests {
    @Test @MainActor func guardedAddUsesCapturedGenerationAndRealNoteWriter() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appending(path: "backups"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService(clock: MutationClock(), diskSpace: MutationDiskCapacity())
        let notes = CoreNoteService(), types = CoreNotetypeService(), undo = CoreUndoService()
        do {
            try await service.open(location, randomSeed: 83)
            if try await Core.fsrsPolicyStatus() != .ok {
                let admission = try await Core.enableFsrs()
                try await admission.value()
            }
            let generation = try await service.waitAvailable(timeoutMilliseconds: 0)
            let type = try await types.addStockNotetype(kind: .basic, name: "Guarded Add bridge")
            let snapshot = try await types.readNotetype(id: type.id)
            let deck = try await CoreDeckService().addDeck(name: "Guarded Add destination")
            let captured = CoreBridge.AddScreenNoteRequest(expectedGeneration: generation, notetype: snapshot,
                fields: ["<b>captured front</b>", "back"], tags: ["GuardedAdd"], deckId: deck.id)
            let saved = try await notes.addNoteForScreen(captured)
            #expect(saved.summary.generation == generation && saved.summary.undoName == "Add Note")
            #expect(saved.cards.count == 1 && saved.written)
            let row = try await notes.readNote(id: saved.id)
            #expect(row.notetypeId == snapshot.id && row.fields == captured.fields)
            #expect(saved.summary.changes.notes.contains { $0.id == row.id && $0.change == .added })
            #expect(saved.summary.changes.cards.contains { saved.cards.contains($0.id) && $0.change == .added })
            let undoing = try await undo.startUndo(scope: .global)
            _ = try await undoing.value()
            await #expect(throws: BridgeError.noteNotFound(id: row.id)) { try await notes.readNote(id: row.id) }
            let redoing = try await undo.startRedo(scope: .global)
            _ = try await redoing.value()
            #expect(try await notes.readNote(id: row.id) == row)
            try await service.close()
            try await service.open(location, randomSeed: 83)
            let successor = try await service.waitAvailable(timeoutMilliseconds: 0)
            #expect(successor != generation)
            // The ids/layout remain valid across reopen. An FFI implementation
            // that reacquires the successor and forgets the capture would add.
            let priorUndo = try await undo.peekUndo(scope: .global)
            await #expect(throws: BridgeError.staleGeneration(planned: generation, current: successor)) {
                try await notes.addNoteForScreen(captured)
            }
            #expect(try await notes.readNote(id: row.id) == row)
            #expect(try await undo.peekUndo(scope: .global) == priorUndo)
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }

    @Test @MainActor func typedMutationsUndoMediaAndControlUseTheOpenedHostOffMain() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let collection = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: collection, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: collection, backupsDirectory: backups,
                                                     appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let clock = MutationClock()
        let capacity = MutationDiskCapacity()
        let collectionService = CoreCollectionService(clock: clock, diskSpace: capacity)
        let decks = CoreDeckService(), notes = CoreNoteService(), types = CoreNotetypeService()
        let tags = CoreTagService(), undo = CoreUndoService(), media = CoreMediaService()
        do {
            try await collectionService.open(location, randomSeed: 37)
            if try await Core.fsrsPolicyStatus() != .ok {
                let admission = try await Core.enableFsrs()
                try await admission.value()
            }
            try #require(try await Core.fsrsPolicyStatus() == .ok)
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
            let scheduling = CoreSchedOpsService()
            let suspending = try await scheduling.start(.suspend(.cards(note.cards)), sessionID: nil)
            let suspended = try await suspending.value()
            #expect(suspended.count == 1 && suspended.summary.undoName == "Suspend")
            #expect(suspended.summary.generation == generation && suspended.headDiff == nil)
            let unsuspendingUndo = try await undo.startUndo(scope: .global)
            guard case .global(_, let scheduledStep) = try await unsuspendingUndo.value() else {
                Issue.record("Scheduling undo did not use the host"); try await collectionService.close(); return
            }
            #expect(scheduledStep.name == "Suspend" && scheduledStep.kind == .other)
            #expect(try await scheduling.buriedKinds(deck: deck.id) == CoreBridge.BuriedKinds(manual: false, siblings: false))
            #expect(try await notes.checkNoteFields(notetypeId: nt.id, fields: ["front", "back"], ownNote: nil).duplicates == [note.id])
            let saved = try await notes.updateNote(.init(id: note.id, fields: ["persisted", "back"], tags: ["Parent::Child"]))
            #expect(saved.written)
            // editor/field-html S2: the patch writes only its fields and refuses a deleted note.
            let loaded = try await types.readNotetype(id: nt.id)
            let patched = try await notes.patchNote(.init(id: note.id, notetype: loaded, fields: [1: "patched"]))
            #expect(patched.written && patched.summary.undoName == "Update Note")
            #expect(try await notes.readNote(id: note.id).fields == ["persisted", "patched"])
            #expect(try await notes.patchNote(.init(id: note.id, notetype: loaded, fields: [1: "patched"])).written == false)
            await #expect(throws: BridgeError.noteNotFound(id: -5)) {
                try await notes.patchNote(.init(id: -5, notetype: loaded, fields: [0: "x"]))
            }
            #expect(try await notes.cardDeck(cardId: note.cards[0]) == deck.id)
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
            let noteBeforeDelete = try await notes.readNote(id: note.id)
            do {
                capacity.bytes.withLock { $0 = 0 }
                defer { capacity.bytes.withLock { $0 = .max } }
                let refused = try await decks.startDeleteDecks(ids: [deck.id])
                let outcome = await refused.job.outcome()
                guard case .failed(.internal(let detail)) = outcome else {
                    Issue.record("Expected deletion to stop at the real safety backup: \(outcome)")
                    try await collectionService.close(); return
                }
                #expect(detail.hasPrefix("safety backup failed: "))
                #expect(detail.contains("InsufficientSpace"))
                #expect(detail.contains("available: 0"))
                #expect(detail.contains("Unpublished"))
                await #expect(throws: BridgeError.internal(detail)) { try await refused.value() }
            }
            #expect(try await decks.deckDeletionSummary(ids: [deck.id]) == beforeDelete)
            #expect(try await notes.readNote(id: note.id) == noteBeforeDelete)

            // The successful deletion must publish its own populated pre-delete backup.
            let generationBeforeDelete = try await collectionService.waitAvailable(timeoutMilliseconds: 0)
            #expect(generationBeforeDelete == generation)
            #expect(beforeDelete == CoreBridge.DeckDeletionSummary(decks: 1, cards: 1))
            let treeBeforeDelete = try await decks.treeWithCounts()
            let deckBeforeDelete = try #require(treeBeforeDelete.nodes.first { $0.id == deck.id })
            #expect(deckBeforeDelete.name == "Mutation services renamed")
            #expect(deckBeforeDelete.counts == CoreBridge.ReviewCounts(new: 1, learning: 0, review: 0))
            let excerptsBeforeDelete = try await Core.cardExcerpts(note.cards)
            guard case .card(let cardBeforeDelete) = try await Core.peekFirstCard(deckID: deck.id) else {
                Issue.record("Populated deck has no pre-delete card witness")
                try await collectionService.close(); return
            }
            #expect(cardBeforeDelete.cardId == note.cards[0] && cardBeforeDelete.noteId == note.id)
            #expect(cardBeforeDelete.flag == 5)
            let backupNamesBeforeDelete = Set(try FileManager.default.contentsOfDirectory(atPath: backups.path))
            let deletingDeck = try await decks.startDeleteDecks(ids: [deck.id])
            let deletedDeck = try await deletingDeck.value()
            #expect(deletedDeck.deleted == beforeDelete)
            #expect(deletedDeck.summary.generation == generationBeforeDelete)
            #expect(deletedDeck.summary.seq > saved.summary.seq)
            #expect(deletedDeck.summary.changes.decks.filter { $0.change == .removed } == [
                .init(id: deck.id, change: .removed)
            ])
            #expect(deletedDeck.summary.changes.cards == [
                .init(id: cardBeforeDelete.cardId, noteID: note.id, change: .removed, memoryStateOnly: false)
            ])
            #expect(deletedDeck.summary.changes.notes == [.init(id: note.id, change: .removed)])
            #expect(try await decks.treeWithCounts().nodes.allSatisfy { $0.id != deck.id })
            #expect(try await Core.cardExcerpts(note.cards) == [.missing(cardID: cardBeforeDelete.cardId)])
            await #expect(throws: BridgeError.noteNotFound(id: note.id)) { try await notes.readNote(id: note.id) }

            let backupNamesAfterDelete = Set(try FileManager.default.contentsOfDirectory(atPath: backups.path))
            let newBackupNames = backupNamesAfterDelete.subtracting(backupNamesBeforeDelete)
            try #require(newBackupNames.count == 1)
            let backupName = try #require(newBackupNames.first)
            let backupURL = backups.appending(path: backupName)
            #expect(backupURL.pathExtension == "colpkg")
            let backupFile = try backupURL.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
            try #require(backupFile.isRegularFile == true && (backupFile.fileSize ?? 0) > 0)
            let entryService = CoreCollectionEntryService(clock: clock, diskSpace: capacity)
            let inspection = try await entryService.inspect(source: backupURL, location: location,
                operation: CollectionEntryOperation(), progress: { _ in })
            let activation: CoreBridge.CollectionActivation
            do {
                try #require(inspection.facts.notes == 1 && inspection.facts.cards == UInt64(note.cards.count))
                #expect(inspection.facts.replacingExisting)
                activation = try await entryService.activate(inspection,
                    operation: CollectionEntryOperation(), progress: { _ in })
            } catch {
                try? await inspection.discard()
                throw error
            }
            #expect(activation.generation == generationBeforeDelete + 1)
            #expect(try await collectionService.waitAvailable(timeoutMilliseconds: 0) == activation.generation)
            // NoteRecord equality covers identity, fields, tags, guid and notetype.
            #expect(try await notes.readNote(id: note.id) == noteBeforeDelete)
            let restoredTree = try await decks.treeWithCounts()
            #expect(restoredTree.generation == activation.generation)
            #expect(restoredTree.nodes == treeBeforeDelete.nodes)
            #expect(restoredTree.nodes.first { $0.id == deck.id } == deckBeforeDelete)
            #expect(try await decks.deckDeletionSummary(ids: [deck.id]) == beforeDelete)
            #expect(try await Core.cardExcerpts(note.cards) == excerptsBeforeDelete)
            guard case .card(let restoredCard) = try await Core.peekFirstCard(deckID: deck.id) else {
                Issue.record("New pre-delete backup did not restore the generated card")
                try await collectionService.close(); return
            }
            #expect(restoredCard.cardId == cardBeforeDelete.cardId && restoredCard.noteId == cardBeforeDelete.noteId)
            #expect(restoredCard.kind == cardBeforeDelete.kind)
            #expect(restoredCard.nextStates == cardBeforeDelete.nextStates)
            #expect(restoredCard.predictedNext == cardBeforeDelete.predictedNext)
            #expect(restoredCard.flag == cardBeforeDelete.flag && restoredCard.marked == cardBeforeDelete.marked)
            #expect(restoredCard.homePreset == cardBeforeDelete.homePreset)
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
