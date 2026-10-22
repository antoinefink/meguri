import CardRendering
import CoreBridge
import Foundation
import Testing
@testable import AppModel

@MainActor
@Suite struct ReviewMediaBindingTests {
    @Test func journalArrivalUpdatesResolverAndFailedOrStaleBindingClosesIt() async throws {
        let directory = FileManager.default.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: directory) }
        let root = directory.appending(path: "collection.media", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try Data("asset".utf8).write(to: root.appending(path: "MiXeD.PNG"))
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = CollectionLocation(directory: directory)
        let owner = ReviewCollection(configuration: configuration, navigation: AppNavigation())
        owner.loadMediaIndex = { MediaNameSnapshot(generation: 0, root: root, names: []) }
        let resolver = try #require(await owner.mediaResolverForReview())
        await #expect(throws: (any Error).self) { try await resolver.openURLPath("/mixed.png") }
        owner.loadMediaIndex = {
            MediaNameSnapshot(generation: 0, root: root, names: [TrackedMediaName(name: "MiXeD.PNG", caseKey: "mixed.png")])
        }
        let refreshed = try #require(await owner.mediaResolverForReview())
        #expect(refreshed === resolver)
        let file = try await resolver.openURLPath("/mixed.png")
        #expect(try await file.read(offset: 0, count: 16) == Data("asset".utf8))
        await file.close()
        owner.loadMediaIndex = { MediaNameSnapshot(generation: 1, root: root, names: []) }
        #expect(await owner.mediaResolverForReview() == nil)
        await #expect(throws: (any Error).self) { try await resolver.openURLPath("/MiXeD.PNG") }
        owner.loadMediaIndex = { throw BridgeError.internal("journal read failed") }
        #expect(await owner.mediaResolverForReview() == nil)
        #expect(owner.error?.contains("journal read failed") == true)
    }

    @Test func bindingRequiresBothGenerationAndCollectionRoot() {
        let root = URL(fileURLWithPath: "/tmp/collection/collection.media")
        #expect(ReviewCollection.acceptsMedia(MediaNameSnapshot(generation: 4, root: root, names: []), generation: 4, root: root))
        #expect(!ReviewCollection.acceptsMedia(MediaNameSnapshot(generation: 3, root: root, names: []), generation: 4, root: root))
        #expect(!ReviewCollection.acceptsMedia(MediaNameSnapshot(generation: 4, root: root.deletingLastPathComponent(), names: []), generation: 4, root: root))
    }
}
