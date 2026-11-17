import Foundation
import Testing
@testable import CoreBridge

@Suite struct ProviderAdmissionTests {
    @Test func malformedLegacyValuesAreNotPerFieldDefaults() throws {
        let values: [Any] = [NSNull(), [:] as [String: Any], ["enabled": 1, "paused": false, "automatic": true, "media": true] as [String: Any]]
        for value in values {
            #expect(throws: (any Error).self) { try CoreProviderService.strictLegacyJSON(value) }
        }
        #expect(try CoreProviderService.strictLegacyJSON(nil) == nil)
        let settings: [String: Any] = ["enabled": true, "paused": true, "automatic": false, "media": false]
        #expect(try CoreProviderService.strictLegacyJSON(settings) != nil)
    }
}

// These real Host lifetimes share EventTests' process-wide serialization.
extension EventTests {
    @Test @MainActor func persistedProviderHandoffNeedsExplicitRecoveryOnTheReopenedHost() async throws {
        try await withProviderAdmissionCollection { collection, location in
            let original = try await CoreProviderService.attached()
            // Discard the actual original handle. Recovery cannot reconstruct
            // it or substitute a persisted assertion that the old process joined.
            _ = try await original.beginHandoff(to: .meguri)
            let file = location.manualTransferDirectory.appending(path: "provider.json")
            let intent = try Data(contentsOf: file)
            #expect(try await original.state().phase == .handoffRecovery)
            try await collection.close()
            try await collection.open(location, randomSeed: 271)
            let current = try await CoreProviderService.attached()
            #expect(try await current.refresh().phase == .handoffRecovery)
            #expect(try Data(contentsOf: file) == intent)
            await #expect(throws: (any Error).self) { _ = try await original.recoverHandoffToLocalOnly() }
            await #expect(throws: (any Error).self) { _ = try await original.retryFailedPublication() }
            let recovered = try await current.recoverHandoffToLocalOnly()
            #expect(recovered.selection == .localOnly && recovered.phase == .localOnly)
            #expect(!recovered.permitsICloudStartup && !recovered.removalPending)
            let committed = try Data(contentsOf: file)
            #expect(committed != intent)
            await #expect(throws: (any Error).self) {
                _ = try await original.saveSettings(.init(enabled: true, paused: false, automatic: true, media: true))
            }
            #expect(try Data(contentsOf: file) == committed)
            _ = try await CoreDeckService().addDeck(name: "Synthetic local study after recovery")
            try await collection.close()
            try await collection.open(location, randomSeed: 271)
            let reopened = try await CoreProviderService.attached()
            #expect(try await reopened.refresh().selection == .localOnly)
            #expect(try Data(contentsOf: file) == committed)
        }
    }

    @Test @MainActor func lostCredentialRemovalCannotBeCompletedByCurrentOwnerRecovery() async throws {
        try await withProviderAdmissionCollection { collection, location in
            let original = try await CoreProviderService.attached()
            _ = try await original.beginHandoff(to: .localOnly, removeCredentials: true)
            try #require(try await original.state().removalPending)
            let file = location.manualTransferDirectory.appending(path: "provider.json")
            let intent = try Data(contentsOf: file)
            try await collection.close()
            try await collection.open(location, randomSeed: 271)
            let current = try await CoreProviderService.attached()
            await #expect(throws: (any Error).self) { _ = try await current.recoverHandoffToLocalOnly() }
            let stopped = try await current.refresh()
            #expect(stopped.phase == .handoffRecovery && stopped.removalPending)
            #expect(!stopped.permitsICloudStartup)
            #expect(try Data(contentsOf: file) == intent)
        }
    }

    @Test(arguments: ["{\"version\":999}", "{\"version\":1,\"handoff\":{\"old\":\"ICloud\"}}"])
    @MainActor func explicitProviderActionsCannotRepairUnknownOrPartialMetadata(metadata: String) async throws {
        try await withProviderAdmissionCollection(metadata: Data(metadata.utf8)) { _, location in
            let current = try await CoreProviderService.attached()
            let file = location.manualTransferDirectory.appending(path: "provider.json")
            let before = try Data(contentsOf: file)
            await #expect(throws: (any Error).self) { _ = try await current.recoverHandoffToLocalOnly() }
            await #expect(throws: (any Error).self) { _ = try await current.retryFailedPublication() }
            #expect(try await current.refresh().phase == .blocked)
            #expect(try Data(contentsOf: file) == before)
        }
    }
}

@MainActor private func withProviderAdmissionCollection(
    metadata: Data? = nil,
    _ body: @MainActor (CoreCollectionService, CoreBridge.CollectionLocation) async throws -> Void
) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-provider-admission-\(UUID().uuidString)")
    let location = CoreBridge.CollectionLocation(collectionDirectory: root.appending(path: "collection"),
        backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget.snapshot"),
        manualTransferDirectory: root.appending(path: "installation"))
    for directory in [location.collectionDirectory, location.backupsDirectory, location.manualTransferDirectory] {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }
    defer { try? FileManager.default.removeItem(at: root) }
    if let metadata { try metadata.write(to: location.manualTransferDirectory.appending(path: "provider.json")) }
    _ = try await CoreProviderService.migrate(directory: location.manualTransferDirectory, legacyJSON: nil)
    try await MaintenanceLane.shared.bootstrapCollection(location)
    let collection = CoreCollectionService()
    do {
        try await collection.open(location, randomSeed: 271)
        try await body(collection, location)
        try await collection.close()
    } catch { try? await collection.close(); throw error }
}
