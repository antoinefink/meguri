import Foundation

/// Where the collection lives (KTD12): one directory in the app's private
/// Application Support holding `collection.anki2` and its media. The test
/// hooks install fixtures here (R1963).
nonisolated public struct CollectionLocation: Sendable, Hashable {
    public let directory: URL

    public init(directory: URL) {
        self.directory = directory
    }

    public var collectionFile: URL {
        directory.appending(path: "collection.anki2", directoryHint: .notDirectory)
    }

    /// `<Application Support>/Collection/` in the app's container.
    public static var standard: CollectionLocation {
        CollectionLocation(directory: URL.applicationSupportDirectory.appending(path: "Collection", directoryHint: .isDirectory))
    }
}
