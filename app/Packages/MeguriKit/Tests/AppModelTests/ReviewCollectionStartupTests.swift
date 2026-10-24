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

    @Test func existingInvalidCollectionFailsWithoutChangingOriginalBytes() async throws {
        let files = FileManager.default
        let source = try #require(Bundle.module.url(forResource: "schema-18", withExtension: "anki2"))
        let root = files.temporaryDirectory.appending(path: UUID().uuidString, directoryHint: .isDirectory)
        defer { try? files.removeItem(at: root) }
        let recovery = root.appending(path: "recovery.anki2")
        try files.createDirectory(at: root, withIntermediateDirectories: true)
        try files.copyItem(at: source, to: recovery)
        let directory = root.appending(path: "Collection", directoryHint: .isDirectory)
        try files.createDirectory(at: directory, withIntermediateDirectories: true)
        let database = directory.appending(path: "collection.anki2")
        let original = Data("Synthetic invalid collection for the startup regression.".utf8)
        try original.write(to: database)
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = AppModel.CollectionLocation(directory: directory)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        let service = CoreCollectionService()
        try #require(try await service.lifecycle() == .noCollection)
        func release() async throws {
            _ = await owner.close()
            try await service.close()
            let archive = root.appending(path: "invalid-original.anki2")
            try files.moveItem(at: database, to: archive)
            await owner.start()
            let closed = await owner.close()
            try #require(closed)

            try files.moveItem(at: recovery, to: database)
            var recoveryFailure: (any Error)?
            do {
                await owner.start()
                try #require(owner.state == .ready, "Recovery open failed: \(owner.error ?? String(describing: owner.state))")
                #expect(try await service.lifecycle() == .ready(generation: owner.generation))
                #expect(try Data(contentsOf: archive) == original)
            } catch { recoveryFailure = error }
            let recoveryClosed = await owner.close()
            if !recoveryClosed {
                Issue.record("Recovery close failed: \(owner.error ?? "unknown error")")
                try await service.close()
                try files.moveItem(at: database, to: root.appending(path: "recovery-closed.anki2"))
                await owner.start()
                let cancelled = await owner.close()
                try #require(cancelled)
            }
            if let recoveryFailure { throw recoveryFailure }
        }
        var failure: (any Error)?
        do {
            await owner.start()
            if case .failed(let message) = owner.state {
                #expect(message.contains("collection"))
                #expect(message.contains("Settings") || message.contains("try again"))
                #expect(!message.contains("detail:"))
                #expect(!message.contains("not a database"))
                #expect(!message.contains(root.path))
            } else {
                Issue.record("An existing invalid collection must present an open failure")
            }
            #expect(try Data(contentsOf: database) == original)
            #expect(owner.decks.isEmpty)
        } catch { failure = error }
        do {
            try await release()
            #expect(try await service.lifecycle() == .noCollection)
        } catch {
            if let failure { Issue.record(failure) }
            throw error
        }
        if let failure { throw failure }
    }

    @Test func openFailurePresentationDoesNotEchoDiagnosticDetails() {
        let diagnostic = "synthetic-diagnostic-must-stay-private"
        let failures: [CollectionOpenFailure] = [
            .openInAnotherProgram, .tooOld(version: -1), .tooNew(version: 999), .v1Scheduler,
            .notACollection(missingTables: [diagnostic]),
            .upgradeFailed(version: 18, stage: .step(1), detail: diagnostic),
            .corrupt(detail: diagnostic), .connectionInit(detail: diagnostic),
            .notFound, .io(detail: diagnostic)
        ]
        for failure in failures {
            let message = ReviewCollection.openExplanation(BridgeError.open(failure))
            #expect(!message.isEmpty)
            #expect(!message.contains(diagnostic))
            // Job completion and lifecycle delivery must explain the same refusal.
            #expect(message == ReviewCollection.explanation(CollectionFailedReason.open(failure)))
        }
        #expect(ReviewCollection.explanation(CollectionOpenFailure.openInAnotherProgram).contains("Close it there"))
        #expect(ReviewCollection.explanation(CollectionOpenFailure.tooNew(version: 999)).contains("update"))
        let reasons: [CollectionFailedReason] = [
            .openStep(step: diagnostic, detail: diagnostic), .replace(detail: diagnostic), .corrupt(detail: diagnostic)
        ]
        for reason in reasons { #expect(!ReviewCollection.explanation(reason).contains(diagnostic)) }
        #expect(!ReviewCollection.openExplanation(BridgeError.internal(diagnostic)).contains(diagnostic))
        #expect(!ReviewCollection.openExplanation(NSError(domain: diagnostic, code: 1)).contains(diagnostic))
    }
}
