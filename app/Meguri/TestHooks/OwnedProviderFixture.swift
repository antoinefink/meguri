#if MEGURI_TEST_HOOKS
import AppModel
import Foundation

extension TestHooks {
    /// One disposable launch's storage. Only the captured successful Core close
    /// may release it; failed installation/open/close deliberately leaves evidence.
    final class OwnedProviderFixture {
        static var source: URL {
            var checkout = URL(filePath: #filePath)
            for _ in 0..<4 { checkout.deleteLastPathComponent() }
            return checkout.appending(path: "build/fixtures/recipes/local-review-1", directoryHint: .isDirectory)
        }

        let collectionLocation: CollectionLocation
        let installationDirectory: URL
        let defaults: UserDefaults
        private let root: URL
        private let suite: String
        private var removedRoot = false
        private var cleaned = false

        static func validateSource(_ fixture: URL?) throws(TestHookError) {
            let expected = source
            guard let fixture, fixture.isFileURL,
                  fixture.path(percentEncoded: false) == expected.path(percentEncoded: false),
                  fixture.standardizedFileURL == expected.standardizedFileURL,
                  fixture.resolvingSymlinksInPath().standardizedFileURL == expected.standardizedFileURL else {
                throw TestHookError("OwnedProviderFixture refuses another source or a path alias")
            }
            do {
                let values = try fixture.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                guard values.isDirectory == true, values.isSymbolicLink != true else {
                    throw TestHookError("OwnedProviderFixture source must be an existing directory")
                }
            } catch let error as TestHookError { throw error }
            catch { throw TestHookError("OwnedProviderFixture source must be an existing directory") }
        }

        /// No caller-supplied path or defaults domain can become a cleanup target.
        init() throws(TestHookError) {
            let root = FileManager.default.temporaryDirectory.appending(path: "Meguri-OwnedProvider-\(UUID().uuidString)", directoryHint: .isDirectory)
            let suite = "Meguri.OwnedProvider.\(UUID().uuidString)"
            guard let defaults = UserDefaults(suiteName: suite) else {
                throw TestHookError("Could not create disposable provider defaults")
            }
            do {
                guard !FileManager.default.fileExists(atPath: root.path) else {
                    throw TestHookError("Owned provider root already exists")
                }
                try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false,
                    attributes: [.posixPermissions: 0o700])
            } catch let error as TestHookError { throw error }
            catch { throw TestHookError("Could not create private owned provider root") }
            self.root = root
            self.suite = suite
            self.defaults = defaults
            collectionLocation = CollectionLocation(directory: root.appending(path: "Collection", directoryHint: .isDirectory),
                backupsDirectory: root.appending(path: "Backups", directoryHint: .isDirectory))
            installationDirectory = root.appending(path: "Installation", directoryHint: .isDirectory)
        }

        /// Called only after the captured collection returned its actual close result.
        /// A refusal never consumes ownership; a repeated successful Quit is harmless.
        func cleanup(afterClosing closed: Bool) -> Bool {
            guard closed else { return false }
            if cleaned { return true }
            if !removedRoot {
                do {
                    let values = try root.resourceValues(forKeys: [.isDirectoryKey, .isSymbolicLinkKey])
                    guard values.isDirectory == true, values.isSymbolicLink != true else { return false }
                    try FileManager.default.removeItem(at: root)
                    removedRoot = true
                } catch { return false }
            }
            defaults.removePersistentDomain(forName: suite)
            guard defaults.synchronize(), defaults.persistentDomain(forName: suite) == nil else { return false }
            cleaned = true
            return true
        }
    }
}
#endif
