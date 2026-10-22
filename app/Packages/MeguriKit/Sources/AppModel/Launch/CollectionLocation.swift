import Foundation

/// Where the collection lives (KTD12): one directory in the app's private
/// Application Support holding `collection.anki2` and its media. The test
/// hooks install fixtures here (R1963).
nonisolated public struct CollectionLocation: Sendable, Hashable {
    public let directory: URL
    public let backupsDirectory: URL

    public init(directory: URL, backupsDirectory: URL? = nil) {
        self.directory = directory
        self.backupsDirectory = backupsDirectory ?? directory.deletingLastPathComponent().appending(path: "Backups", directoryHint: .isDirectory)
    }

    public var collectionFile: URL {
        directory.appending(path: "collection.anki2", directoryHint: .notDirectory)
    }

    /// `<Application Support>/Collection/` in the app's container.
    public static var standard: CollectionLocation {
        #if os(iOS)
        let backups = URL.documentsDirectory.appending(path: "Backups", directoryHint: .isDirectory)
        #else
        let backups = URL.applicationSupportDirectory.appending(path: "Backups", directoryHint: .isDirectory)
        #endif
        return CollectionLocation(directory: URL.applicationSupportDirectory.appending(path: "Collection", directoryHint: .isDirectory),
                                  backupsDirectory: backups)
    }
}
