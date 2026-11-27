import Foundation
@testable import CoreBridge

/// A synthetic local collection for unrelated bridge suites. No provider is selected.
func withBridgeCollection(_ body: () async throws -> Void) async throws {
    let root = FileManager.default.temporaryDirectory.appending(path: "synthetic-bridge-\(UUID().uuidString)")
    let location = CollectionLocation(collectionDirectory: root.appending(path: "collection"),
        backupsDirectory: root.appending(path: "backups"), appGroupSnapshot: root.appending(path: "widget"),
        manualTransferDirectory: root.appending(path: "installation"))
    for directory in [location.collectionDirectory, location.backupsDirectory, location.manualTransferDirectory] {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    }
    defer { try? FileManager.default.removeItem(at: root) }
    _ = try await CoreProviderService.migrate(directory: location.manualTransferDirectory, legacyJSON: nil)
    try await MaintenanceLane.shared.bootstrapCollection(location)
    let collection = CoreCollectionService()
    do {
        try await collection.open(location, randomSeed: 114)
        try await body()
        try await collection.close()
    } catch { try? await collection.close(); throw error }
}
