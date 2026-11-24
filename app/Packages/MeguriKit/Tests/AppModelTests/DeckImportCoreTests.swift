@testable import AppModel
import CoreBridge
import Foundation
import Testing

// Shares the production host with the serialized startup suite.
extension ReviewCollectionStartupTests {
    /// U162 end to end on the real core: the intake classifies a deck package,
    /// the sheet's model inspects it, imports it with the stored options, and
    /// the deck list shows the new decks without a relaunch; Undo Import
    /// takes them away again.
    @Test func deckPackageImportsThroughTheSheetModel() async throws {
        let files = FileManager.default
        let directory = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: directory) }
        let collection = directory.appending(path: "Collection", directoryHint: .isDirectory)
        try files.createDirectory(at: collection, withIntermediateDirectories: true)
        try files.copyItem(at: #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2")),
                           to: collection.appending(path: "collection.anki2"))
        let package = directory.appending(path: "Shared Deck.apkg")
        try files.copyItem(at: #require(Bundle.module.url(forResource: "deck-tree", withExtension: "apkg")), to: package)
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: collection)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let sync = SyncCoordinator(directory: collection, configuration: nil, defaults: nil)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation(), sync: sync,
            providerDirectory: directory.appending(path: "synthetic-installation", directoryHint: .isDirectory))
        await owner.start()
        do {
            try #require(owner.state == .ready)
            func names(_ nodes: [CountedDeckNode], _ parent: String = "") -> [String] {
                nodes.flatMap { node -> [String] in
                    let name = parent.isEmpty ? node.name : parent + "::" + node.name
                    return [name] + names(node.children, name)
                }
            }
            let before = names(owner.decks)
            owner.deckImport.progressDelay = .zero

            // The intake: P18 by content and name, then the sheet.
            owner.intake.receive([package])
            for _ in 0..<500 where owner.deckImport.step != .options && owner.deckImport.step != .refused {
                try await Task.sleep(for: .milliseconds(10))
            }
            await owner.deckImport.waitForWork()
            try #require(owner.deckImport.step == .options, "\(String(describing: owner.deckImport.refusal))")
            #expect(owner.navigation.sheet == .deckImport)
            let facts = try #require(owner.deckImport.facts)
            #expect(facts.notes > 0 && facts.cards > 0 && !facts.decks.isEmpty)
            #expect(owner.deckImport.fileName == "Shared Deck.apkg")

            owner.deckImport.importPackage()
            await owner.deckImport.waitForWork()
            try #require(owner.deckImport.step == .result, "\(String(describing: owner.deckImport.refusal))")
            let result = try #require(owner.deckImport.result)
            #expect(result.counts.notesAdded == facts.notes)
            #expect(result.log[.new] == facts.notes)
            #expect(result.undoStep == "Import" && owner.deckImport.canUndo)

            // The deck list reads the import without a relaunch.
            var after = names(owner.decks)
            for _ in 0..<300 where Set(after) == Set(before) {
                try await Task.sleep(for: .milliseconds(10)); after = names(owner.decks)
            }
            #expect(Set(after).isStrictSuperset(of: Set(before)))
            // The log's first page.
            owner.deckImport.loadLog(.new)
            for _ in 0..<300 where owner.deckImport.log[.new] == nil { try await Task.sleep(for: .milliseconds(10)) }
            #expect(UInt64(owner.deckImport.log[.new]?.count ?? 0) == min(facts.notes, 50))

            owner.deckImport.undoImport()
            for _ in 0..<300 where !owner.deckImport.undone { try await Task.sleep(for: .milliseconds(10)) }
            #expect(owner.deckImport.undone)
            var undone = names(owner.decks)
            for _ in 0..<300 where Set(undone) != Set(before) {
                try await Task.sleep(for: .milliseconds(10)); undone = names(owner.decks)
            }
            #expect(Set(undone) == Set(before))

            owner.deckImport.done()
            #expect(owner.navigation.sheet == nil && owner.deckImport.step == .idle)
            #expect(await owner.close())
        } catch {
            _ = await owner.close()
            throw error
        }
    }
}
