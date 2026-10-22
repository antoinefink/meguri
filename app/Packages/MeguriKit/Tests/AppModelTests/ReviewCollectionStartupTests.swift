import CoreBridge
import Foundation
import Testing
@testable import AppModel

// Owns the process's real collection; run this suite with a focused selector.
@MainActor
@Suite(.serialized) struct ReviewCollectionStartupTests {
    @Test func freshInstallOpensExistingCollectionWithoutBackups() async throws {
        let files = FileManager.default
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        let original = try Data(contentsOf: source)
        let root = files.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        defer { try? files.removeItem(at: root) }
        let support = root.appending(path: "Application Support", directoryHint: .isDirectory)
        let directory = support.appending(path: "Collection", directoryHint: .isDirectory)
        let backups = support.appending(path: "Backups", directoryHint: .isDirectory)
        let exported = root.appending(path: "export.anki2")
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        try files.copyItem(at: source, to: exported)
        let database = directory.appending(path: "collection.anki2")
        try files.copyItem(at: exported, to: database)
        #expect(try Data(contentsOf: database) == original)
        try #require(!files.fileExists(atPath: backups.path))

        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        configuration.clock = .fixed(Date(timeIntervalSince1970: 1_894_701_600))
        configuration.timeZone = TimeZone(secondsFromGMT: 0)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let service = CoreCollectionService()
        try #require(try await service.lifecycle() == .noCollection)
        do {
            // No fake open or fixture-global override: this uses startOpen/CoreHost.
            await owner.start()
            try #require(owner.state == .ready, "Real open failed: \(owner.error ?? String(describing: owner.state))")
            #expect(try await service.lifecycle() == .ready(generation: owner.generation))
            let backupsExist = files.fileExists(atPath: backups.path)
            #expect(!backupsExist, "Only the real backup writer creates backup directories after capacity preflight")
            #expect(!owner.decks.isEmpty)
            // Owner close flushes before releasing the real core collection.
            let closed = await owner.close()
            try #require(closed, "Close failed: \(owner.error ?? "unknown error")")
            #expect(try await service.lifecycle() == .noCollection)
            #expect(files.fileExists(atPath: database.path))
            #expect(try Data(contentsOf: source) == original)
            #expect(try Data(contentsOf: exported) == original)
        } catch {
            _ = await owner.close()
            try? await service.flushForBackground()
            try? await service.close()
            throw error
        }
    }
}
