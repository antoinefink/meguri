#if MEGURI_TEST_HOOKS
import AppModel
import Foundation

extension TestHooks {
    /// Persistent across relaunches, exclusive to one synthetic Debug journey.
    /// No path argument, shared collection access, cleanup or fixture reinstall.
    enum FixtureRun {
        static func location(id: UUID, fixture: URL?, bundleIdentifier: String) throws(TestHookError) -> CollectionLocation {
            #if DEBUG
            guard bundleIdentifier == "com.antoinefink.meguri.dev" else {
                throw TestHookError("FixtureRunID requires Meguri Dev")
            }
            let files = FileManager.default
            let parent = URL.applicationSupportDirectory.appending(path: "Meguri-TestRuns")
            let root = parent.appending(path: id.uuidString)
            let location = CollectionLocation(directory: root.appending(path: "Collection"),
                backupsDirectory: root.appending(path: "Backups"))
            do {
                try files.createDirectory(at: parent, withIntermediateDirectories: true)
                try requireDirectory(parent)
                if let fixture {
                    var checkout = URL(filePath: #filePath)
                    for _ in 0..<4 { checkout.deleteLastPathComponent() }
                    let empty = checkout.appending(path: "app/Fixtures/CollectionEntry/EmptyCollection")
                    guard fixture.standardizedFileURL == empty.standardizedFileURL,
                          fixture.resolvingSymlinksInPath().standardizedFileURL == empty.standardizedFileURL else {
                        throw TestHookError("FixtureRunID permits only the checkout EmptyCollection fixture")
                    }
                    // Exclusive creation refuses reuse even if the previous run
                    // failed; FixtureInstaller can only copy into this new root.
                    guard !files.fileExists(atPath: root.path) else {
                        throw TestHookError("FixtureRunID already exists; relaunch without Fixture")
                    }
                    try files.createDirectory(at: root, withIntermediateDirectories: false,
                        attributes: [.posixPermissions: 0o700])
                    try files.createDirectory(at: location.backupsDirectory, withIntermediateDirectories: false)
                } else {
                    try requireDirectory(root)
                    try requireDirectory(location.directory)
                    try requireDirectory(location.backupsDirectory)
                }
                return location
            } catch let error as TestHookError { throw error }
            catch { throw TestHookError("Could not prepare isolated fixture run: \(error.localizedDescription)") }
            #else
            throw TestHookError("FixtureRunID requires Debug")
            #endif
        }

        private static func requireDirectory(_ url: URL) throws {
            let values = try url.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
            guard values.isDirectory == true, values.isSymbolicLink != true else {
                throw TestHookError("Fixture run directories must not be links")
            }
        }
    }
}

extension TestHooks.Session {
    /// Reuse the normal provider composition with fresh Off settings and its
    /// own installation directory; never discover the VM Dev copy's account.
    func fixtureRunCollection(configuration: LaunchConfiguration, navigation: AppNavigation) -> ReviewCollection? {
        #if DEBUG
        guard options.fixtureRunID != nil else { return nil }
        let sync = SyncCoordinator(directory: configuration.collectionLocation.directory,
            clock: configuration.clock, configuration: nil, defaults: nil)
        return ReviewCollection(configuration: configuration, navigation: navigation, sync: sync,
            providerDirectory: configuration.collectionLocation.directory.deletingLastPathComponent().appending(path: "Installation"))
        #else
        return nil
        #endif
    }
}
#endif
