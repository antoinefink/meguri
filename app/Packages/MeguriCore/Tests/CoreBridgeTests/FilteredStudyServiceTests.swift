import CoreBridge
import Foundation
import Synchronization
import Testing

private final class FilteredStudyClock: CollectionClock {
    private let instant = Mutex<Int64>(1_894_701_600_000)
    func advance() { instant.withLock { $0 += 1000 } }
    func nowMilliseconds() -> Int64 { instant.withLock { $0 } }
    func utcOffsetSeconds(atMilliseconds: Int64) -> Int32 { 0 }
    func zoneIdentifier() -> String { "UTC" }
}

// Real facade, lanes, Rust jobs and exclusive replacement in the serialized
// process-wide collection suite. No lifecycle event or FFI result is substituted.
extension EventTests {
    @Test(arguments: [false, true]) @MainActor func filteredStudyFacadeUsesOriginatingGenerationAcrossRealReplacement(
        usedBeforeReplacement: Bool
    ) async throws {
        try await checkFilteredStudyReplacement(usedBeforeReplacement: usedBeforeReplacement)
    }

    @MainActor private func checkFilteredStudyReplacement(usedBeforeReplacement: Bool) async throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
            backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
            manualTransferDirectory: root.appending(path: "ManualTransfers"))
        try files.createDirectory(at: location.collectionDirectory, withIntermediateDirectories: true)
        try files.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: true)
        defer { try? files.removeItem(at: root) }
        try await MaintenanceLane.shared.bootstrapCollection(location)
        let clock = FilteredStudyClock(), collections = CoreCollectionService(clock: clock)
        let maintenance = CoreMaintenanceService(), decks = CoreDeckService()
        try await collections.open(location, randomSeed: 631)
        do {
            let first = try await collections.waitAvailable(timeoutMilliseconds: 0)
            let oldFacade = CoreFilteredStudyService()
            var definition = CoreBridge.FilteredStudyDefinition(name: "Original study",
                terms: [.init(search: "", limit: 100, order: 1)], reschedule: true,
                againSeconds: 60, hardSeconds: 600, goodSeconds: 0)
            if usedBeforeReplacement {
                _ = try await oldFacade.save(deck: nil, definition: definition, allowEmpty: true, generation: first)
            }
            let backup = try await maintenance.createBackup(operation: MaintenanceOperation(), progress: { _ in })
            clock.advance()
            let restored = try await maintenance.restoreBackup(name: backup.name, operation: MaintenanceOperation(), progress: { _ in })
            let next = restored.generation
            #expect(next == first + 1)
            let before = try await decks.allDeckNames()
            definition.name = "Old route must not save here"
            do throws(BridgeError) {
                _ = try await oldFacade.save(deck: nil, definition: definition, allowEmpty: true, generation: first)
                Issue.record("A route from the old collection wrote into its successor")
            } catch {
                #expect(error == .staleGeneration(planned: first, current: next))
            }
            #expect(try await decks.allDeckNames() == before)

            let facade = CoreFilteredStudyService()
            let unicode = try await decks.addDeck(name: "A\u{a0}B")
            let child = try await decks.addDeck(name: "A\u{a0}B::Child")
            let lookalike = try await decks.addDeck(name: "A B")
            _ = try await decks.addDeck(name: "A B::Child")
            let scope = try await facade.deckScope(deck: unicode.id, generation: next)
            let expectedIDs = [unicode.id, child.id].sorted().map(String.init).joined(separator: ",")
            #expect(scope == "did:\(expectedIDs)")
            #expect(try await facade.deckScope(deck: lookalike.id, generation: next) != scope)
            definition.name = "Replacement study"
            let selected = try await facade.save(deck: nil, definition: definition, allowEmpty: true, generation: next)
            #expect(try await facade.definition(deck: selected, generation: next) == definition)
            var noMatches = definition
            noMatches.name = "Unconfirmed edit"
            do throws(BridgeError) {
                _ = try await facade.save(deck: selected, definition: noMatches, allowEmpty: false, generation: next)
                Issue.record("An empty edit saved without consent")
            } catch {
                #expect(error == .deckRefused(.noMatches))
            }
            #expect(try await facade.definition(deck: selected, generation: next) == definition)
            #expect(try await facade.save(deck: selected, definition: noMatches, allowEmpty: true, generation: next) == selected)
            #expect(try await facade.definition(deck: selected, generation: next) == noMatches)
            try await facade.rebuild(deck: selected, generation: next)
            try await facade.empty(deck: selected, generation: next)
            let request = CoreBridge.CustomStudyRequest(mode: .increaseNew, amount: 0, state: .new, includeTags: [], excludeTags: [])
            #expect(try await facade.custom(deck: 1, request: request, generation: next) == 1)
            _ = try await facade.customDefaults(deck: 1, generation: next)
            try await collections.close()
            for await event in CoreEventStream.shared.events {
                if event.generation == next && event.kind == .lifecycle(.noCollection) { break }
            }
        } catch {
            try? await collections.close()
            throw error
        }
    }
}
