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
        /// a directory, installed as the collection directory. The app writes
        /// what is installed, so it is always a copy: a link given as `source`
        /// is followed, and a directory containing a link is refused (copied,
        /// the link would let the app write to a committed fixture).
        func install(from source: URL, into location: CollectionLocation) throws(TestHookError) {
            guard bundleIdentifier.hasSuffix(Self.requiredSuffix) else {
                throw TestHookError(
                    "FixtureInstaller refuses to run: bundle id '\(bundleIdentifier)' does not end in '\(Self.requiredSuffix)'"
                )
            }
            let files = FileManager.default
            let given = source.path(percentEncoded: false)
            let source = source.resolvingSymlinksInPath()
            var isDirectory: ObjCBool = false
            guard files.fileExists(atPath: source.path(percentEncoded: false), isDirectory: &isDirectory) else {
                throw TestHookError("-MeguriFixture: no file or directory at \(given)")
            }
            if isDirectory.boolValue, let link = Self.firstLink(under: source) {
                throw TestHookError("-MeguriFixture: \(given) contains a symbolic link (\(link)); put a copy of its target in the fixture")
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
                throw TestHookError("-MeguriFixture: could not install \(given): \(error.localizedDescription)")
            }
        }

        /// The path, relative to `directory`, of the first symbolic link in
        /// its tree (the enumerator lists links without following them).
        private static func firstLink(under directory: URL) -> String? {
            let root = directory.standardizedFileURL.pathComponents.count
            let entries = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: [.isSymbolicLinkKey])
            while let url = entries?.nextObject() as? URL {
                if (try? url.resourceValues(forKeys: [.isSymbolicLinkKey]).isSymbolicLink) == true {
                    return url.standardizedFileURL.pathComponents.dropFirst(root).joined(separator: "/")
                }
            }
            return nil
        }
    }
}
#endif
