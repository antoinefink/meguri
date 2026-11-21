import Foundation
import Testing
@testable import CoreBridge

private final class ReadsClock: CollectionClock {
    func nowMilliseconds() -> Int64 { 1_894_701_600_000 }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

private final class ReadsDiskCapacity: DiskSpaceProviding {
    func availableBytes(at directory: URL) throws -> UInt64 { .max }
}

extension EventTests {
    /// U267's reads through CoreBridge on an opened collection: the list in
    /// A2's order, every job's value, the summary's counts confirmed back
    /// into a draft, and an unknown note type's error.
    @Test @MainActor func notetypeReadsRunAsJobsOnTheOpenedCollection() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        let directory = root.appending(path: "collection")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: root.appending(path: "backups"), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CoreBridge.CollectionLocation(collectionDirectory: directory,
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService(clock: ReadsClock(), diskSpace: ReadsDiskCapacity())
        let notes = CoreNoteService(), types = CoreNotetypeService()
        do {
            try await service.open(location, randomSeed: 41)
            let added = try await types.addNotetype(.init(name: "Trio", cloze: false, fields: ["Front", "Back", "Extra"],
                templates: [.init(name: "Forward", front: "{{Front}}", back: "{{FrontSide}}", browserFront: "", browserBack: ""),
                            .init(name: "Reverse", front: "{{Back}}", back: "{{FrontSide}}", browserFront: "", browserBack: "")],
                css: ""))
            for fields in [["a", "b", "x"], ["c", "<br>", ""]] {
                _ = try await notes.addNote(.init(notetypeId: added.id, fields: fields, tags: [], deckId: 1, guid: nil,
                                                  updateAddDefaults: false))
            }

            let list = try await types.notetypeList()
            #expect(list.contains { $0.id == added.id && $0.name == "Trio" })
            let counts = try await types.startNotetypeNoteCounts().value()
            #expect(counts[added.id] == 2)
            #expect(Set(counts.keys) == Set(list.map(\.id)))

            let contents = try await types.startNotetypeFieldContentCounts(notetypeId: added.id).value()
            #expect(contents == NotetypeFieldContentCounts(notes: 2, fields: [2, 1, 1]))
            let unknown = try await types.startNotetypeFieldContentCounts(notetypeId: 9_999)
            await #expect(throws: BridgeError.notFound(table: .notetypes, key: "9999")) { try await unknown.value() }

            let record = try await types.readNotetype(id: added.id)
            var draft = NotetypeStructureDraft(record, skipChecks: true)
            draft.fields.removeLast()
            draft.templates.removeLast()
            let summary = try await types.startNotetypeDraftSummary(draft).value()
            #expect(summary.losesContent)
            #expect(summary.changes == [.fieldRemoved(ord: 2, name: "Extra", notesWithContent: 1),
                                        .templateRemoved(ord: 1, name: "Reverse", cardsRemoved: 1, notesWithoutCards: 0)])
            #expect(NotetypeConfirmedLosses(confirming: summary) == NotetypeConfirmedLosses(fields: [2: 1], templates: [1: 1]))

            let blank = try await types.startNotetypeNotesWithoutCard(NotetypeStructureDraft(record, skipChecks: true)).value()
            #expect(blank.count == 2)
            #expect(blank[0] == NotetypeBlankFront(noteIds: [], field: nil))
            #expect(blank[1].noteIds.count == 1 && blank[1].field == 1)

            let removal = try await types.startNotetypeRemovalCounts(notetypeId: added.id).value()
            #expect(removal == NotetypeRemovalCounts(notes: 2, cards: 3))

            #expect(try await types.notetypeNearFieldName("extra", fields: ["Front", "Extra"]) == 1)
            #expect(try await types.notetypeNearFieldName("Elsewhere", fields: ["Front"]) == nil)
            try await service.close()
        } catch {
            try? await service.close()
            throw error
        }
    }
}
