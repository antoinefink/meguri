import Foundation
import Testing
@testable import CoreBridge

private final class RemovalClock: CollectionClock {
    func nowMilliseconds() -> Int64 { 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}
private final class RemovalDisk: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

extension EventTests {
    /// The published removal bridge must carry both preconditions, forward
    /// typed refusals, return the real result and expose the core's Undo.
    @Test @MainActor func notetypeRemovalRecountsDeletesAndUndoesThroughTheBridge() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory, backupsDirectory: backups,
            appGroupSnapshot: root.appending(path: "widget.snapshot"), manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let collection = CoreCollectionService(clock: RemovalClock(), diskSpace: RemovalDisk())
        let types = CoreNotetypeService(), notes = CoreNoteService()
        do {
            try await collection.open(location, randomSeed: 41)
            let added = try await types.addNotetype(.init(name: "Removal Sample", cloze: false, fields: ["Front", "Back"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{Back}}", browserFront: "", browserBack: ""),
                            .init(name: "Reverse", front: "{{Back}}", back: "{{Front}}", browserFront: "", browserBack: "")], css: ""))
            let note = try await notes.addNote(.init(notetypeId: added.id, fields: ["front", "back"], tags: [], deckId: 1,
                guid: nil, updateAddDefaults: false))
            let before = try await types.readNotetype(id: added.id)
            let counts = try await types.startNotetypeRemovalCounts(notetypeId: added.id).value()
            #expect(counts == .init(notes: 1, cards: 2))
            let generation = try await collection.waitAvailable(timeoutMilliseconds: 1_000)
            await #expect(throws: BridgeError.staleGeneration(planned: 0, current: generation)) {
                try await types.startNotetypeRemoval(.init(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 2,
                    expectedGeneration: 0))
            }
            #expect(try await types.readNotetype(id: added.id) == before)
            // A lower count in either category must refuse without writing.
            for request in [NotetypeRemoval(notetypeId: added.id, confirmedNotes: 0, confirmedCards: 2),
                            NotetypeRemoval(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 1)] {
                let job = try await types.startNotetypeRemoval(request)
                await #expect(throws: BridgeError.notetypeStructureRefused(.removalLossesChanged(notes: 1, cards: 2))) {
                    try await job.value()
                }
                #expect(try await types.readNotetype(id: added.id) == before)
            }
            let removed = try await types.startNotetypeRemoval(.init(notetypeId: added.id, confirmedNotes: 1, confirmedCards: 2,
                expectedGeneration: generation)).value()
            #expect(removed.removed && removed.id == added.id && removed.notesRemoved == 1 && removed.cardsRemoved == 2)
            #expect(!((try await types.notetypeList()).contains { $0.id == added.id }))
            #expect(removed.summary.undoName == "Remove Note Type")
            _ = try await CoreUndoService().startUndo(scope: .global).value()
            #expect(try await types.readNotetype(id: added.id) == before)
            #expect(try await notes.readNote(id: note.id).fields == ["front", "back"])
            #expect(try await types.startNotetypeRemovalCounts(notetypeId: added.id).value() == counts)
            // S12: removing a missing type is an acknowledged no-op.
            let missing = try await types.startNotetypeRemoval(.init(notetypeId: 9_999, confirmedNotes: 0, confirmedCards: 0)).value()
            #expect(!missing.removed)
            try await collection.close()
        } catch {
            try? await collection.close()
            throw error
        }
    }
}
