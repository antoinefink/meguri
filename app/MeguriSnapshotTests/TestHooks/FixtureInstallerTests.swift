#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import Testing

@testable import Meguri

/// Fixture installation (U7, R1963), in the app process. Xcode widens the
/// sandbox of a Mac app built for testing, so these tests say nothing about
/// the fixture-read entitlement (tools/mg xcode-check checks it).
@MainActor
@Suite struct FixtureInstallerTests {
    let scratch = FileManager.default.temporaryDirectory.appending(path: "FixtureInstallerTests-\(UUID().uuidString)", directoryHint: .isDirectory)

    private func write(_ files: [String: String], under directory: URL) throws {
        for (path, text) in files {
            let url = directory.appending(path: path)
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data(text.utf8).write(to: url)
        }
    }

    private func contents(of directory: URL) throws -> [String: String] {
        var result: [String: String] = [:]
        let root = directory.standardizedFileURL.pathComponents.count
        for case let url as URL in FileManager.default.enumerator(at: directory, includingPropertiesForKeys: nil)! {
            var isDirectory: ObjCBool = false
            FileManager.default.fileExists(atPath: url.path(percentEncoded: false), isDirectory: &isDirectory)
            guard !isDirectory.boolValue else { continue }
            let path = url.standardizedFileURL.pathComponents.dropFirst(root).joined(separator: "/")
            result[path] = String(decoding: try Data(contentsOf: url), as: UTF8.self)
        }
        return result
    }

    @Test(arguments: ["com.antoinefink.meguri", "com.antoinefink.meguri.devx", ""])
    func refusesWithoutTheDevSuffix(bundleIdentifier: String) throws {
        defer { try? FileManager.default.removeItem(at: scratch) }
        try write(["source/collection.anki2": "fixture"], under: scratch)
        let location = CollectionLocation(directory: scratch.appending(path: "Collection", directoryHint: .isDirectory))
        try write(["Collection/collection.anki2": "real collection"], under: scratch)
        let installer = TestHooks.FixtureInstaller(bundleIdentifier: bundleIdentifier)
        #expect(throws: TestHookError("FixtureInstaller refuses to run: bundle id '\(bundleIdentifier)' does not end in '.dev'")) {
            try installer.install(from: scratch.appending(path: "source/collection.anki2"), into: location)
        }
        #expect(try contents(of: location.directory) == ["collection.anki2": "real collection"])
    }

    @Test func installsAFileAsTheCollectionReplacingWhatWasThere() throws {
        defer { try? FileManager.default.removeItem(at: scratch) }
        try write(["source/any-name.anki2": "fixture", "Collection/old.txt": "stale", "Collection/collection.anki2": "old"], under: scratch)
        let location = CollectionLocation(directory: scratch.appending(path: "Collection", directoryHint: .isDirectory))
        try TestHooks.FixtureInstaller(bundleIdentifier: "com.antoinefink.meguri.dev")
            .install(from: scratch.appending(path: "source/any-name.anki2"), into: location)
        #expect(try contents(of: location.directory) == ["collection.anki2": "fixture"])
    }

    @Test func installsADirectoryAsTheCollectionDirectory() throws {
        defer { try? FileManager.default.removeItem(at: scratch) }
        try write(["source/collection.anki2": "db", "source/collection.media/a.png": "png", "Collection/old.txt": "stale"], under: scratch)
        let location = CollectionLocation(directory: scratch.appending(path: "Collection", directoryHint: .isDirectory))
        try TestHooks.FixtureInstaller(bundleIdentifier: "com.antoinefink.meguri.dev")
            .install(from: scratch.appending(path: "source", directoryHint: .isDirectory), into: location)
        #expect(try contents(of: location.directory) == ["collection.anki2": "db", "collection.media/a.png": "png"])
    }

    @Test func createsMissingParentDirectories() throws {
        defer { try? FileManager.default.removeItem(at: scratch) }
        try write(["source/collection.anki2": "fixture"], under: scratch)
        let location = CollectionLocation(directory: scratch.appending(path: "a/b/Collection", directoryHint: .isDirectory))
        try TestHooks.FixtureInstaller(bundleIdentifier: "x.dev").install(from: scratch.appending(path: "source/collection.anki2"), into: location)
        #expect(try contents(of: location.directory) == ["collection.anki2": "fixture"])
    }

    @Test func aMissingSourceIsAnError() throws {
        defer { try? FileManager.default.removeItem(at: scratch) }
        let location = CollectionLocation(directory: scratch.appending(path: "Collection", directoryHint: .isDirectory))
        let missing = scratch.appending(path: "nope/collection.anki2")
        #expect(throws: TestHookError("-MeguriFixture: no file or directory at \(missing.path(percentEncoded: false))")) {
            try TestHooks.FixtureInstaller(bundleIdentifier: "x.dev").install(from: missing, into: location)
        }
    }

    /// The real entry: `prepare` with this process's bundle id installs, and
    /// with a bundle id lacking `.dev` replaces the whole launch with the error.
    @Test func prepareUsesTheBundleIdentifier() throws {
        defer { try? FileManager.default.removeItem(at: scratch) }
        try write(["source/collection.anki2": "fixture"], under: scratch)
        let source = scratch.appending(path: "source/collection.anki2").path(percentEncoded: false)
        var configuration = LaunchConfiguration()
        configuration.collectionLocation = CollectionLocation(directory: scratch.appending(path: "Collection", directoryHint: .isDirectory))

        var refused = configuration
        let refusal = TestHooks.prepare(&refused, arguments: ["Meguri", "-MeguriFixture", source], bundleIdentifier: "com.antoinefink.meguri")
        #expect(refusal.error?.message.hasPrefix("FixtureInstaller refuses to run: bundle id 'com.antoinefink.meguri'") == true)
        #expect(!FileManager.default.fileExists(atPath: configuration.collectionLocation.directory.path(percentEncoded: false)))

        let session = TestHooks.prepare(&configuration, arguments: ["Meguri", "-MeguriFixture", source])
        #expect(Bundle.main.bundleIdentifier?.hasSuffix(".dev") == true)
        #expect(session.error == nil)
        #expect(try contents(of: configuration.collectionLocation.directory) == ["collection.anki2": "fixture"])
    }
}
#endif
