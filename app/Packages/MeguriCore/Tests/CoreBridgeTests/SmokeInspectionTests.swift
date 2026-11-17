#if DEBUG
import Foundation
import Testing
@testable import CoreBridge

// The existing serialized process-owner suite owns CoreHost/listener lifetime.
// Generated local inspection only; no account, SDK, recovery or signed evidence.
extension EventTests {
    @Test @MainActor func smokeIntegrityUsesOwnedGenerationAndSeesDurableLocalMutation() async throws {
        let root = FileManager.default.temporaryDirectory.appending(path: "smoke-inspection-\(UUID().uuidString)")
        let data = root.appending(path: "collection")
        let backups = root.appending(path: "backups")
        try FileManager.default.createDirectory(at: data, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: backups, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let location = CollectionLocation(collectionDirectory: data, backupsDirectory: backups,
                                          appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let service = CoreCollectionService()
        do {
            try await service.open(location, randomSeed: 113)
            let generation = try await service.waitAvailable(timeoutMilliseconds: 0)
            let inspection = CoreSmokeInspection()
            let initial = try await inspection.integrity()
            #expect(initial.generation == generation && initial.integrityOK && initial.schemaOK && initial.referenceFailures == 0)
            let types = try await CoreNotetypeService().notetypeNames()
            let basic = try #require(types.first { $0.name == "Basic" })
            let mutation = try await CoreNoteService().addNote(.init(notetypeId: basic.id,
                fields: ["generated smoke Q", "generated smoke A"], tags: [], deckId: 1, guid: nil, updateAddDefaults: false))
            #expect(mutation.written && mutation.cards.count == 1)
            try await service.flushForBackground()
            let final = try await inspection.integrity()
            #expect(final.generation == generation && final.seq >= mutation.summary.seq && final.seq > initial.seq)
            #expect(final.integrityOK && final.schemaOK && final.referenceFailures == 0)
            try await service.close()
            await #expect(throws: (any Error).self) { try await inspection.integrity() }
            for await event in CoreEventStream.shared.events {
                if event.generation == generation && event.kind == .lifecycle(.noCollection) { break }
            }
        } catch {
            try? await service.close()
            throw error
        }
    }
}
#endif
