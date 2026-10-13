#if MEGURI_TEST_HOOKS
import AppModel
import Foundation

extension TestHooks {
    /// Installs a fixture collection into the app's collection location
    /// (R1963), replacing whatever is there. It runs only when the bundle id
    /// ends in `.dev`: a Debug or Profile build, whose container is not the
    /// daily-driver app's, so a fixture can never replace a real collection.
    struct FixtureInstaller {
        static let requiredSuffix = ".dev"

        let bundleIdentifier: String

        /// `source` is a collection file, installed as `collection.anki2`, or
        /// a directory, installed as the collection directory.
        func install(from source: URL, into location: CollectionLocation) throws(TestHookError) {
            guard bundleIdentifier.hasSuffix(Self.requiredSuffix) else {
                throw TestHookError(
                    "FixtureInstaller refuses to run: bundle id '\(bundleIdentifier)' does not end in '\(Self.requiredSuffix)'"
                )
            }
            let files = FileManager.default
            var isDirectory: ObjCBool = false
            guard files.fileExists(atPath: source.path(percentEncoded: false), isDirectory: &isDirectory) else {
                throw TestHookError("-MeguriFixture: no file or directory at \(source.path(percentEncoded: false))")
            }
            do {
                if files.fileExists(atPath: location.directory.path(percentEncoded: false)) {
                    try files.removeItem(at: location.directory)
                }
                try files.createDirectory(at: location.directory.deletingLastPathComponent(), withIntermediateDirectories: true)
                if isDirectory.boolValue {
                    try files.copyItem(at: source, to: location.directory)
                } else {
                    try files.createDirectory(at: location.directory, withIntermediateDirectories: false)
                    try files.copyItem(at: source, to: location.collectionFile)
                }
            } catch {
                throw TestHookError("-MeguriFixture: could not install \(source.path(percentEncoded: false)): \(error.localizedDescription)")
            }
        }
    }
}
#endif
