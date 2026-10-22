#if MEGURI_TEST_HOOKS
import AppModel
import Foundation
import Testing
@testable import Meguri

@MainActor @Suite struct ImportSourceTests {
    @Test func setupCopiesOnlyDedicatedPublicSourceAndNeverChangesCollectionOrBackups() throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        let documents = root.appending(path: "Documents")
        let backups = documents.appending(path: "Backups")
        let collection = root.appending(path: "Collection/collection.anki2")
        try files.createDirectory(at: backups, withIntermediateDirectories: true)
        try files.createDirectory(at: collection.deletingLastPathComponent(), withIntermediateDirectories: true)
        let backup = backups.appending(path: "keep.colpkg")
        let source = root.appending(path: "public.colpkg")
        let bytes = Data("public source-copy-only test; not an importable package".utf8)
        try bytes.write(to: source)
        try Data("collection sentinel".utf8).write(to: collection)
        try Data("backup sentinel".utf8).write(to: backup)
        try TestHooks.ImportSourceInstaller.install(from: source, bundleIdentifier: "test.dev", documents: documents)
        let destination = documents.appending(path: "RF2 Import Tests/review.colpkg")
        #expect(try Data(contentsOf: destination) == bytes)
        #expect(try Data(contentsOf: source) == bytes)
        #expect(try Data(contentsOf: collection) == Data("collection sentinel".utf8))
        #expect(try Data(contentsOf: backup) == Data("backup sentinel".utf8))
        try Data("new public source".utf8).write(to: source)
        try TestHooks.ImportSourceInstaller.install(from: source, bundleIdentifier: "test.dev", documents: documents)
        #expect(try Data(contentsOf: destination) == Data("new public source".utf8))
        #expect(try files.contentsOfDirectory(atPath: documents.path).sorted() == ["Backups", "RF2 Import Tests"])
    }
    @Test func sourceSetupRefusesProductionBundleAndLinks() throws {
        let files = FileManager.default
        let root = files.temporaryDirectory.appending(path: UUID().uuidString)
        defer { try? files.removeItem(at: root) }
        try files.createDirectory(at: root, withIntermediateDirectories: true)
        let source = root.appending(path: "public.colpkg")
        try Data("public".utf8).write(to: source)
        #expect(throws: TestHookError("ImportSourceInstaller requires a .dev bundle")) {
            try TestHooks.ImportSourceInstaller.install(from: source, bundleIdentifier: "test", documents: root.appending(path: "Documents"))
        }
        let link = root.appending(path: "link.colpkg")
        try files.createSymbolicLink(at: link, withDestinationURL: source)
        #expect(throws: (any Error).self) {
            try TestHooks.ImportSourceInstaller.install(from: link, bundleIdentifier: "test.dev", documents: root.appending(path: "Documents"))
        }
        #expect(!files.fileExists(atPath: root.appending(path: "Documents").path))
    }
    @Test func parsingSourceNeverSelectsAnAlternateRouteOrDestination() throws {
        let options = try TestHooks.LaunchOptions(arguments: ["-MeguriImportSource", "/public/review.colpkg"])
        #expect(options.importSource == URL(filePath: "/public/review.colpkg"))
        #expect(options.fixture == nil && options.route == nil)
    }
}
#endif
